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

use core::panic::PanicInfo;

use sel4::BootInfo;
use sel4_root_task::{root_task, Never};
use crate::debug::debug_print_bootinfo;
use crate::bootstrap::smos_bootstrap;
use sel4::CPtr;
use crate::uart::uart_init;
use crate::cspace::CSpace;
use crate::ut::UTTable;
use crate::printing::print_init;
use crate::vmem_layout::{STACK, STACK_PAGES};
use crate::page::PAGE_SIZE_4K;
use crate::util::alloc_retype;
use crate::mapping::map_frame;

extern "C" fn main_continued(cspace_ptr : *mut CSpace, ut_table_ptr: *mut UTTable) {
    log_rs!("Switched to new stack...");

    sel4::init_thread::slot::TCB.cap().tcb_suspend().expect("Failed to suspend");
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
     for i in 0..vmem_layout::STACK_PAGES {
        let (frame_cptr, ut) = alloc_retype(&mut cspace, &mut ut_table,
                                            sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage))
                                            .expect("Failed to alloc_retype");

        let frame = sel4::CPtr::from_bits(frame_cptr.try_into().unwrap()).cast::<sel4::cap_type::SmallPage>();
        map_frame(&mut cspace, &mut ut_table, frame.cast(), sel4::init_thread::slot::VSPACE.cap(), vaddr,
                  sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None);
        vaddr += PAGE_SIZE_4K;
     }

    log_rs!("Switching to new stack (stack_top = 0x{:x})...", vaddr);

    stack::utils_run_on_stack(vaddr, main_continued, &mut cspace, &mut ut_table);

    sel4::init_thread::slot::TCB.cap().tcb_suspend()?;
    unreachable!()
}
