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
extern crate alloc;

use ::elf::ElfBytes;
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
use crate::elf_load::load_elf;

const IRQ_EP_BADGE: usize = 1 << (sel4_sys::seL4_BadgeBits - 1);
const APP_EP_BADGE: usize = 101;
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

// struct UserProcess {
//     vspace: (usize, UTWrapper),
// }


// fn create_one_lvl_cspace(cspace: &mut CSpace,ut_table: &mut UTTable) -> Result<CSpace> {
    
// }

fn init_process_stack(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable, vspace: sel4::cap::VSpace,) -> Result<usize, sel4::Error> {
    // @alwin: For now, the stack is one page big
    let stack_top = vmem_layout::PROCESS_STACK_TOP;
    let mut stack_bottom = stack_top - PAGE_SIZE_4K;

    for _ in 0..vmem_layout::USER_DEFAULT_STACK_PAGES {
        // @alwin: fix! this leaks memory
        let frame = frame_table.alloc_frame(cspace, ut_table).ok_or(sel4::Error::NotEnoughMemory)?;
        let user_frame = sel4::CPtr::from_bits(cspace.alloc_slot()?.try_into().unwrap()).cast::<sel4::cap_type::UnspecifiedFrame>();
        cspace.root_cnode().relative(user_frame).copy(&cspace.root_cnode().relative(frame_table.frame_from_ref(frame).get_cap()), sel4::CapRightsBuilder::all().build());
        map_frame(cspace, ut_table, user_frame, vspace, stack_bottom, sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None);

        stack_bottom -= PAGE_SIZE_4K;
    }

    return Ok(stack_top);
}




// @alwin: this leaks cslots and caps!
fn start_first_process(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable,
                       sched_control: sel4::cap::SchedControl, name: &str, ep: sel4::cap::Endpoint) -> Result<(), sel4::Error> {

    /* Create a VSpace */
    let mut vspace = alloc_retype::<sel4::cap_type::VSpace>(cspace, ut_table, sel4::ObjectBlueprint::Arch(
                                                        sel4::ObjectBlueprintArch::SeL4Arch(
                                                        sel4::ObjectBlueprintAArch64::VSpace)))?;

    /* assign the vspace to an asid pool */
    sel4::init_thread::slot::ASID_POOL.cap().asid_pool_assign(vspace.0)?;

    /* Create a simple 1 level CSpace */
    let mut proc_cspace = UserCSpace::new(cspace, ut_table, false)?;

    /* Create an IPC buffer */
    // @alwin: THis should really probs be an alloc_frame()
    let mut ipc_buffer = alloc_retype::<sel4::cap_type::SmallPage>(cspace, ut_table, sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage))?;

    /* allocate a new slot in the target cspace which we will mint a badged endpoint cap into --
     * the badge is used to identify the process */
     let mut proc_ep = proc_cspace.alloc_slot()?;

     /* now mutate the cap, thereby setting the badge */
     proc_cspace.root_cnode().relative_bits_with_depth(proc_ep.try_into().unwrap(), sel4::WORD_SIZE)
                           .mint(&cspace.root_cnode().relative(ep),
                                 sel4::CapRightsBuilder::all().build(),
                                 APP_EP_BADGE.try_into().unwrap())?;


    /* Create a new TCB object */
    let mut tcb = alloc_retype::<sel4::cap_type::Tcb>(cspace, ut_table, sel4::ObjectBlueprint::Tcb)?;


    /* Configure the TCB */
    // @alwin: changing the WORD_SIZE to 64 causes a panic
    tcb.0.tcb_configure(proc_cspace.root_cnode(), sel4::CNodeCapData::new(0, 0), vspace.0, vmem_layout::PROCESS_IPC_BUFFER.try_into().unwrap(), ipc_buffer.0)?;


    /* Create scheduling context */
    let mut sched_context = alloc_retype::<sel4::cap_type::SchedContext>(cspace, ut_table, sel4::ObjectBlueprint::SchedContext{ size_bits: sel4_sys::seL4_MinSchedContextBits.try_into().unwrap()})?;

    /* Configure the scheduling context to use the first core with budget equal to period */
    sched_control.sched_control_configure_flags(sched_context.0, 1000, 1000, 0, 0, 0);

    // @alwin: The endpoint passed in here should actually be badged like the other one
    tcb.0.tcb_set_sched_params(sel4::init_thread::slot::TCB.cap(), 0, 0, sched_context.0, ep)?;

    tcb.0.debug_name(name.as_bytes());

    // @alwin: cpio stuff here

    let elf = ElfBytes::<elf::endian::AnyEndian>::minimal_parse(TEST_ELF_CONTENTS).or(Err(sel4::Error::InvalidArgument))?;

    let sp = init_process_stack(cspace, ut_table, frame_table, vspace.0)?;

    load_elf(cspace, ut_table, frame_table, vspace.0, &elf);


    map_frame(cspace, ut_table, ipc_buffer.0.cast(), vspace.0, vmem_layout::PROCESS_IPC_BUFFER,
          sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None)?;

    let mut user_context = sel4::UserContext::default();
    *user_context.pc_mut() = elf.ehdr.e_entry;
    *user_context.sp_mut() = sp.try_into().unwrap();

    tcb.0.tcb_write_registers(true, 2, &mut user_context)?;

    return Ok(());
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

    start_first_process(cspace, ut_table, &mut frame_table, bootinfo.sched_control().index(0).cap(), "test_app", ipc_ep).expect("Failed to start first process");

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
