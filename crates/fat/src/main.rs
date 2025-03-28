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
    // let root_server_ep = sel4::CPtr::from_bits(1).cast::<sel4::cap_type::Endpoint>();

    // sel4::with_ipc_buffer_mut(|ipc_buf| {
    // ipc_buf.msg_regs_mut()[0] = 0xbeefcafe
    // });
    // sel4::debug_println!("after accessing ipc buffer");
    // let msg_info = sel4::MessageInfoBuilder::default().label(1).length(1).build();
    // root_server_ep.call(msg_info);

    sel4::debug_println!(
        "Hello world! I am test_app ^_^! I got arg[0] = {}",
        smos_runtime::args::args().nth(0).unwrap()
    );

    // let slot = cspace.alloc_slot().expect("Failed to alloc slot");
    // let window_hndl_cap = rs_conn.window_create(0xA0000000, 4096, Some(cspace.to_absolute_cptr(slot))).expect("blah");
    // let conn_slot = cspace.alloc_slot().expect("Failed to alloc slot");
    // let fs_conn = rs_conn.conn_create::<ObjectServerConnection>(&cspace.to_absolute_cptr(conn_slot), "BOOT_FS").expect("foo");
    // let obj = rs_conn.obj_create(None, 4096, sel4::CapRights::all(), None).expect("bar");
    // rs_conn.view(&window_hndl_cap, &obj, 0, 0, 4096, sel4::CapRights::all());
    // sel4::debug_println!("{:?}", obj);

    // rs_conn.window_destroy(window_hndl_cap);
    // @alwin: Process badged caps and the caps provided to clients to communicate with the root
    // server look the same, so I think they are deleted by the capability revocation. Need to do
    // something so they look different

    loop {}
    unreachable!()
}
