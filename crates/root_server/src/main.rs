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

use core::panic::PanicInfo;

use sel4::BootInfo;
use sel4_root_task::{root_task, Never};
use crate::debug::debug_print_bootinfo;
use crate::bootstrap::smos_bootstrap;
use sel4::CPtr;
use crate::uart::{uart_init, uart_put_char};
use crate::cspace::CSpace;
use crate::ut::UTTable;

macro_rules! log_rs {
    () => {
        sel4::debug_println!();
    };
    ($($arg:tt)*) => {{
        sel4::debug_print!("root_server|INFO: ");
        sel4::debug_println!($($arg)*);
    }};
}

#[root_task]
fn main(bootinfo: &sel4::BootInfoPtr) -> sel4::Result<Never> {
    debug_print_bootinfo(bootinfo);

    // Name the thread
    sel4::init_thread::slot::TCB.cap().debug_name(b"SMOS:root");

    let (mut cspace, mut ut_table) = smos_bootstrap(bootinfo)?;
    uart_init(&mut cspace, &mut ut_table);

    // @alwin: Now that UART has been initialized, we should be able to define and use a println!()
    // macro from here on which plugs into uart_put_char()

    sel4::debug_println!("TEST_PASS");

    sel4::init_thread::slot::TCB.cap().tcb_suspend()?;
    unreachable!()
}
