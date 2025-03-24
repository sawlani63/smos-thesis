#![no_std]
#![no_main]

use alloc::ffi::CString;
use core::ffi::c_char;
use core::ffi::CStr;
use smos_common::obj_attributes::ObjAttributes;
use smos_common::syscall::ObjectServerInterface;
use smos_common::util::ROUND_UP;
use smos_sddf::net_config::LibSddfLwipConfig;
use smos_sddf::net_config::SDDF_LIB_SDDF_LWIP_MAGIC;

use smos_common::{
    channel::Channel,
    client_connection::ClientConnection,
    connection::{sDDFConnection, RootServerConnection},
    sddf::QueueType,
    syscall::{sDDFInterface, NonRootServerInterface, RootServerInterface},
};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::{args::args, smos_declare_main};
use smos_sddf::{
    config::RegionResource,
    dma_region::DMARegion,
    net_config::{NetClientConfig, NetConnectionResource, SDDF_NET_MAGIC},
    notification_channel::{
        BidirectionalChannel, NotificationChannel, PPCAllowed, PPCForbidden, RecieveOnlyChannel,
    },
    queue::{Queue, QueuePair, SerialQueue},
    sddf_bindings::{init, sddf_event_loop, sddf_set_channel},
    sddf_channel::sDDFChannel,
    serial_config::{SerialClientConfig, SerialConnectionResource, SDDF_SERIAL_MAGIC},
    timer_config::{TimerClientConfig, SDDF_TIMER_MAGIC},
};

extern crate alloc;
use alloc::vec::Vec;

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;

//@alwin: These should probably all be runtime allocated
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

const SERIAL_TX_QUEUE: usize = 0x3_000_000;
const SERIAL_RX_QUEUE: usize = 0x3_002_000;
const SERIAL_QUEUE_SIZE: usize = 0x1000;

const SERIAL_TX_DATA: usize = 0x3_004_000;
const SERIAL_RX_DATA: usize = 0x3_006_000;
const SERIAL_DATA_SIZE: usize = 0x2000;

const PBUF_POOL_ADDR: usize = 0x4_000_000;
const PBUF_POOL_SIZE: usize = ROUND_UP(
    PBUF_POOL_ADDR * RX_QUEUE_CAPACITY as usize * 2,
    sel4_sys::seL4_PageBits as usize,
);
const PBUF_STRUCT_SIZE: usize = 56;

extern "C" {
    static mut net_config: NetClientConfig;
    static mut timer_config: TimerClientConfig;
    static mut serial_config: SerialClientConfig;
    static mut lib_sddf_lwip_config: LibSddfLwipConfig;
}

#[no_mangle]
pub static pd_name: &CStr = unsafe { CStr::from_bytes_with_nul_unchecked(b"client0\0") };

#[no_mangle]
pub unsafe extern "C" fn sddf_get_pd_name() -> *const c_char {
    return pd_name.as_ptr();
}

