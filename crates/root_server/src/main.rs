//
// Copyright 2023, Colias Group, LLC
//
// SPDX-License-Identifier: BSD-2-Clause
//

#![no_std]
#![no_main]


mod debug;
mod cspace;
mod page;
mod bootstrap;
mod arith;
mod ut;
mod limits;
mod bitfield;
mod dma;
mod mapping;
mod util;
mod uart;
#[macro_use]
mod printing;
mod vmem_layout;
mod stack;
mod frame_table;
mod tests;
mod clock;
mod irq;

use sel4_root_task::{root_task, Never};
use crate::debug::debug_print_bootinfo;
use crate::bootstrap::smos_bootstrap;
use crate::uart::uart_init;
use crate::cspace::CSpace;
use crate::ut::UTTable;
use crate::printing::print_init;
use crate::page::PAGE_SIZE_4K;
use crate::util::alloc_retype;
use crate::mapping::map_frame;
use crate::tests::run_tests;
use crate::frame_table::FrameTable;
use crate::clock::{clock_init, register_timer};
use crate::irq::IRQDispatch;

const IRQ_EP_BADGE: usize = 1 << (sel4_sys::seL4_BadgeBits - 1);
const IRQ_IDENT_BADGE_BITS: usize = IRQ_EP_BADGE - 1;

/* Create and endpoint and a bounding notification object. These are never freed so we don't keep
   track of the UTs used to allocate them.  */
fn ipc_init(cspace: &mut CSpace, ut_table: &mut UTTable)
            -> Result<(sel4::cap::Endpoint, sel4::cap::Notification), sel4::Error> {

    /* Create the notification */
    let (ntfn_cptr, _) = alloc_retype(cspace, ut_table, sel4::ObjectBlueprint::Notification)?;
    let ntfn = sel4::CPtr::from_bits(ntfn_cptr.try_into().unwrap()).cast::<sel4::cap_type::Notification>();

    /* Bind it to the TCB */
    sel4::init_thread::slot::TCB.cap().tcb_bind_notification(ntfn)?;

    /* Create the endpoint */
    let (ep_cptr, _) = alloc_retype(cspace, ut_table, sel4::ObjectBlueprint::Endpoint)?;
    let ep = sel4::CPtr::from_bits(ep_cptr.try_into().unwrap()).cast::<sel4::cap_type::Endpoint>();
    return Ok((ep, ntfn));
}


fn callback(_idk: usize, _idk2: *const ()) {
    log_rs!("hey there!");
}

fn syscall_loop(cspace: &mut CSpace, ut_table: &mut UTTable, ep: sel4::cap::Endpoint, irq_dispatch: &mut IRQDispatch)
               -> Result<(), sel4::Error> {

    let (reply_cptr, reply_ut) = alloc_retype(cspace, ut_table, sel4::ObjectBlueprint::Reply)?;
    let reply = sel4::CPtr::from_bits(reply_cptr.try_into().unwrap()).cast::<sel4::cap_type::Reply>();

    let mut have_reply = false;
    let mut reply_msg_info = sel4::MessageInfoBuilder::default().label(0)
                                                                .caps_unwrapped(0)
                                                                .extra_caps(0)
                                                                .length(0)
                                                                .build();

    log_rs!("setting timer...");
    register_timer(5000000000, callback, core::ptr::null())?;

    loop {
        let (msg, mut badge) = {
            if have_reply {
                ep.reply_recv(reply_msg_info, reply)
            } else {
                ep.recv(reply)
            }
        };

        let label = msg.label();

        if badge & IRQ_EP_BADGE as u64 != 0 {
            badge = irq_dispatch.handle_irq(badge as usize);
            have_reply = false;
            // Handle IRQ notification
        } else if label == sel4_sys::seL4_Fault_tag::seL4_Fault_NullFault {
            // IPC message
        } else {
            // Some kind of fault
        }

        reply_msg_info = sel4::MessageInfoBuilder::default().label(0)
                                                            .caps_unwrapped(0)
                                                            .extra_caps(0)
                                                            .length(0)
                                                            .build();
    }

    Ok(())
}

extern "C" fn main_continued(cspace_ptr : *mut CSpace, ut_table_ptr: *mut UTTable) -> ! {
    log_rs!("Switched to new stack...");

    /* Get the cspace and ut_table back. This is slightly cursed because these exist on
       the old stack. Don't think anything bad will come from it, but kind of a weird situation.
       Really, it might be better to manually copy the structs to the new stack, but that
       seems much more painful */

    let cspace = unsafe {&mut *cspace_ptr};
    let ut_table = unsafe {&mut *ut_table_ptr};

    let (ipc_ep, ntfn) = ipc_init(cspace, ut_table).expect("Failed to initialize IPC");
    let mut frame_table = FrameTable::init(sel4::init_thread::slot::VSPACE.cap());

    let mut irq_dispatch = IRQDispatch::new(sel4::init_thread::slot::IRQ_CONTROL.cap(), ntfn,
                                            IRQ_EP_BADGE, IRQ_IDENT_BADGE_BITS);

    run_tests(cspace, ut_table, &mut frame_table);

    log_rs!("TESTS PASSED!");

    clock_init(cspace, &mut irq_dispatch, ntfn).expect("Failed to initialize clock");

    syscall_loop(cspace, ut_table, ipc_ep, &mut irq_dispatch).expect("Something went wrong in the syscall loop");

    sel4::init_thread::slot::TCB.cap().tcb_suspend().expect("Failed to suspend");
    unreachable!()
}

#[root_task]
fn main(bootinfo: &sel4::BootInfoPtr) -> sel4::Result<Never> {
    sel4::debug_println!("Starting...");
    debug_print_bootinfo(bootinfo);

    // Name the thread
    sel4::init_thread::slot::TCB.cap().debug_name(b"SMOS:root");

    /* Set up CSpce and untyped tables */
    let (mut cspace, mut ut_table) = smos_bootstrap(bootinfo)?;

    /* Setup the uart driver and configure printing with it  */
    let uart_printer = uart_init(&mut cspace, &mut ut_table)?;
    print_init(uart_printer);

    /*
     * After this point, sel4::debug_print!() should no longer be used.
     * log_rs, println!() and print!() should be used instead, as these use
     * the more efficient internal UART driver instead of relying on seL4_DebugPutChar.
     */

     // Allocate and switch to a bigger stack with a guard page
     let mut vaddr = vmem_layout::STACK;
     for _i in 0..vmem_layout::STACK_PAGES {
        let (frame_cptr, _) = alloc_retype(&mut cspace, &mut ut_table,
                                            sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage))
                                            .expect("Failed to alloc_retype");

        let frame = sel4::CPtr::from_bits(frame_cptr.try_into().unwrap()).cast::<sel4::cap_type::SmallPage>();
        map_frame(&mut cspace, &mut ut_table, frame.cast(), sel4::init_thread::slot::VSPACE.cap(), vaddr,
                  sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None)
                  .expect("Failed to map stack page");
        vaddr += PAGE_SIZE_4K;
     }

    log_rs!("Switching to new stack (stack_top = 0x{:x})...", vaddr);

    stack::utils_run_on_stack(vaddr, main_continued, &mut cspace, &mut ut_table);

    sel4::init_thread::slot::TCB.cap().tcb_suspend()?;
    unreachable!()
}
