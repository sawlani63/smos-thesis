#![no_std]
#![no_main]
#![feature(core_intrinsics)]
#![allow(internal_features)]
#![feature(lang_items)]


use core::arch::global_asm;
use sel4_panicking::catch_unwind;
use core::ptr;
use sel4::CapTypeForFrameObjectOfFixedSize;
use smos_runtime::{smos_declare_main, Never};
use smos_cspace::SMOSUserCSpace;
use smos_client::syscall::{RootServerInterface, ClientConnection};
use smos_common::connection::RootServerConnection;

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, cspace: SMOSUserCSpace) -> sel4::Result<Never> {
    sel4::debug_println!("hi there");

    // let root_server_ep = sel4::CPtr::from_bits(1).cast::<sel4::cap_type::Endpoint>();

    // sel4::with_ipc_buffer_mut(|ipc_buf| {
        // ipc_buf.msg_regs_mut()[0] = 0xbeefcafe
    // });
    // sel4::debug_println!("after accessing ipc buffer");
    // let msg_info = sel4::MessageInfoBuilder::default().label(1).length(1).build();
    // root_server_ep.call(msg_info);
    rs_conn.test_simple(1);

    unreachable!()
}