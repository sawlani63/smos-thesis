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

use sel4::BootInfo;
use sel4_root_task::{root_task, Never};
use crate::debug::debug_print_bootinfo;
use crate::bootstrap::smos_bootstrap;
use sel4::CPtr;

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

    log_rs!("Starting...");
    // Name the thread
    sel4::init_thread::slot::TCB.cap().debug_name(b"SMOS:root");

    let cspace = smos_bootstrap(bootinfo);

    let blueprint = sel4::ObjectBlueprint::Notification;

    let untyped = {
        let slot = bootinfo.untyped().start()
            + bootinfo
                .untyped_list()
                .iter()
                .position(|desc| {
                    !desc.is_device() && desc.size_bits() >= blueprint.physical_size_bits()
                })
                .unwrap();

        CPtr::from_bits(slot.try_into().unwrap()).cast::<sel4::cap_type::Untyped>()
    };

    let mut empty_slots = bootinfo.empty().range();
    let unbadged_notification_slot = empty_slots.next().unwrap();
    let badged_notification_slot = empty_slots.next().unwrap();
    let unbadged_notification = CPtr::from_bits(unbadged_notification_slot.try_into().unwrap()).cast::<sel4::cap_type::Notification>();
    let badged_notification = CPtr::from_bits(badged_notification_slot.try_into().unwrap()).cast::<sel4::cap_type::Notification>();


    let cnode = sel4::init_thread::slot::CNODE.cap();

    untyped.untyped_retype(
        &blueprint,
        &cnode.relative_self(),
        unbadged_notification_slot,
        1,
    )?;

    let badge = 0x1337;

    cnode.relative(badged_notification).mint(
        &cnode.relative(unbadged_notification),
        sel4::CapRights::write_only(),
        badge,
    )?;

    badged_notification.signal();

    let (_, observed_badge) = unbadged_notification.wait();

    sel4::debug_println!("badge = {:#x}", badge);
    assert_eq!(observed_badge, badge);

    sel4::debug_println!("TEST_PASS");

    sel4::init_thread::slot::TCB.cap().tcb_suspend()?;
    unreachable!()
}
