#![no_std]
#![no_main]
#![feature(core_intrinsics)]
#![allow(internal_features)]
#![feature(lang_items)]

use smos_common::connection::RootServerConnection;
use smos_common::syscall::RootServerInterface;
use smos_cspace::SMOSUserCSpace;
use smos_runtime::{smos_declare_main, Never};
extern crate alloc;

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, _cspace: SMOSUserCSpace) -> sel4::Result<Never> {
    sel4::debug_println!("Hello world! I am init ^_^! I will now initialize the system...");

    /* Start the ethernet driver */
    rs_conn
        .process_spawn("eth_driver", "BOOT_FS", 254, Some(&["eth0"]))
        .expect("Failed to start eth_driver");

    /* Start the serial driver */
    rs_conn
        .process_spawn("serial_driver", "BOOT_FS", 254, Some(&["serial0"]))
        .expect("Failed to start serial_driver");

    /* eth components */
    /* Start the eth virt rx */
    rs_conn
        .process_spawn("eth_virt_rx", "BOOT_FS", 253, Some(&["rx_eth0", "eth0"]))
        .expect("Failed to start eth_virt_rx");

    /* Start the eth virt tx */
    rs_conn
        .process_spawn("eth_virt_tx", "BOOT_FS", 253, Some(&["tx_eth0", "eth0"]))
        .expect("Failed to start eth_virt_tx");

    /* Start the copier */
    rs_conn
        .process_spawn(
            "eth_copier",
            "BOOT_FS",
            252,
            Some(&["cli0_copy_eth0", "rx_eth0"]),
        )
        .expect("Failed to start eth_copier");

    /* Serial components */
    rs_conn
        .process_spawn(
            "serial_virt_rx",
            "BOOT_FS",
            253,
            Some(&["rx_serial0", "serial0"]),
        )
        .expect("Failed to start eth_virt_rx");

    rs_conn
        .process_spawn(
            "serial_virt_tx",
            "BOOT_FS",
            253,
            Some(&["tx_serial0", "serial0"]),
        )
        .expect("Failed to start eth_virt_rx");

    /* Start the timer driver */
    rs_conn
        .process_spawn("timer", "BOOT_FS", 254, Some(&["timer0"]))
        .expect("Failed to star timer_driver");

    /* Start the client */
    rs_conn
        .process_spawn(
            "echo_server",
            "BOOT_FS",
            251,
            Some(&[
                "cli0_copy_eth0",
                "tx_eth0",
                "rx_serial0",
                "tx_serial0",
                "timer0",
            ]),
        )
        .expect("Failed to start echo_server");

    loop {}

    // rs_conn.process_spawn("eth_virt_tx", "BOOT_FS", 253, Some(&["tx_eth0", "eth0"]));

    // /* Start a user application */
    // rs_conn.process_spawn("test_app", "BOOT_FS", 252, None);

    // let root_server_ep = sel4::CPtr::from_bits(1).cast::<sel4::cap_type::Endpoint>();

    // sel4::with_ipc_buffer_mut(|ipc_buf| {
    // ipc_buf.msg_regs_mut()[0] = 0xbeefcafe
    // });
    // sel4::debug_println!("after accessing ipc buffer");
    // let msg_info = sel4::MessageInfoBuilder::default().label(1).length(1).build();
    // root_server_ep.call(msg_info);

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

    // rs_conn.process_exit();

    // loop {}
    // unreachable!()
}
