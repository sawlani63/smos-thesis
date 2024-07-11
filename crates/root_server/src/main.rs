//
// Copyright 2023, Colias Group, LLC
//
// SPDX-License-Identifier: BSD-2-Clause
//

#![no_std]
#![no_main]

mod arith;
mod bootstrap;
mod cspace;
mod debug;
mod dma;
mod limits;
mod mapping;
mod page;
mod uart;
mod ut;
mod util;
#[macro_use]
mod printing;
mod clock;
mod connection;
mod elf_load;
mod fault;
mod frame_table;
mod handle;
mod heap;
mod irq;
mod object;
mod proc;
mod stack;
mod syscall;
mod tests;
mod view;
mod vm;
#[rustfmt::skip]
mod vmem_layout;
mod window;
extern crate alloc;

use crate::bootstrap::smos_bootstrap;
use crate::clock::{clock_init, register_timer};
use crate::cspace::{CSpace, CSpaceTrait, UserCSpace};
use crate::debug::debug_print_bootinfo;
use crate::fault::handle_fault;
use crate::frame_table::FrameTable;
use crate::handle::create_handle_cap_table;
use crate::handle::RootServerResource;
use crate::heap::initialise_heap;
use crate::irq::IRQDispatch;
use crate::mapping::map_frame;
use crate::page::PAGE_SIZE_4K;
use crate::printing::print_init;
use crate::proc::{start_process, MAX_PID};
use crate::syscall::handle_syscall;
use crate::tests::run_tests;
use crate::uart::uart_init;
use crate::ut::{UTTable, UTWrapper};
use crate::util::alloc_retype;
use sel4::cap::VSpace;
use sel4::{BootInfo, BootInfoPtr};
use sel4_root_task::{root_task, Never};
use smos_server::event::*;
use smos_server::handle_capability::HandleCapabilityTable;
use vmem_layout::PROCESS_STACK_TOP;
// use crate::connection::publish_boot_fs;

const BFS_CONTENTS: &[u8] = include_bytes!(env!("BOOT_FS_ELF"));
// const LOADER_CONTENTS: &[u8] = include_bytes!(env!("LOADER_ELF"));
//const TEST_ELF_CONTENTS: &[u8] = include_bytes!(env!("TEST_ELF"));

/* Create and endpoint and a bounding notification object. These are never freed so we don't keep
track of the UTs used to allocate them.  */
fn ipc_init(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
) -> Result<(sel4::cap::Endpoint, sel4::cap::Notification), sel4::Error> {
    /* Create the notification */
    let (ntfn, _) = alloc_retype::<sel4::cap_type::Notification>(
        cspace,
        ut_table,
        sel4::ObjectBlueprint::Notification,
    )?;

    /* Bind it to the TCB */
    sel4::init_thread::slot::TCB
        .cap()
        .tcb_bind_notification(ntfn)?;

    /* Create the endpoint */
    let (ep, _) = alloc_retype::<sel4::cap_type::Endpoint>(
        cspace,
        ut_table,
        sel4::ObjectBlueprint::Endpoint,
    )?;
    return Ok((ep, ntfn));
}

fn callback(_idk: usize, _idk2: *const ()) {
    log_rs!("hey there!");
}

pub type RSReplyWrapper = (sel4::cap::Reply, UTWrapper);

fn syscall_loop(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
    frame_table: &mut FrameTable,
    handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
    ep: sel4::cap::Endpoint,
    irq_dispatch: &mut IRQDispatch,
    sched_control: sel4::cap::SchedControl,
) -> Result<(), sel4::Error> {
    let mut reply: RSReplyWrapper =
        alloc_retype::<sel4::cap_type::Reply>(cspace, ut_table, sel4::ObjectBlueprint::Reply)?;
    let mut reply_msg_info = None;

    log_rs!("setting timer...");
    register_timer(5000000000, callback, core::ptr::null())?;

    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let recv_slot = cspace
        .root_cnode()
        .relative_bits_with_depth(recv_slot_inner as u64, sel4::WORD_SIZE);
    sel4::with_ipc_buffer_mut(|ipc_buf| {
        ipc_buf.set_recv_slot(&recv_slot);
    });

    loop {
        let (msg, mut badge) = {
            if reply_msg_info.is_some() {
                ep.reply_recv(reply_msg_info.unwrap(), reply.0)
            } else {
                ep.recv(reply.0)
            }
        };

        let label = msg.label();

        reply_msg_info = match decode_entry_type(badge.try_into().unwrap()) {
            EntryType::Irq => {
                badge = irq_dispatch.handle_irq(badge as usize);
                None
            }
            EntryType::Signal => {
                panic!("RS shouldn't recieve signals");
            }
            EntryType::Invocation(pid) => {
                /* We recieved a syscall from something in the system*/
                handle_syscall(
                    msg,
                    pid,
                    cspace,
                    frame_table,
                    ut_table,
                    handle_cap_table,
                    sched_control,
                    ep,
                    recv_slot,
                    reply,
                )
            }
            EntryType::Fault(pid) => {
                /* We must have recieved a message from a fault handler endpoint */
                handle_fault(cspace, frame_table, ut_table, reply, msg, pid)
                /* @alwin: what to actually do when this returns None. This means that the faulting
                thread won't be resumed, so what we should we do? First of all, we can't use the
                same reply object, so we should destroy it and allocate a new one (otherwise
                we get the reply object has an unexecuted reply warning thing, but this might
                actually just be benign). We should probably also clean up the process instead of
                just leaving it laying around */
            }
        };

        /* Should you always do this if reply_msg_info is none? */
        if reply_msg_info.is_none() {
            reply = alloc_retype::<sel4::cap_type::Reply>(
                cspace,
                ut_table,
                sel4::ObjectBlueprint::Reply,
            )?;
        }
    }

    Ok(())
}