fn init_net(
    rs_conn: &RootServerConnection,
    cspace: &mut SMOSUserCSpace,
    listen_conn: &sDDFConnection,
    rx_name: &str,
    tx_name: &str,
) -> (
    QueuePair,
    QueuePair,
    DMARegion,
    DMARegion,
    NotificationChannel<BidirectionalChannel, PPCForbidden>,
    NotificationChannel<BidirectionalChannel, PPCForbidden>,
) {
    /* Create the eth Rx queues */
    let rx_queues = QueuePair::new(rs_conn, cspace, RX_ACTIVE, RX_FREE, RX_QUEUE_SIZE)
        .expect("Failed to create rx queue pair");

    /* Create the eth Tx queues */
    let tx_queues = QueuePair::new(rs_conn, cspace, TX_ACTIVE, TX_FREE, TX_QUEUE_SIZE)
        .expect("Failed to create tx queue pair");

    /* Create the eth data regions */
    let rx_data_region = DMARegion::new(rs_conn, cspace, RX_DATA, RX_DATA_SIZE, true)
        .expect("Failed to create rx dma region");
    let tx_data_region = DMARegion::new(rs_conn, cspace, TX_DATA, TX_DATA_SIZE, true)
        .expect("Failed to create tx dma region");

    /* Create connection/channels with rx copier */
    let rx_conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut rx_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(rx_conn_ep_slot), rx_name)
        .expect("Failed to establish connection to rx virt");

    rx_conn
        .conn_open(None)
        .expect("Failed to open connection with driver");

    let rx_channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::new(
        rs_conn,
        &rx_conn,
        cspace,
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
        .sddf_data_region_provide(rx_data_region.obj_hndl, RX_DATA_SIZE)
        .expect("Failed to provide rx data region to copier");

    // /* Create connection with tx virt */
    let tx_conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut tx_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(tx_conn_ep_slot), tx_name)
        .expect("Failed to establish connection to tx virt");

    tx_conn
        .conn_open(None)
        .expect("Failed to open connection with tx");

    let tx_channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::new(
        rs_conn,
        &tx_conn,
        cspace,
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
        .sddf_data_region_provide(tx_data_region.obj_hndl, TX_DATA_SIZE)
        .expect("Failed to provide tx data region to tx virt");

    return (
        rx_queues,
        tx_queues,
        rx_data_region,
        tx_data_region,
        rx_channel,
        tx_channel,
    );
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
    sel4::debug_println!("Hello, I am the echo server");

    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 5);

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

    /* Set up the connection to the net subsystem */
    let (net_rx_queues, net_tx_queues, net_rx_data, net_tx_data, net_rx_channel, net_tx_channel) =
        init_net(&rs_conn, &mut cspace, &listen_conn, args[0], args[1]);

    /* Set up the connection to the serial subsystem */
    let (
        serial_rx_queue,
        serial_tx_queue,
        serial_rx_data,
        serial_tx_data,
        serial_rx_channel,
        serial_tx_channel,
    ) = init_serial(&rs_conn, &mut cspace, &listen_conn, args[2], args[3]);

    /* Set up a connection to the timer */
    let timer_conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut timer_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(timer_conn_ep_slot), args[4])
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

    /* Setup all the channels */

    sddf_set_channel(
        timer_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelRecvPPC(timer_channel),
    )
    .expect("Failed to set up channel with timer");
    sddf_set_channel(
        net_rx_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(net_rx_channel),
    )
    .expect("Failed to set up channel with Rx Virt");
    sddf_set_channel(
        net_tx_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(net_tx_channel),
    )
    .expect("Failed to set up channel with Tx Virt");
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

    let pbuf_pool_win = rs_conn
        .window_create(PBUF_POOL_ADDR, PBUF_POOL_SIZE, None)
        .expect("Failed to create window for pbufs");

    let pbuf_pool_obj = rs_conn
        .obj_create(
            None,
            PBUF_POOL_SIZE,
            sel4::CapRights::all(),
            ObjAttributes::DEFAULT,
            None,
        )
        .expect("Failed to create obj for pbufs");

    let view_hndl = rs_conn
        .view(
            &pbuf_pool_win,
            &pbuf_pool_obj,
            0,
            0,
            PBUF_POOL_SIZE,
            sel4::CapRights::all(),
        )
        .expect("Failed to create view for pbufs");

    /* Start up the client */
    unsafe {
        net_config = NetClientConfig {
            magic: SDDF_NET_MAGIC,
            rx: NetConnectionResource {
                free_queue: RegionResource {
                    vaddr: net_rx_queues.free.vaddr,
                    size: net_rx_queues.free.size,
                },
                active_queue: RegionResource {
                    vaddr: net_rx_queues.active.vaddr,
                    size: net_rx_queues.free.size,
                },
                num_buffers: 512,
                id: net_rx_channel.from_bit.unwrap(),
            },
            rx_data: RegionResource {
                vaddr: net_rx_data.vaddr,
                size: net_rx_data.size,
            },
            tx: NetConnectionResource {
                free_queue: RegionResource {
                    vaddr: net_tx_queues.free.vaddr,
                    size: net_tx_queues.free.size,
                },
                active_queue: RegionResource {
                    vaddr: net_tx_queues.active.vaddr,
                    size: net_tx_queues.free.vaddr,
                },
                num_buffers: 512,
                id: net_tx_channel.from_bit.unwrap(),
            },
            tx_data: RegionResource {
                vaddr: net_tx_data.vaddr,
                size: net_tx_data.vaddr,
            },
            mac_addr: [0x07, 0x00, 0x00, 0x01, 0x54, 0x52],
        };

        serial_config = SerialClientConfig {
            magic: SDDF_SERIAL_MAGIC,
            rx: SerialConnectionResource {
                queue: RegionResource {
                    vaddr: serial_rx_queue.vaddr,
                    size: serial_rx_queue.size,
                },
                data: RegionResource {
                    vaddr: serial_rx_data.vaddr,
                    size: serial_rx_data.size,
                },
                id: serial_rx_channel.from_bit.unwrap(),
            },
            tx: SerialConnectionResource {
                queue: RegionResource {
                    vaddr: serial_tx_queue.vaddr,
                    size: serial_tx_queue.size,
                },
                data: RegionResource {
                    vaddr: serial_tx_data.vaddr,
                    size: serial_tx_data.size,
                },
                id: serial_tx_channel.from_bit.unwrap(),
            },
        };

        timer_config = TimerClientConfig {
            magic: SDDF_TIMER_MAGIC,
            driver_id: timer_channel.from_bit.unwrap(),
        };

        lib_sddf_lwip_config = LibSddfLwipConfig {
            magic: SDDF_LIB_SDDF_LWIP_MAGIC,
            pbuf_pool: RegionResource {
                vaddr: PBUF_POOL_ADDR,
                size: PBUF_POOL_SIZE,
            },
            num_pbufs: RX_QUEUE_CAPACITY as u64 * 2,
        }
    }

    unsafe { init() };

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    sddf_event_loop(listen_conn, reply);
}
