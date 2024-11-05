#![no_std]
#![no_main]

use smos_common::client_connection::ClientConnection;
use smos_common::connection::{sDDFConnection, RootServerConnection};
use smos_common::sddf::QueueType;
use smos_common::syscall::{sDDFInterface, NonRootServerInterface, RootServerInterface};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_sddf::dma_region::DMARegion;
use smos_sddf::notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden};
use smos_sddf::queue::QueuePair;
extern crate alloc;
use alloc::vec::Vec;
use smos_common::error::InvocationError;
use smos_common::local_handle::{
    ConnRegistrationHandle, ConnectionHandle, HandleOrHandleCap, LocalHandle, ObjectHandle,
};
use smos_common::server_connection::ServerConnection;
use smos_common::syscall::ReplyWrapper;
use smos_sddf::queue::{ActiveQueue, FreeQueue, Queue};
use smos_sddf::sddf_bindings::{sddf_event_loop, sddf_init, sddf_set_channel};
use smos_sddf::sddf_channel::sDDFChannel;
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::event::{smos_serv_cleanup, smos_serv_decode_invocation, smos_serv_replyrecv};
use smos_server::reply::SMOSReply;
use smos_server::syscalls::SMOS_Invocation;
use smos_server::syscalls::{
    sDDFChannelRegisterBidirectional, sDDFProvideDataRegion, sDDFQueueRegister, ConnOpen,
};

#[repr(C)]
struct Resources {
    rx_free_virt: u64,
    rx_active_virt: u64,
    virt_queue_size: u64,
    rx_free_cli: u64,
    rx_active_cli: u64,
    cli_queue_size: u64,
    virt_data_region: u64,
    cli_data_region: u64,
    virt_id: u8,
    cli_id: u8,
}

extern "C" {
    static mut resources: Resources;
}

struct Client {
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    id: usize,
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    active: Option<Queue<ActiveQueue>>,
    free: Option<Queue<FreeQueue>>,
    channel: Option<NotificationChannel<BidirectionalChannel, PPCForbidden>>,
    data: Option<DMARegion>,
    initialized: bool,
}

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;

const VIRT_FREE: usize = 0x2_000_000;
const VIRT_ACTIVE: usize = 0x2_200_000;
const CLI_FREE: usize = 0x2_400_000;
const CLI_ACTIVE: usize = 0x2_600_000;

const CLI_DMA_RCV_REGION: usize = 0x2_800_000;
const VIRT_DMA_RECV_REGION: usize = 0x3_000_000;

const VIRT_QUEUE_SIZE: usize = 0x200_000;
const VIRT_QUEUE_CAPACITY: usize = 512;
const CLIENT_QUEUE_SIZE: usize = 0x200_000;
const CLIENT_QUEUE_CAPACITY: usize = 512;
#[allow(dead_code)]
const CLI_RCV_REGION_SIZE: usize = 0x200_000;
const RCV_DMA_REGION_SIZE: usize = 0x2_200_000;

fn handle_conn_open(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    id: usize,
    args: &ConnOpen,
    slot: &mut Option<Client>,
) -> Result<SMOSReply, InvocationError> {
    /* The virtualizer does not support a shared buffer */
    if args.shared_buf_obj.is_some() {
        return Err(InvocationError::InvalidArguments);
    }

    let registration_handle = rs_conn
        .conn_register(publish_hndl, id)
        .expect("@alwin: Can this be an assertion?");

    *slot = Some(Client {
        id: id,
        conn_registration_hndl: registration_handle,
        free: None,
        active: None,
        channel: None,
        data: None,
        initialized: false,
    });

    return Ok(SMOSReply::ConnOpen);
}

fn handle_client_register(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    client: &mut Client,
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
    client: &mut Client,
    args: &sDDFQueueRegister,
) -> Result<SMOSReply, InvocationError> {
    /* We expect them to register a channel first */
    if client.channel.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    /* We need the size to match what we expect */
    if args.size != CLIENT_QUEUE_SIZE {
        return Err(InvocationError::InvalidArguments);
    }

    match args.queue_type {
        QueueType::Active => {
            if client.active.is_some() {
                sel4::debug_println!("Already set up an active queue");
                return Err(InvocationError::InvalidArguments);
            }

            client.active = Some(Queue::<ActiveQueue>::open(
                rs_conn,
                CLI_ACTIVE,
                args.size,
                HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
            )?);
        }
        QueueType::Free => {
            if client.free.is_some() {
                sel4::debug_println!("Already set up an free queue");
                return Err(InvocationError::InvalidArguments);
            }

            client.free = Some(Queue::<FreeQueue>::open(
                rs_conn,
                CLI_FREE,
                args.size,
                HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
            )?);
        }
    }

    return Ok(SMOSReply::sDDFQueueRegister);
}

