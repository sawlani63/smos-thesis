#![no_std]
#![no_main]
#![feature(core_intrinsics)]
#![allow(internal_features)]
#![feature(lang_items)]

use core::arch::global_asm;
use core::ptr;
use sel4::CapTypeForFrameObjectOfFixedSize;
use sel4_panicking::catch_unwind;
use smos_common::client_connection::ClientConnection;
use smos_common::connection::{ObjectServerConnection, RootServerConnection};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::{smos_declare_main, Never};

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) -> sel4::Result<Never> {
    sel4::debug_println!(
        "Hello world! I am test_app ^_^! I got arg[0] = {}",
        smos_runtime::args::args().nth(0).unwrap()
    );

    loop {}
    unreachable!()
}
