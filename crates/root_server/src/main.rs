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
mod printing;

use core::panic::PanicInfo;

use sel4::BootInfo;
use sel4_root_task::{root_task, Never};
use crate::debug::debug_print_bootinfo;
use crate::bootstrap::smos_bootstrap;
use sel4::CPtr;
use crate::uart::{uart_init};
use crate::cspace::CSpace;
use crate::ut::UTTable;
use crate::printing::print_init;


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

    log_rs!("TEST_PASS");
    sel4::init_thread::slot::TCB.cap().tcb_suspend()?;
    unreachable!()
}