fn handle_provide_data_region(
    rs_conn: &RootServerConnection,
    client: &mut Client,
    args: &sDDFProvideDataRegion,
) -> Result<SMOSReply, InvocationError> {
    if client.active.is_none() || client.free.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    client.data = Some(DMARegion::open(
        rs_conn,
        HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
        CLI_DMA_RCV_REGION,
        CLIENT_QUEUE_SIZE,
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
) -> Client {
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
                        handle_conn_open(&rs_conn, listen_conn.hndl(), id, &t, &mut client)
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

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) {
    sel4::debug_println!("Hello, I am eth copier!!!");
    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 2);

    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot for ep");
    let listen_conn = rs_conn
        .conn_publish::<sDDFConnection>(NTFN_BUFFER, &cspace.to_absolute_cptr(ep_cptr), args[0])
        .expect("Could not publish as a server");

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    /* Allocate a cap recieve slot */
    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

    /* Allow client to connect */
    let client = pre_init(&rs_conn, &mut cspace, &listen_conn, &reply, recv_slot);

    /* Create queue pair for virtualiser */
    let virt_queues = QueuePair::new(
        &rs_conn,
        &mut cspace,
        VIRT_ACTIVE,
        VIRT_FREE,
        VIRT_QUEUE_SIZE,
    )
    .expect("Failed to create virt queue pair");

    /* Create connection to the rx virtualizer */
    let conn_ep_slot = cspace.alloc_slot().expect("Failed to allocate slot");
    let mut virt_conn = rs_conn
        .conn_create::<sDDFConnection>(&cspace.to_absolute_cptr(conn_ep_slot), args[1])
        .expect("Failed to establish connection to rx virtualiser");

    virt_conn
        .conn_open(None)
        .expect("Failed to open connection with virt");

    let virt_channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::new(
        &rs_conn,
        &virt_conn,
        &mut cspace,
        &listen_conn.hndl(),
        None,
    )
    .expect("Failed to establish connection with virt");

    virt_conn
        .sddf_queue_register(
            virt_queues.active.obj_hndl_cap.unwrap(),
            virt_queues.active.size,
            QueueType::Active,
        )
        .expect("Failed to register active queue");

    virt_conn
        .sddf_queue_register(
            virt_queues.free.obj_hndl_cap.unwrap(),
            virt_queues.free.size,
            QueueType::Free,
        )
        .expect("Failed to register free queue");

    let virt_rcv_dma_hndl_cap_slot = cspace.alloc_slot().expect("Could not allocate slot");
    let virt_rcv_dma_hndl_cap = virt_conn
        .sddf_get_data_region(&cspace.to_absolute_cptr(virt_rcv_dma_hndl_cap_slot))
        .expect("Failed to get data region");

    let _rcv_dma_region = DMARegion::open(
        &rs_conn,
        virt_rcv_dma_hndl_cap,
        VIRT_DMA_RECV_REGION,
        RCV_DMA_REGION_SIZE,
    )
    .expect("Failed to open DMA region");

    sddf_set_channel(
        virt_channel.from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(virt_channel),
    )
    .expect("Failed to create channel with virtualiser");
    sddf_set_channel(
        client.channel.unwrap().from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(client.channel.unwrap()),
    )
    .expect("Failed to create channel with with client");

    unsafe {
        resources = Resources {
            rx_free_virt: VIRT_FREE as u64,
            rx_active_virt: VIRT_ACTIVE as u64,
            virt_queue_size: VIRT_QUEUE_CAPACITY as u64,
            rx_free_cli: CLI_FREE as u64,
            rx_active_cli: CLI_ACTIVE as u64,
            cli_queue_size: CLIENT_QUEUE_CAPACITY as u64,
            virt_data_region: VIRT_DMA_RECV_REGION as u64,
            cli_data_region: CLI_DMA_RCV_REGION as u64,
            virt_id: virt_channel.from_bit.unwrap(),
            cli_id: client.channel.unwrap().from_bit.unwrap(),
        }
    }

    unsafe { sddf_init() };

    sddf_event_loop(listen_conn, reply);
}
