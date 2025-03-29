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
use smos_common::connection::{sDDFConnection, ObjectServerConnection, RootServerConnection};
use smos_common::syscall::RootServerInterface;
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_sddf::blk_config::{BlkClientConfig, SDDF_BLK_MAGIC};
use smos_sddf::config::RegionResource;
use smos_sddf::fs_config::{FsConnectionResource, FsServerConfig, LIONS_FS_MAGIC};
use smos_sddf::queue::{Queue, QueuePair};
//TODO MAKE IMPORTS NICER

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;

//TODO USING SERIAL QUEUES RN FOR FS AND ETH QUEUES FOR BLK?? dont know if right
const SERIAL_TX_QUEUE: usize = 0x3_000_000;
const SERIAL_RX_QUEUE: usize = 0x3_002_000;
const SERIAL_QUEUE_SIZE: usize = 0x1000;

extern "C" {
    static mut fs_config: FsServerConfig;
    static mut blk_config: BlkClientConfig;
}

fn init_serial(
    rs_conn: &RootServerConnection,
    cspace: &mut SMOSUserCSpace,
    listen_conn: &sDDFConnection,
    rx_name: &str,
    tx_name: &str,
) -> (
    Queue<SerialQueue>,
    Queue<SerialQueue>,
    DMARegion,
    DMARegion,
    NotificationChannel<BidirectionalChannel, PPCForbidden>,
    NotificationChannel<BidirectionalChannel, PPCForbidden>,
) {
    /* Set up the serial rx queue */
    let serial_rx_queue = Queue::new(rs_conn, cspace, SERIAL_RX_QUEUE, SERIAL_QUEUE_SIZE)
        .expect("Failed to allocate rx queue");

    /* Set up the serial tx queue */
    let serial_tx_queue = Queue::new(rs_conn, cspace, SERIAL_TX_QUEUE, SERIAL_QUEUE_SIZE)
        .expect("Failed to allocate tx queue");

    /* Set up serial data regions */
    let serial_rx_data_region =
        DMARegion::new(rs_conn, cspace, SERIAL_RX_DATA, SERIAL_DATA_SIZE, true)
            .expect("Failed to create rx dma region");
    let serial_tx_data_region =
        DMARegion::new(rs_conn, cspace, SERIAL_TX_DATA, SERIAL_DATA_SIZE, true)
            .expect("Failed to create tx dma region");

    /* Set up a connection to the serial rx virt */

    let serial_rx_conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut serial_rx_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(serial_rx_conn_ep_slot), rx_name)
        .expect("Failed to establish connection to rx virt");

    serial_rx_conn
        .conn_open(None)
        .expect("Failed to open connection with driver");

    let serial_rx_channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::new(
        rs_conn,
        &serial_rx_conn,
        cspace,
        &listen_conn.hndl(),
        None,
    )
    .expect("Failed to connect to copier");

    serial_rx_conn
        .sddf_queue_register(
            serial_rx_queue.obj_hndl_cap.unwrap(),
            SERIAL_QUEUE_SIZE,
            QueueType::None,
        )
        .expect("Failed to register active queue");

    serial_rx_conn
        .sddf_data_region_provide(serial_rx_data_region.obj_hndl, SERIAL_DATA_SIZE)
        .expect("Failed to provide rx data region to copier");

    /* Set up a connection to the serial tx virt */

    let serial_tx_conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut serial_tx_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(serial_tx_conn_ep_slot), tx_name)
        .expect("Failed to establish connection to tx virt");

    serial_tx_conn
        .conn_open(None)
        .expect("Failed to open connection with tx");

    let serial_tx_channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::new(
        rs_conn,
        &serial_tx_conn,
        cspace,
        &listen_conn.hndl(),
        None,
    )
    .expect("Failed to open tx channel");

    serial_tx_conn
        .sddf_queue_register(
            serial_tx_queue.obj_hndl_cap.unwrap(),
            SERIAL_QUEUE_SIZE,
            QueueType::None,
        )
        .expect("Failed to register active queue");
    serial_tx_conn
        .sddf_data_region_provide(serial_tx_data_region.obj_hndl, SERIAL_DATA_SIZE)
        .expect("Failed to provide tx data region to tx virt");

    return (
        serial_rx_queue,
        serial_tx_queue,
        serial_rx_data_region,
        serial_tx_data_region,
        serial_rx_channel,
        serial_tx_channel,
    );
}

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) {
    sel4::debug_println!("Hello, I am the file system");

    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 5); // TODO CONFIRM IF 5 IS RIGHT

    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot for ep");
    let listen_conn = rs_conn
        .conn_publish::<sDDFConnection>(
            NTFN_BUFFER,
            &cspace.to_absolute_cptr(ep_cptr),
            "file_system",
        )
        .expect("Could not publish as a server");

    let (
        serial_rx_queue,
        serial_tx_queue,
        serial_rx_data,
        serial_tx_data,
        serial_rx_channel,
        serial_tx_channel,
    ) = init_serial(&rs_conn, &mut cspace, &listen_conn, args[2], args[3]);

    sddf_set_channel(
        serial_tx_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(serial_tx_channel),
    )
    .expect("Failed to set up channel with serial rx Virt");
    sddf_set_channel(
        serial_rx_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(serial_rx_channel),
    )
    .expect("Failed to set up channel with serial rx Virt");

    unsafe {
        fs_config = FsServerConfig {
            magic: LIONS_FS_MAGIC,
            client: FsConnectionResource {
                command_queue: RegionResource {
                    vaddr: serial_rx_queue.vaddr,
                    size: serial_rx_queue.size,
                },
                completion_queue: RegionResource {
                    vaddr: serial_tx_queue.vaddr,
                    size: serial_tx_queue.size,
                },
                share: RegionResource {
                    //TODO vaddr and size of tx or rx serial data region??
                },
                queue_len: //TODO
                id: 
            }
        };
        blk_config = BlkClientConfig {
            magic: SDDF_BLK_MAGIC,

        }
    }
}
