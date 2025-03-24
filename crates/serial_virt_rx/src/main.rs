#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use smos_common::connection::sDDFConnection;
use smos_common::connection::RootServerConnection;
use smos_common::error::InvocationError;
use smos_common::local_handle::{ConnRegistrationHandle, LocalHandle};
use smos_common::sddf::QueueType;
use smos_common::sddf::VirtType;
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_sddf::driver_setup::sDDFClient;
use smos_sddf::driver_setup::sddf_driver_pre_init;
use smos_sddf::queue::Queue;
use smos_sddf::sddf_channel::sDDFChannel;
use smos_sddf::serial_config::SDDF_SERIAL_MAX_CLIENTS;
use smos_sddf::{
    config::RegionResource,
    dma_region::DMARegion,
    notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden},
    queue::SerialQueue,
    sddf_bindings::{sddf_event_loop, sddf_set_channel},
    serial_config::{SerialConnectionResource, SerialVirtRxConfig, SDDF_SERIAL_MAGIC},
};
extern crate alloc;
use alloc::vec::Vec;
use core::ffi::c_char;
use smos_common::client_connection::ClientConnection;
use smos_common::local_handle::ConnectionHandle;
use smos_common::local_handle::HandleOrHandleCap;
use smos_common::local_handle::ObjectHandle;
use smos_common::syscall::sDDFInterface;
use smos_common::syscall::NonRootServerInterface;
use smos_common::syscall::RootServerInterface;
use smos_sddf::driver_setup::VirtRegistration;
use smos_sddf::sddf_bindings::init;
use smos_server::reply::SMOSReply;
use smos_server::syscalls::sDDFChannelRegisterBidirectional;
use smos_server::syscalls::sDDFProvideDataRegion;
use smos_server::syscalls::sDDFQueueRegister;

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;

const DRV_QUEUE: usize = 0x3_000_000;
const DRV_DATA: usize = 0x3_002_000;
const DRV_DATA_SIZE: usize = 0x2000;

const CLI_QUEUE: usize = 0x4_000_000;
const CLI_DATA: usize = 0x4_002_000;
const CLIENT_QUEUE_SIZE: usize = 0x1000;

extern "C" {
    static mut config: SerialVirtRxConfig;
}

#[derive(Debug, Copy, Clone)]
struct SerialClient {
    id: usize,
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    queue: Option<Queue<SerialQueue>>,
    data: Option<DMARegion>,
    channel: Option<NotificationChannel<BidirectionalChannel, PPCForbidden>>,
}

impl sDDFClient for SerialClient {
    fn new(id: usize, conn_handle: LocalHandle<ConnRegistrationHandle>) -> Self {
        return SerialClient {
            id: id,
            conn_registration_hndl: conn_handle,
            queue: None,
            data: None,
            channel: None,
        };
    }

    fn get_id(&self) -> usize {
        return self.id;
    }

    fn initialized(&self) -> bool {
        return self.channel.is_some() && self.queue.is_some() && self.data.is_some();
    }
}

fn handle_client_register(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    client: &mut SerialClient,
    _status: &mut VirtRegistration,
    args: &sDDFChannelRegisterBidirectional,
) -> Result<SMOSReply, InvocationError> {
    // We expect a client, not a virtualizer
    if args.virt_type.is_some() {
        return Err(InvocationError::InvalidArguments);
    }

    let channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::open(
        rs_conn,
        cspace,
        publish_hndl,
        args.channel_hndl_cap.into(),
    )?;

    client.channel = Some(channel);

    return Ok(SMOSReply::sDDFChannelRegisterBidirectional {
        hndl_cap: client.channel.unwrap().from_hndl_cap.unwrap(),
    });
}

fn handle_queue_register(
    rs_conn: &RootServerConnection,
    client: &mut SerialClient,
    args: &sDDFQueueRegister,
) -> Result<SMOSReply, InvocationError> {
    if client.channel.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    // @alwin: Different cases for rx and tx
    if args.size != CLIENT_QUEUE_SIZE {
        return Err(InvocationError::InvalidArguments);
    }

    if client.queue.is_some() {
        return Err(InvocationError::InvalidArguments);
    }

    client.queue = Some(Queue::<SerialQueue>::open(
        rs_conn,
        CLI_QUEUE as usize,
        args.size,
        HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
    )?);

    return Ok(SMOSReply::sDDFQueueRegister);
}

