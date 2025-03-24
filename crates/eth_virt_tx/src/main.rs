#![no_std]
#![no_main]

use core::mem::MaybeUninit;

use smos_common::client_connection::ClientConnection;
use smos_common::connection::{sDDFConnection, RootServerConnection};
use smos_common::error::InvocationError;
use smos_common::local_handle::{
    ConnRegistrationHandle, ConnectionHandle, HandleOrHandleCap, LocalHandle, ObjectHandle,
};
use smos_common::sddf::{QueueType, VirtType};
use smos_common::server_connection::ServerConnection;
use smos_common::syscall::{
    sDDFInterface, NonRootServerInterface, ReplyWrapper, RootServerInterface,
};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_sddf::config::RegionResource;
use smos_sddf::device_config::DeviceRegionResource;
use smos_sddf::dma_region::DMARegion;
use smos_sddf::driver_setup::sDDFClient;
use smos_sddf::driver_setup::sddf_driver_pre_init;
use smos_sddf::driver_setup::VirtRegistration;
use smos_sddf::net_config::{
    NetConnectionResource, NetVirtTxClientConfig, NetVirtTxConfig, SDDF_NET_MAGIC,
    SDDF_NET_MAX_CLIENTS,
};
use smos_sddf::notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden};
use smos_sddf::queue::{ActiveQueue, FreeQueue, Queue, QueuePair};
use smos_sddf::sddf_bindings::{init, sddf_event_loop, sddf_set_channel};
use smos_sddf::sddf_channel::sDDFChannel;
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::event::{smos_serv_cleanup, smos_serv_decode_invocation, smos_serv_replyrecv};
use smos_server::reply::SMOSReply;
use smos_server::syscalls::{
    sDDFChannelRegisterBidirectional, sDDFProvideDataRegion, sDDFQueueRegister, ConnOpen,
    SMOS_Invocation,
};

extern crate alloc;
use alloc::vec::Vec;

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;

const DRV_FREE: usize = 0x3_000_000;
const DRV_ACTIVE: usize = 0x3_200_000;

const CLI0_ACTIVE: usize = 0x3_400_000;
const CLI0_FREE: usize = 0x3_600_000;
const CLI0_TX_DMA_REGION: usize = 0x3_800_000;

const DRV_QUEUE_SIZE: usize = 0x200_000;
const DRV_QUEUE_CAPACITY: usize = 512;
const CLI_QUEUE_SIZE: usize = 0x200_000;
const CLI_QUEUE_CAPACITY: usize = 512;
// const rcv_dma_region_size: usize = 0x2_200_000;

extern "C" {
    static mut config: NetVirtTxConfig;
}

#[derive(Debug, Copy, Clone)]
struct VirtTxClient {
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    id: usize,
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    active: Option<Queue<ActiveQueue>>,
    free: Option<Queue<FreeQueue>>,
    data: Option<DMARegion>,
    channel: Option<NotificationChannel<BidirectionalChannel, PPCForbidden>>,
}

impl sDDFClient for VirtTxClient {
    fn new(id: usize, conn_handle: LocalHandle<ConnRegistrationHandle>) -> Self {
        return VirtTxClient {
            id: id,
            conn_registration_hndl: conn_handle,
            active: None,
            free: None,
            data: None,
            channel: None,
        };
    }

    fn get_id(&self) -> usize {
        return self.id;
    }

    fn initialized(&self) -> bool {
        return self.channel.is_some()
            && self.free.is_some()
            && self.active.is_some()
            && self.data.is_some();
    }
}

fn handle_client_register(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    client: &mut VirtTxClient,
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
    client: &mut VirtTxClient,
    args: &sDDFQueueRegister,
) -> Result<SMOSReply, InvocationError> {
    /* We expect them to register a channel first */
    if client.channel.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    if args.size != CLI_QUEUE_SIZE {
        return Err(InvocationError::InvalidArguments);
    }

    match args.queue_type {
        QueueType::Active => {
            if client.active.is_some() {
                return Err(InvocationError::InvalidArguments);
            }

            client.active = Some(Queue::<ActiveQueue>::open(
                rs_conn,
                CLI0_ACTIVE,
                args.size,
                HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
            )?);
        }
        QueueType::Free => {
            if client.free.is_some() {
                return Err(InvocationError::InvalidArguments);
            }

            client.free = Some(Queue::<FreeQueue>::open(
                rs_conn,
                CLI0_FREE,
                args.size,
                HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
            )?);
        }
        _ => return Err(InvocationError::InvalidArguments),
    }

    return Ok(SMOSReply::sDDFQueueRegister);
}

