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
mod fault;
mod syscall;
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
use crate::util::{alloc_retype, IRQ_EP_BIT, FAULT_EP_BIT, IRQ_IDENT_BADGE_BITS};
use crate::mapping::map_frame;
use crate::tests::run_tests;
use crate::frame_table::FrameTable;
use crate::clock::{clock_init, register_timer};
use crate::irq::IRQDispatch;
use crate::heap::initialise_heap;
use crate::proc::{start_process, MAX_PID};
use crate::fault::handle_fault;
use crate::syscall::handle_syscall;

// @alwin: The root server should be able to serve the following images:
//      * loader
//      * nfs_server (or whatever components this will eventually become i.e ethernet driver,
//                    virt?, etc)
//
// The root file server should start a loader with the correct arguments to load
// the NFS server. After this, something (either the root server or the NFS server),
// should start the login shell.
//
// Alternatively - the root server contains images for
//      * loader
//      * login_shell
//      * sosh
//      * nfs_server
//      * authentication details?
//
// And should start the login shell, which upon successful login will then start sosh, which
// can then start the NFS server and so on. Another thing, instead of the current approach where
// the loader tries the NFS server and then the boot file server if that fails, it should instead
// take in the name of the server to try. I think the first option is cleaner and less backdoory
// but this has certain consequences on the security state of the system. The NFS server is not
// started by a client, so where is its security context derived from?

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

    let mut reply_msg_info = None;

    log_rs!("setting timer...");
    register_timer(5000000000, callback, core::ptr::null())?;

    loop {
        let (msg, mut badge) = {
            if reply_msg_info.is_some() {
                ep.reply_recv(reply_msg_info.unwrap(), reply)
            } else {
                ep.recv(reply)
            }
        };

        let label = msg.label();

        if badge & IRQ_EP_BIT as u64 != 0 {
            // Handle IRQ notification
            badge = irq_dispatch.handle_irq(badge as usize);
            reply_msg_info = None
        } else if badge & FAULT_EP_BIT as u64 == 0 {
            /* We recieved a syscall from something in the system*/
            assert!(badge < MAX_PID.try_into().unwrap());
            log_rs!("Recieved a system call!");

            log_rs!("With label: {:x}", label);
            log_rs!("With MR: {:x}",  sel4::with_ipc_buffer(|buf| buf.msg_regs()[0]));

            reply_msg_info = handle_syscall(msg, badge);
        } else {
            /* We must have recieved a message from a fault handler endpoint */
            assert!(badge & FAULT_EP_BIT as u64 != 0);
            badge &= !FAULT_EP_BIT as u64;
            assert!(badge < MAX_PID as u64);

            reply_msg_info = handle_fault(msg, badge);
        }
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
                                            IRQ_EP_BIT, IRQ_IDENT_BADGE_BITS);

    initialise_heap(cspace, ut_table).expect("Failed to initialize heap!");

    run_tests(cspace, ut_table, &mut frame_table);

    log_rs!("TESTS PASSED!");

    clock_init(cspace, &mut irq_dispatch, ntfn).expect("Failed to initialize clock");

    let proc = start_process(cspace, ut_table, &mut frame_table,
                             bootinfo.sched_control().index(0).cap(), "test_app", ipc_ep,
                             TEST_ELF_CONTENTS).expect("Failed to start first process");

    // @alwin: Consider putting syscall loop in a struct parameterised by the type of the server
    // and using it like this instead of specifying the type of server in SMOSInvocation::new
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
