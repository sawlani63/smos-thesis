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
mod heap;
mod elf_load;
mod proc;
extern crate alloc;

use sel4::cap::VSpace;
use sel4::{BootInfo, BootInfoPtr};
use sel4_root_task::{root_task, Never};
use vmem_layout::PROCESS_STACK_TOP;
use crate::debug::debug_print_bootinfo;
use crate::bootstrap::smos_bootstrap;
use crate::uart::uart_init;
use crate::cspace::{CSpace, UserCSpace, CSpaceTrait};
use crate::ut::{UTTable, UTWrapper};
use crate::printing::print_init;
use crate::page::PAGE_SIZE_4K;
use crate::util::alloc_retype;
use crate::mapping::map_frame;
use crate::tests::run_tests;
use crate::frame_table::FrameTable;
use crate::clock::{clock_init, register_timer};
use crate::irq::IRQDispatch;
use crate::heap::initialise_heap;
use crate::proc::{start_first_process};

const IRQ_EP_BADGE: usize = 1 << (sel4_sys::seL4_BadgeBits - 1);
const IRQ_IDENT_BADGE_BITS: usize = IRQ_EP_BADGE - 1;

const TEST_ELF_CONTENTS: &[u8] = include_bytes!(env!("TEST_ELF"));

/* Create and endpoint and a bounding notification object. These are never freed so we don't keep
   track of the UTs used to allocate them.  */
fn ipc_init(cspace: &mut CSpace, ut_table: &mut UTTable)
            -> Result<(sel4::cap::Endpoint, sel4::cap::Notification), sel4::Error> {

    /* Create the notification */
    let (ntfn, _) = alloc_retype::<sel4::cap_type::Notification>(cspace, ut_table, sel4::ObjectBlueprint::Notification)?;

    /* Bind it to the TCB */
    sel4::init_thread::slot::TCB.cap().tcb_bind_notification(ntfn)?;

    /* Create the endpoint */
    let (ep, _) = alloc_retype::<sel4::cap_type::Endpoint>(cspace, ut_table, sel4::ObjectBlueprint::Endpoint)?;
    return Ok((ep, ntfn));
}


fn callback(_idk: usize, _idk2: *const ()) {
    log_rs!("hey there!");
}

fn syscall_loop(cspace: &mut CSpace, ut_table: &mut UTTable, ep: sel4::cap::Endpoint, irq_dispatch: &mut IRQDispatch)
               -> Result<(), sel4::Error> {

    let (reply, reply_ut) = alloc_retype::<sel4::cap_type::Reply>(cspace, ut_table, sel4::ObjectBlueprint::Reply)?;

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

extern "C" fn main_continued(bootinfo_raw: *const BootInfo, cspace_ptr : *mut CSpace, ut_table_ptr: *mut UTTable) -> ! {
    log_rs!("Switched to new stack...");

    // Safety: This is reconstructing bootinfo from the ptr passed into main(). Hence, this must be a valid pointer.
    let bootinfo = unsafe { sel4::BootInfoPtr::new(bootinfo_raw) };

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

    initialise_heap(cspace, ut_table).expect("Failed to initialize heap!");

    run_tests(cspace, ut_table, &mut frame_table);

    log_rs!("TESTS PASSED!");

    clock_init(cspace, &mut irq_dispatch, ntfn).expect("Failed to initialize clock");

    let proc = start_first_process(cspace, ut_table, &mut frame_table,
                                   bootinfo.sched_control().index(0).cap(), "test_app", ipc_ep,
                                   TEST_ELF_CONTENTS).expect("Failed to start first process");

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
        let (frame, _) = alloc_retype::<sel4::cap_type::SmallPage>(&mut cspace, &mut ut_table,
                                            sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage))
                                            .expect("Failed to alloc_retype");

        map_frame(&mut cspace, &mut ut_table, frame.cast(), sel4::init_thread::slot::VSPACE.cap(), vaddr,
                  sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None)
                  .expect("Failed to map stack page");
        vaddr += PAGE_SIZE_4K;
     }

    log_rs!("Switching to new stack (stack_top = 0x{:x})...", vaddr);

    stack::utils_run_on_stack(vaddr, main_continued, bootinfo, &mut cspace, &mut ut_table);

    sel4::init_thread::slot::TCB.cap().tcb_suspend()?;
    unreachable!()
}