fn handle_provide_data_region(
    rs_conn: &RootServerConnection,
    client: &mut VirtTxClient,
    args: &sDDFProvideDataRegion,
) -> Result<SMOSReply, InvocationError> {
    if client.active.is_none() || client.free.is_none() {
        return Err(InvocationError::InvalidArguments);
    }
    client.data = Some(DMARegion::open(
        rs_conn,
        HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
        CLI0_TX_DMA_REGION,
        0x200_000,
    )?);

    return Ok(SMOSReply::sDDFProvideDataRegion);
}

fn check_done(client: [Option<VirtTxClient>; 1]) -> bool {
    if client[0].is_none() {
        return false;
    }

    return client[0].unwrap().initialized();
}

// @alwin: This initial setup is pretty duplicated between the Tx and Rx virtualizers
#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) {
    sel4::debug_println!("Hello, I am eth_tx_virt!!!");

    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 2);

    /* Register as a server */
    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let listen_conn = rs_conn
        .conn_publish::<sDDFConnection>(NTFN_BUFFER, &cspace.to_absolute_cptr(ep_cptr), args[0])
        .expect("Could not publish as a server");

    /* Create the driver queue pair */
    let drv_queues = QueuePair::new(&rs_conn, &mut cspace, DRV_ACTIVE, DRV_FREE, DRV_QUEUE_SIZE)
        .expect("Failed to create driver queue pair");

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    /* Allocate a cap recieve slot */
    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

    let client = sddf_driver_pre_init(
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
    .expect("Failed to establish connection with client");

    /* Create connection to eth driver */
    let conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut drv_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(conn_ep_slot), args[1])
        .expect("Failed to establish connection to eth driver");

    drv_conn
        .conn_open(None)
        .expect("Failed to open connection with driver");

    /* Create a channel with the driver */
    let drv_channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::new(
        &rs_conn,
        &drv_conn,
        &mut cspace,
        &listen_conn.hndl(),
        Some(VirtType::Tx),
    )
    .expect("Failed to establish channel with driver");

    drv_conn
        .sddf_queue_register(
            drv_queues.active.obj_hndl_cap.unwrap(),
            drv_queues.active.size,
            QueueType::Active,
        )
        .expect("Failed to register active queue");
    drv_conn
        .sddf_queue_register(
            drv_queues.free.obj_hndl_cap.unwrap(),
            drv_queues.free.size,
            QueueType::Free,
        )
        .expect("Failed to register free queue");

    sddf_set_channel(
        drv_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(drv_channel),
    )
    .expect("Failed to set driver channel");
    sddf_set_channel(
        client.channel.unwrap().from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(client.channel.unwrap()),
    )
    .expect("Failed to set client channel");

    unsafe {
        let mut clients: [MaybeUninit<NetVirtTxClientConfig>; SDDF_NET_MAX_CLIENTS] =
            MaybeUninit::uninit().assume_init();

        clients[0] = MaybeUninit::new(NetVirtTxClientConfig {
            conn: NetConnectionResource {
                free_queue: RegionResource {
                    vaddr: client.free.unwrap().vaddr,
                    size: client.free.unwrap().size,
                },
                active_queue: RegionResource {
                    vaddr: client.active.unwrap().vaddr,
                    size: client.active.unwrap().size,
                },
                num_buffers: 512,
                id: client.channel.unwrap().from_bit.unwrap(),
            },
            data: DeviceRegionResource {
                region: RegionResource {
                    vaddr: client.data.as_ref().unwrap().vaddr,
                    size: client.data.as_ref().unwrap().size,
                },
                io_addr: client.data.as_ref().unwrap().paddr,
            },
        });

        config = NetVirtTxConfig {
            magic: SDDF_NET_MAGIC,
            driver: NetConnectionResource {
                free_queue: RegionResource {
                    vaddr: drv_queues.free.vaddr,
                    size: drv_queues.free.size,
                },
                active_queue: RegionResource {
                    vaddr: drv_queues.active.vaddr,
                    size: drv_queues.active.size,
                },
                num_buffers: 512,
                id: drv_channel.from_bit.unwrap(),
            },
            clients: clients,
            num_clients: 1,
        };
    }

    unsafe { init() };

    sddf_event_loop(listen_conn, reply);
}