extern "C" fn main_continued(
    bootinfo_raw: *const BootInfo,
    cspace_ptr: *mut CSpace,
    ut_table_ptr: *mut UTTable,
) -> ! {
    log_rs!("Switched to new stack...");

    // Safety: This is reconstructing bootinfo from the ptr passed into main(). Hence, this must be a valid pointer.
    let bootinfo = unsafe { sel4::BootInfoPtr::new(bootinfo_raw) };

    /* Get the cspace and ut_table back. This is slightly cursed because these exist on
    the old stack. Don't think anything bad will come from it, but kind of a weird situation.
    Really, it might be better to manually copy the structs to the new stack, but that
    seems much more painful */

    let cspace = unsafe { &mut *cspace_ptr };
    let ut_table = unsafe { &mut *ut_table_ptr };

    let (ipc_ep, ntfn) = ipc_init(cspace, ut_table).expect("Failed to initialize IPC");
    let mut frame_table = FrameTable::init(sel4::init_thread::slot::VSPACE.cap());

    let mut irq_dispatch = IRQDispatch::new(
        sel4::init_thread::slot::IRQ_CONTROL.cap(),
        ntfn,
        NTFN_IRQ_BITS,
        IRQ_IDENT_BADGE_BITS,
    );

    initialise_heap(cspace, ut_table).expect("Failed to initialize heap!");

    /* Set up the handle capability table*/
    let mut handle_cap_table = HandleCapabilityTable::new(
        create_handle_cap_table(cspace, ipc_ep).expect("Failed to set up handle cap table"),
    );

    run_tests(cspace, ut_table, &mut frame_table);

    log_rs!("TESTS PASSED!");

    clock_init(cspace, &mut irq_dispatch, ntfn).expect("Failed to initialize clock");

    // publish_boot_fs(ipc_ep);

    let proc = start_process(
        cspace,
        ut_table,
        &mut frame_table,
        bootinfo.sched_control().index(0).cap(),
        "test_app",
        ipc_ep,
        BFS_CONTENTS,
        None,
        None,
        254,
    )
    .expect("Failed to start first process");

    // @alwin: Consider putting syscall loop in a struct parameterised by the type of the server
    // and using it like this instead of specifying the type of server in SMOSInvocation::new
    syscall_loop(
        cspace,
        ut_table,
        &mut frame_table,
        &mut handle_cap_table,
        ipc_ep,
        &mut irq_dispatch,
        bootinfo.sched_control().index(0).cap(),
    )
    .expect("Something went wrong in the syscall loop");

    sel4::init_thread::slot::TCB
        .cap()
        .tcb_suspend()
        .expect("Failed to suspend");
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
        let (frame, _) = alloc_retype::<sel4::cap_type::SmallPage>(
            &mut cspace,
            &mut ut_table,
            sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage),
        )
        .expect("Failed to alloc_retype");

        map_frame(
            &mut cspace,
            &mut ut_table,
            frame.cast(),
            sel4::init_thread::slot::VSPACE.cap(),
            vaddr,
            sel4::CapRightsBuilder::all().build(),
            sel4::VmAttributes::DEFAULT,
            None,
        )
        .expect("Failed to map stack page");
        vaddr += PAGE_SIZE_4K;
    }

    log_rs!("Switching to new stack (stack_top = 0x{:x})...", vaddr);

    stack::utils_run_on_stack(vaddr, main_continued, bootinfo, &mut cspace, &mut ut_table);

    sel4::init_thread::slot::TCB.cap().tcb_suspend()?;
    unreachable!()
}
