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

use sel4::BootInfo;
use sel4_root_task::{root_task, Never};
use crate::debug::debug_print_bootinfo;
use crate::bootstrap::smos_bootstrap;

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
fn main(bootinfo: &sel4::BootInfo) -> sel4::Result<Never> {
    debug_print_bootinfo(bootinfo);

    log_rs!("Starting...");
    // Name the thread
    sel4::BootInfo::init_thread_tcb().debug_name(b"SMOS:root");

    let cspace = smos_bootstrap(bootinfo);





    let blueprint = sel4::ObjectBlueprint::Notification;

    let untyped = {
        let slot = bootinfo.untyped().start
            + bootinfo
                .untyped_list()
                .iter()
                .position(|desc| {
                    !desc.is_device() && desc.size_bits() >= blueprint.physical_size_bits()
                })
                .unwrap();
        sel4::BootInfo::init_cspace_local_cptr::<sel4::cap_type::Untyped>(slot)
    };

    let mut empty_slots = bootinfo.empty();
    let unbadged_notification_slot = empty_slots.next().unwrap();
    let badged_notification_slot = empty_slots.next().unwrap();
    let unbadged_notification = sel4::BootInfo::init_cspace_local_cptr::<
        sel4::cap_type::Notification,
    >(unbadged_notification_slot);
    let badged_notification = sel4::BootInfo::init_cspace_local_cptr::<sel4::cap_type::Notification>(
        badged_notification_slot,
    );



    let cnode = sel4::BootInfo::init_thread_cnode();

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

    sel4::BootInfo::init_thread_tcb().tcb_suspend()?;
    unreachable!()
}
