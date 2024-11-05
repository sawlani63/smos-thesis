#![no_std]
#![no_main]

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
use smos_sddf::dma_region::DMARegion;
use smos_sddf::notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden};
use smos_sddf::queue::{ActiveQueue, FreeQueue, Queue, QueuePair};
use smos_sddf::sddf_bindings::{sddf_event_loop, sddf_init, sddf_set_channel};
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

#[repr(C)]
struct Client {
    tx_free: u64,
    tx_active: u64,
    queue_size: u64,
    client_ch: u8,
    buffer_data_region_vaddr: u64,
    buffer_data_region_paddr: u64,
}

#[repr(C)]
struct Resources {
    tx_free_drv: u64,
    tx_active_drv: u64,
    drv_queue_size: u64,
    drv_ch: u8,
    num_network_clients: u8,
    clients: [Client; 1],
}

extern "C" {
    static mut resources: Resources;
}

struct CliConn {
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    id: usize,
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    active: Option<Queue<ActiveQueue>>,
    free: Option<Queue<FreeQueue>>,
    data: Option<DMARegion>,
    channel: Option<NotificationChannel<BidirectionalChannel, PPCForbidden>>,
    initialized: bool,
}

fn handle_conn_open(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    id: usize,
    args: &ConnOpen,
    slot: &mut Option<CliConn>,
) -> Result<SMOSReply, InvocationError> {
    /* The virtualizer does not support a shared buffer */
    if args.shared_buf_obj.is_some() {
        return Err(InvocationError::InvalidArguments);
    }

    let registration_handle = rs_conn
        .conn_register(publish_hndl, id)
        .expect("@alwin: Can this be an assertion?");

    *slot = Some(CliConn {
        id: id,
        conn_registration_hndl: registration_handle,
        free: None,
        active: None,
        data: None,
        channel: None,
        initialized: false,
    });

    return Ok(SMOSReply::ConnOpen);
}

fn handle_client_register(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    client: &mut CliConn,
    args: &sDDFChannelRegisterBidirectional,
) -> Result<SMOSReply, InvocationError> {
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
    client: &mut CliConn,
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
    }

    return Ok(SMOSReply::sDDFQueueRegister);
}

fn handle_provide_data_region(
    rs_conn: &RootServerConnection,
    client: &mut CliConn,
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
    client.initialized = true;

    return Ok(SMOSReply::sDDFProvideDataRegion);
}

fn pre_init<T: ServerConnection>(
    rs_conn: &RootServerConnection,
    cspace: &mut SMOSUserCSpace,
    listen_conn: &T,
    reply: &ReplyWrapper,
    recv_slot: sel4::AbsoluteCPtr,
) -> CliConn {
    let mut client = None;
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

            let ret = if client.is_none() {
                match invocation.as_ref().unwrap() {
                    SMOS_Invocation::ConnOpen(t) => {
                        handle_conn_open(&rs_conn, listen_conn.hndl(), id, t, &mut client)
                    }
                    _ => todo!(), // @alwin: Client calls something before opening connection
                }
            } else {
                match invocation.as_ref().unwrap() {
                    SMOS_Invocation::ConnOpen(_) => todo!(), // @alwin: Client calls conn_open again
                    SMOS_Invocation::sDDFChannelRegisterBidirectional(t) => handle_client_register(
                        rs_conn,
                        listen_conn.hndl(),
                        cspace,
                        &mut client.as_mut().unwrap(),
                        &t,
                    ),
                    SMOS_Invocation::sDDFQueueRegister(t) => {
                        handle_queue_register(rs_conn, &mut client.as_mut().unwrap(), &t)
                    }
                    SMOS_Invocation::sDDFProvideDataRegion(t) => {
                        handle_provide_data_region(rs_conn, &mut client.as_mut().unwrap(), &t)
                    }
                    _ => panic!("Should not get any other invocations!"),
                }
            };

            reply_msg_info = smos_serv_cleanup(invocation.unwrap(), recv_slot, ret);
        } else {
            reply_msg_info = None;
        }

        if client.as_ref().unwrap().initialized {
            reply.cap.send(reply_msg_info.unwrap());
            return client.unwrap();
        }
    }
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

    let client = pre_init(&rs_conn, &mut cspace, &listen_conn, &reply, recv_slot);

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
        resources = Resources {
            tx_free_drv: DRV_FREE as u64,
            tx_active_drv: DRV_ACTIVE as u64,
            drv_queue_size: DRV_QUEUE_CAPACITY as u64,
            drv_ch: drv_channel.from_bit.unwrap(),
            num_network_clients: 1,
            clients: [Client {
                tx_free: CLI0_FREE as u64,
                tx_active: CLI0_ACTIVE as u64,
                queue_size: CLI_QUEUE_CAPACITY as u64,
                client_ch: client.channel.unwrap().from_bit.unwrap(),
                buffer_data_region_vaddr: CLI0_TX_DMA_REGION as u64,
                buffer_data_region_paddr: client.data.unwrap().paddr as u64,
            }],
        }
    }

    unsafe { sddf_init() };

    sddf_event_loop(listen_conn, reply);
}
