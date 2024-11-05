#![no_std]
#![no_main]

use smos_common::client_connection::ClientConnection;
use smos_common::connection::{sDDFConnection, RootServerConnection};
use smos_common::sddf::QueueType;
use smos_common::syscall::{sDDFInterface, NonRootServerInterface, RootServerInterface};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_sddf::dma_region::DMARegion;
use smos_sddf::notification_channel::{
    BidirectionalChannel, NotificationChannel, PPCAllowed, PPCForbidden, RecieveOnlyChannel,
};
use smos_sddf::queue::QueuePair;
use smos_sddf::sddf_bindings::{sddf_event_loop, sddf_init, sddf_set_channel};
use smos_sddf::sddf_channel::sDDFChannel;
extern crate alloc;
use alloc::vec::Vec;

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;

const RX_FREE: usize = 0x2_000_000;
const RX_ACTIVE: usize = 0x2_200_000;
const TX_FREE: usize = 0x2_400_000;
const TX_ACTIVE: usize = 0x2_600_000;

const RX_DATA: usize = 0x2_800_000;
const TX_DATA: usize = 0x2_a00_000;

const RX_QUEUE_SIZE: usize = 0x200_000;
const TX_QUEUE_SIZE: usize = 0x200_000;
const RX_DATA_SIZE: usize = 0x200_000;
const TX_DATA_SIZE: usize = 0x200_000;

const RX_QUEUE_CAPACITY: usize = 512;
const TX_QUEUE_CAPACITY: usize = 512;

const MAC_ADDR: usize = 0x525401000007;

#[repr(C)]
struct Resources {
    rx_free: u64,
    rx_active: u64,
    rx_queue_capacity: u64,
    tx_free: u64,
    tx_active: u64,
    tx_queue_capacity: u64,
    rx_data_region: u64,
    tx_data_region: u64,
    mac_addr: u64,

    timer_id: u8,
    rx_id: u8,
    tx_id: u8,
}

extern "C" {
    static mut resources: Resources;
}

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) {
    sel4::debug_println!("Hello, I am the echo server");

    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 3);

    /* Register as a server */
    // @alwin: This actually shouldn't be a server
    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let listen_conn = rs_conn
        .conn_publish::<sDDFConnection>(
            NTFN_BUFFER,
            &cspace.to_absolute_cptr(ep_cptr),
            "echo_server",
        )
        .expect("Could not publish as a server");

    /* Create the Rx queues */
    let rx_queues = QueuePair::new(&rs_conn, &mut cspace, RX_ACTIVE, RX_FREE, RX_QUEUE_SIZE)
        .expect("Failed to create rx queue pair");

    /* Create the Tx queues */
    let tx_queues = QueuePair::new(&rs_conn, &mut cspace, TX_ACTIVE, TX_FREE, TX_QUEUE_SIZE)
        .expect("Failed to create tx queue pair");

    /* Create the data regions */
    let rx_data_region = DMARegion::new(&rs_conn, &mut cspace, RX_DATA, RX_DATA_SIZE, true)
        .expect("Failed to create rx dma region");
    let tx_data_region = DMARegion::new(&rs_conn, &mut cspace, TX_DATA, TX_DATA_SIZE, true)
        .expect("Failed to create tx dma region");

    /* Create connection/channels with rx copier */
    let rx_conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut rx_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(rx_conn_ep_slot), args[0])
        .expect("Failed to establish connection to rx virt");

    rx_conn
        .conn_open(None)
        .expect("Failed to open connection with driver");

    let rx_channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::new(
        &rs_conn,
        &rx_conn,
        &mut cspace,
        &listen_conn.hndl(),
        None,
    )
    .expect("Failed to connect to copier");

    rx_conn
        .sddf_queue_register(
            rx_queues.active.obj_hndl_cap.unwrap(),
            RX_QUEUE_SIZE,
            QueueType::Active,
        )
        .expect("Failed to register active queue");
    rx_conn
        .sddf_queue_register(
            rx_queues.free.obj_hndl_cap.unwrap(),
            RX_QUEUE_SIZE,
            QueueType::Free,
        )
        .expect("Failed to register free queue");

    rx_conn
        .sddf_data_region_provide(rx_data_region.obj_hndl)
        .expect("Failed to provide rx data region to copier");

    // /* Create connection with tx virt */
    let tx_conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut tx_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(tx_conn_ep_slot), args[1])
        .expect("Failed to establish connection to tx virt");

    tx_conn
        .conn_open(None)
        .expect("Failed to open connection with tx");

    let tx_channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::new(
        &rs_conn,
        &tx_conn,
        &mut cspace,
        &listen_conn.hndl(),
        None,
    )
    .expect("Failed to open tx channel");

    tx_conn
        .sddf_queue_register(
            tx_queues.active.obj_hndl_cap.unwrap(),
            TX_QUEUE_SIZE,
            QueueType::Active,
        )
        .expect("Failed to register active queue");
    tx_conn
        .sddf_queue_register(
            tx_queues.free.obj_hndl_cap.unwrap(),
            TX_QUEUE_SIZE,
            QueueType::Free,
        )
        .expect("Failed to register free queue");

    tx_conn
        .sddf_data_region_provide(tx_data_region.obj_hndl)
        .expect("Failed to provide tx data region to tx virt");

    /* Set up a connection to the timer */
    let timer_conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut timer_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(timer_conn_ep_slot), args[2])
        .expect("Failed to establish connection to timer");

    timer_conn
        .conn_open(None)
        .expect("Failed ot open connection with timer");

    let timer_channel = NotificationChannel::<RecieveOnlyChannel, PPCAllowed>::new(
        &rs_conn,
        &timer_conn,
        &mut cspace,
        &listen_conn.hndl(),
    )
    .expect("Failed to open timer channel");

    sddf_set_channel(
        timer_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelRecvPPC(timer_channel),
    )
    .expect("Failed to set up channel with timer");
    sddf_set_channel(
        rx_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(rx_channel),
    )
    .expect("Failed to set up channel with Rx Virt");
    sddf_set_channel(
        tx_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(tx_channel),
    )
    .expect("Failed to set up channel with Tx Virt");

    /* Start up the client */
    unsafe {
        resources = Resources {
            rx_free: RX_FREE as u64,
            rx_active: RX_ACTIVE as u64,
            rx_queue_capacity: RX_QUEUE_CAPACITY as u64,
            tx_free: TX_FREE as u64,
            tx_active: TX_ACTIVE as u64,
            tx_queue_capacity: TX_QUEUE_CAPACITY as u64,
            rx_data_region: RX_DATA as u64,
            tx_data_region: TX_DATA as u64,
            mac_addr: MAC_ADDR as u64,

            timer_id: timer_channel.from_bit.unwrap(),
            rx_id: rx_channel.from_bit.unwrap(),
            tx_id: tx_channel.from_bit.unwrap(),
        }
    }

    unsafe { sddf_init() };

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    sddf_event_loop(listen_conn, reply);
}
