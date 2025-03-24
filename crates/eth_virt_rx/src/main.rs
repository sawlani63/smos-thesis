#![no_std]
#![no_main]

use core::mem::MaybeUninit;

use smos_common::{
    client_connection::ClientConnection,
    connection::{sDDFConnection, RootServerConnection},
    error::InvocationError,
    local_handle::{
        ConnRegistrationHandle, ConnectionHandle, HandleOrHandleCap, LocalHandle, ObjectHandle,
    },
    obj_attributes::ObjAttributes,
    sddf::{QueueType, VirtType},
    server_connection::ServerConnection,
    syscall::{
        sDDFInterface, NonRootServerInterface, ObjectServerInterface, ReplyWrapper,
        RootServerInterface,
    },
    util::ROUND_UP,
};

use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;

use smos_sddf::{
    config::RegionResource,
    device_config::DeviceRegionResource,
    dma_region::DMARegion,
    driver_setup::{sDDFClient, sddf_driver_pre_init, VirtRegistration},
    net_config::{
        NetConnectionResource, NetVirtRxClientConfig, NetVirtRxConfig, SDDF_NET_MAGIC,
        SDDF_NET_MAX_CLIENTS,
    },
    notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden},
    queue::{ActiveQueue, FreeQueue, Queue, QueuePair},
    sddf_bindings::{init, sddf_event_loop, sddf_set_channel},
    sddf_channel::sDDFChannel,
};

use smos_server::{
    event::{
        decode_entry_type, smos_serv_cleanup, smos_serv_decode_invocation, smos_serv_replyrecv,
        EntryType,
    },
    reply::SMOSReply,
    syscalls::{sDDFChannelRegisterBidirectional, sDDFQueueRegister, ConnOpen, SMOS_Invocation},
};
extern crate alloc;
use alloc::vec::Vec;

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;

const CPY_FREE: usize = 0x2_000_000;
const CPY_ACTIVE: usize = 0x2_200_000;
const DRV_FREE: usize = 0x3_000_000;
const DRV_ACTIVE: usize = 0x3_200_000;
const BUFFER_METADATA: usize = 0x3_400_000;
const RCV_DMA: usize = 0x3_600_000;

const DRV_QUEUE_SIZE: usize = 0x200_000;
const DRV_QUEUE_CAPACITY: usize = 512;
const CPY_QUEUE_SIZE: usize = 0x200_000;
const CPY_QUEUE_CAPACITY: usize = 512;
const RCV_DMA_REGION_SIZE: usize = 0x2_200_000;

extern "C" {
    static mut config: NetVirtRxConfig;
}

#[derive(Debug, Copy, Clone)]
struct Copier {
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    id: usize,
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    active: Option<Queue<ActiveQueue>>,
    free: Option<Queue<FreeQueue>>,
    channel: Option<NotificationChannel<BidirectionalChannel, PPCForbidden>>,
    initialized: bool,
}

impl sDDFClient for Copier {
    fn new(id: usize, conn_handle: LocalHandle<ConnRegistrationHandle>) -> Self {
        return Copier {
            id: id,
            conn_registration_hndl: conn_handle,
            active: None,
            free: None,
            channel: None,
            initialized: false,
        };
    }

    fn get_id(&self) -> usize {
        return self.id;
    }

    fn initialized(&self) -> bool {
        return self.initialized;
    }
}

fn handle_copier_register(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    cpy: &mut Copier,
    _status: &mut VirtRegistration,
    args: &sDDFChannelRegisterBidirectional,
) -> Result<SMOSReply, InvocationError> {
    // We expect a copier, not a virtualizer
    if args.virt_type.is_some() {
        return Err(InvocationError::InvalidArguments);
    }

    let channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::open(
        rs_conn,
        cspace,
        publish_hndl,
        args.channel_hndl_cap.into(),
    )?;

    cpy.channel = Some(channel);

    return Ok(SMOSReply::sDDFChannelRegisterBidirectional {
        hndl_cap: cpy.channel.unwrap().from_hndl_cap.unwrap(),
    });
}

fn handle_queue_register(
    rs_conn: &RootServerConnection,
    cpy: &mut Copier,
    args: &sDDFQueueRegister,
) -> Result<SMOSReply, InvocationError> {
    /* We expect them to register a channel first */
    if cpy.channel.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    if args.size != CPY_QUEUE_SIZE {
        return Err(InvocationError::InvalidArguments);
    }

    match args.queue_type {
        QueueType::Active => {
            if cpy.active.is_some() {
                return Err(InvocationError::InvalidArguments);
            }

            cpy.active = Some(Queue::<ActiveQueue>::open(
                rs_conn,
                CPY_ACTIVE,
                args.size,
                HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
            )?);
        }
        QueueType::Free => {
            if cpy.free.is_some() {
                return Err(InvocationError::InvalidArguments);
            }

            cpy.free = Some(Queue::<FreeQueue>::open(
                rs_conn,
                CPY_FREE,
                args.size,
                HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
            )?);
        }
        _ => return Err(InvocationError::InvalidArguments),
    }

    return Ok(SMOSReply::sDDFQueueRegister);
}

