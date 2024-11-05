#![no_std]
#![no_main]

use smos_common::client_connection::ClientConnection;
use smos_common::connection::{sDDFConnection, RootServerConnection};
use smos_common::error::InvocationError;
use smos_common::local_handle::HandleOrHandleCap;
use smos_common::local_handle::ObjectHandle;
use smos_common::local_handle::{ConnRegistrationHandle, ConnectionHandle, LocalHandle};
use smos_common::sddf::{QueueType, VirtType};
use smos_common::server_connection::ServerConnection;
use smos_common::syscall::{
    sDDFInterface, NonRootServerInterface, ReplyWrapper, RootServerInterface,
};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_sddf::dma_region::DMARegion;
use smos_sddf::notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden};
use smos_sddf::queue::{ActiveQueue, FreeQueue, Queue, QueuePair};
use smos_sddf::sddf_bindings::{sddf_event_loop, sddf_init, sddf_set_channel};
use smos_sddf::sddf_channel::sDDFChannel;
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::event::{smos_serv_cleanup, smos_serv_decode_invocation, smos_serv_replyrecv};
use smos_server::reply::SMOSReply;
use smos_server::syscalls::{
    sDDFChannelRegisterBidirectional, sDDFQueueRegister, ConnOpen, SMOS_Invocation,
};

extern crate alloc;
use alloc::vec::Vec;

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;

const CPY_FREE: usize = 0x2_000_000;
const CPY_ACTIVE: usize = 0x2_200_000;
const DRV_FREE: usize = 0x3_000_000;
const DRV_ACTIVE: usize = 0x3_200_000;
const RCV_DMA: usize = 0x3_400_000;

const DRV_QUEUE_SIZE: usize = 0x200_000;
const DRV_QUEUE_CAPACITY: usize = 512;
const CPY_QUEUE_SIZE: usize = 0x200_000;
const CPY_QUEUE_CAPACITY: usize = 512;
const RCV_DMA_REGION_SIZE: usize = 0x2_200_000;

const CLI_MAC_ADDR: usize = 0x525401000007;

#[repr(C)]
struct Client {
    rx_free: u64,
    rx_active: u64,
    queue_size: u64,
    client_id: u8,
    mac_addr: u64,
}

#[repr(C)]
struct Resources {
    rx_free_drv: u64,
    rx_active_drv: u64,
    drv_queue_size: u64,
    buffer_data_vaddr: u64,
    buffer_data_paddr: u64,

    driver_id: u8,
    num_network_clients: u8,

    clients: [Client; 1],
}

extern "C" {
    static mut resources: Resources;
}

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

fn handle_conn_open(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    id: usize,
    args: &ConnOpen,
    slot: &mut Option<Copier>,
) -> Result<SMOSReply, InvocationError> {
    /* The virtualizer does not support a shared buffer */
    if args.shared_buf_obj.is_some() {
        return Err(InvocationError::InvalidArguments);
    }

    let registration_handle = rs_conn
        .conn_register(publish_hndl, id)
        .expect("@alwin: Can this be an assertion?");

    *slot = Some(Copier {
        id: id,
        conn_registration_hndl: registration_handle,
        free: None,
        active: None,
        channel: None,
        initialized: false,
    });

    return Ok(SMOSReply::ConnOpen);
}

fn handle_copier_register(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    cpy: &mut Copier,
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

fn pre_init<T: ServerConnection>(
    rs_conn: &RootServerConnection,
    cspace: &mut SMOSUserCSpace,
    listen_conn: &T,
    reply: &ReplyWrapper,
    recv_slot: sel4::AbsoluteCPtr,
    rcv_dma_region: &DMARegion,
) -> Copier {
    let mut cpy = None;
    let mut reply_msg_info = None;

    /* Specify the slot in which we should recieve caps */
    sel4::with_ipc_buffer_mut(|ipc_buf| {
        ipc_buf.set_recv_slot(&recv_slot);
    });

    loop {
        let (msg, badge) = smos_serv_replyrecv(listen_conn, reply, reply_msg_info);

        if let EntryType::Invocation(id) = decode_entry_type(badge.try_into().unwrap()) {
            let invocation = smos_serv_decode_invocation::<sDDFConnection>(&msg, recv_slot, None);
            if let Err(e) = invocation {
                reply_msg_info = e;
                continue;
            }

            let ret = if cpy.is_none() {
                match invocation.as_ref().unwrap() {
                    SMOS_Invocation::ConnOpen(t) => {
                        handle_conn_open(&rs_conn, listen_conn.hndl(), id, t, &mut cpy)
                    }
                    _ => todo!(), // @alwin: Client calls something before opening connection
                }
            } else {
                match invocation.as_ref().unwrap() {
                    SMOS_Invocation::ConnOpen(_) => todo!(), // @alwin: Client calls conn_open again
                    SMOS_Invocation::sDDFChannelRegisterBidirectional(t) => handle_copier_register(
                        rs_conn,
                        listen_conn.hndl(),
                        cspace,
                        &mut cpy.as_mut().unwrap(),
                        &t,
                    ),
                    SMOS_Invocation::sDDFQueueRegister(t) => {
                        handle_queue_register(rs_conn, &mut cpy.as_mut().unwrap(), &t)
                    }
                    SMOS_Invocation::sDDFGetDataRegion => {
                        handle_get_data_region(&mut cpy.as_mut().unwrap(), rcv_dma_region)
                    }
                    _ => panic!("Should not get any other invocations!"),
                }
            };

            reply_msg_info = smos_serv_cleanup(invocation.unwrap(), recv_slot, ret);
        } else {
            reply_msg_info = None;
        }

        if cpy.as_ref().unwrap().initialized {
            reply.cap.send(reply_msg_info.unwrap());
            return cpy.unwrap();
        }
    }
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

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    /* Allocate a cap recieve slot */
    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

    let copier = pre_init(
        &rs_conn,
        &mut cspace,
        &listen_conn,
        &reply,
        recv_slot,
        &rcv_dma_region,
    );

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
        resources = Resources {
            rx_free_drv: DRV_FREE as u64,
            rx_active_drv: DRV_ACTIVE as u64,
            drv_queue_size: DRV_QUEUE_CAPACITY as u64,
            buffer_data_vaddr: RCV_DMA as u64,
            buffer_data_paddr: rcv_dma_region.paddr as u64,

            driver_id: drv_channel.from_bit.unwrap(),
            num_network_clients: 1,

            clients: [Client {
                rx_free: CPY_FREE as u64,
                rx_active: CPY_ACTIVE as u64,
                queue_size: CPY_QUEUE_CAPACITY as u64,
                client_id: copier.channel.unwrap().from_bit.unwrap(),
                mac_addr: CLI_MAC_ADDR as u64,
            }],
        }
    }

    unsafe { sddf_init() };

    sddf_event_loop(listen_conn, reply);
}