fn handle_provide_data_region(
    rs_conn: &RootServerConnection,
    client: &mut SerialClient,
    args: &sDDFProvideDataRegion,
) -> Result<SMOSReply, InvocationError> {
    if client.queue.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    client.data = Some(DMARegion::open(
        rs_conn,
        HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
        CLI_DATA,
        args.size,
    )?);

    return Ok(SMOSReply::sDDFProvideDataRegion);
}

//@alwin: Can this be moved inside the common sddf crate
fn check_done(clients: [Option<SerialClient>; 1]) -> bool {
    if clients[0].is_none() {
        return false;
    }

    return clients[0].unwrap().initialized();
}

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) {
    sel4::debug_println!("Hello, I am serial_rx_virt!!!");

    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 2);

    /* Register as a server */
    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let listen_conn = rs_conn
        .conn_publish::<sDDFConnection>(NTFN_BUFFER, &cspace.to_absolute_cptr(ep_cptr), args[0])
        .expect("Could not publish as a server");

    /* Create the driver queue */
    let drv_queue = Queue::<SerialQueue>::new(&rs_conn, &mut cspace, DRV_QUEUE, 0x1000)
        .expect("Failed to create driver queue");

    /* Create the driver data region */
    let drv_data_region = DMARegion::new(&rs_conn, &mut cspace, DRV_DATA, DRV_DATA_SIZE, true)
        .expect("Failed to create data region");

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    /* Allocate a cap recieve slot */
    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

    let client = sddf_driver_pre_init::<sDDFConnection, SerialClient, 1>(
        &rs_conn,
        &mut cspace,
        &listen_conn,
        &reply,
        recv_slot,
        Some(handle_client_register),
        Some(handle_queue_register),
        Some(handle_provide_data_region),
        None,
        check_done,
    )[0]
    .expect("Failed to establish connection to a copier");

    /* Create connection to serial driver */
    let conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut drv_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(conn_ep_slot), args[1])
        .expect("Failed to establish connection to serial driver");

    drv_conn
        .conn_open(None)
        .expect("Failed to open connection with driver");
    let drv_channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::new(
        &rs_conn,
        &drv_conn,
        &mut cspace,
        &listen_conn.hndl(),
        Some(VirtType::Rx),
    )
    .expect("Failed to establish channel with driver");

    drv_conn
        .sddf_queue_register(
            drv_queue.obj_hndl_cap.unwrap(),
            drv_queue.size,
            QueueType::None,
        )
        .expect("Failed to register queue");

    drv_conn
        .sddf_data_region_provide(drv_data_region.obj_hndl, DRV_DATA_SIZE)
        .expect("Failed to provide data region to driver");

    sddf_set_channel(
        drv_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(drv_channel),
    )
    .expect("Failed to set up channel to driver");
    sddf_set_channel(
        client.channel.unwrap().from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(client.channel.unwrap()),
    )
    .expect("Failed to set up channel to client");

    unsafe {
        let mut clients: [MaybeUninit<SerialConnectionResource>; SDDF_SERIAL_MAX_CLIENTS] =
            MaybeUninit::uninit().assume_init();

        clients[0] = MaybeUninit::new(SerialConnectionResource {
            queue: RegionResource {
                vaddr: client.queue.unwrap().vaddr,
                size: client.queue.unwrap().size,
            },
            data: RegionResource {
                vaddr: client.data.unwrap().vaddr,
                size: client.data.unwrap().vaddr,
            },
            id: client.channel.unwrap().from_bit.unwrap(),
        });

        config = SerialVirtRxConfig {
            magic: SDDF_SERIAL_MAGIC,
            driver: SerialConnectionResource {
                queue: RegionResource {
                    vaddr: drv_queue.vaddr,
                    size: drv_queue.size,
                },
                data: RegionResource {
                    vaddr: drv_data_region.vaddr,
                    size: drv_data_region.size,
                },
                id: drv_channel.from_bit.unwrap(),
            },
            clients: clients,
            num_clients: 1,
            switch_char: 28 as c_char,
            terminate_num_char: '\r' as c_char,
        }
    }

    unsafe { init() }

    sddf_event_loop(listen_conn, reply);
}