fn handle_get_data_region(
    cpy: &mut Copier,
    rcv_dma_region: &DMARegion,
) -> Result<SMOSReply, InvocationError> {
    if cpy.active.is_none() || cpy.free.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    if let HandleOrHandleCap::HandleCap(hndl_cap) = rcv_dma_region.obj_hndl {
        cpy.initialized = true;
        return Ok(SMOSReply::sDDFGetDataRegion { hndl_cap: hndl_cap });
    } else {
        panic!("Expected this to be a handle cap");
    }
}

//@alwin: Can this be moved inside the common sddf crate
fn check_done(copier: [Option<Copier>; 1]) -> bool {
    if copier[0].is_none() {
        return false;
    }

    return copier[0].unwrap().initialized();
}

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) {
    sel4::debug_println!("Hello, I am eth_rx_virt!!!");

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

    /* Create the recieve DMA region */
    let rcv_dma_region = DMARegion::new(
        &rs_conn,
        &mut cspace,
        RCV_DMA as usize,
        RCV_DMA_REGION_SIZE,
        true,
    )
    .expect("Failed to create DMA region");

    /* Create the buffer metadata region */
    let buffer_metadata_size = ROUND_UP(
        size_of::<u32>() * 512,
        sel4_sys::seL4_PageBits.try_into().unwrap(),
    );
    let buffer_metadata_win_hndl = rs_conn
        .window_create(BUFFER_METADATA, buffer_metadata_size, None)
        .expect("Failed to create buffer metadata window");

    let buffer_metadata_obj_hndl = rs_conn
        .obj_create(
            None,
            buffer_metadata_size,
            sel4::CapRights::all(),
            ObjAttributes::DEFAULT,
            None,
        )
        .expect("Failed to create buffer metadata view");

    let buffer_metadata_view_hndl = rs_conn
        .view(
            &buffer_metadata_win_hndl,
            &buffer_metadata_obj_hndl,
            0,
            0,
            buffer_metadata_size,
            sel4::CapRights::all(),
        )
        .expect("Failed to create buffer metadata view");

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    /* Allocate a cap recieve slot */
    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

    let copier = sddf_driver_pre_init::<sDDFConnection, Copier, 1>(
        &rs_conn,
        &mut cspace,
        &listen_conn,
        &reply,
        recv_slot,
        Some(handle_copier_register),
        Some(handle_queue_register),
        None,
        Some((handle_get_data_region, &rcv_dma_region)),
        check_done,
    )[0]
    .expect("Failed to establish connection to a copier");

    /* Create connection to eth driver */
    let conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut drv_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(conn_ep_slot), args[1])
        .expect("Failed to establish connection to eth driver");

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
    .expect("Failed to set up channel to driver");
    sddf_set_channel(
        copier.channel.unwrap().from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(copier.channel.unwrap()),
    )
    .expect("Failed to set up channel to copier");

    /* Start the virtualizer */
    unsafe {
        let mut clients: [MaybeUninit<NetVirtRxClientConfig>; SDDF_NET_MAX_CLIENTS] =
            MaybeUninit::uninit().assume_init();

        clients[0] = MaybeUninit::new(NetVirtRxClientConfig {
            conn: NetConnectionResource {
                free_queue: RegionResource {
                    vaddr: copier.free.unwrap().vaddr,
                    size: copier.free.unwrap().size,
                },
                active_queue: RegionResource {
                    vaddr: copier.active.unwrap().vaddr,
                    size: copier.active.unwrap().size,
                },
                num_buffers: 512,
                id: copier.channel.unwrap().from_bit.unwrap(),
            },
            mac_addr: [0x07, 0x00, 0x00, 0x01, 0x54, 0x52],
        });

        config = NetVirtRxConfig {
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
            data: DeviceRegionResource {
                region: RegionResource {
                    vaddr: RCV_DMA,
                    size: RCV_DMA_REGION_SIZE,
                },
                io_addr: rcv_dma_region.paddr,
            },
            buffer_metadata: RegionResource {
                vaddr: BUFFER_METADATA,
                size: buffer_metadata_size,
            },
            clients: clients,
            num_clients: 1,
        };
    }

    unsafe { init() };

    sddf_event_loop(listen_conn, reply);
}
