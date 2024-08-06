#![no_std]
#![no_main]

use core::ffi::{c_char, CStr};
use smos_common::client_connection::ClientConnection;
use smos_common::connection::{sDDFConnection, ObjectServerConnection, RootServerConnection};
use smos_common::error::InvocationError;
use smos_common::local_handle::{
    ConnRegistrationHandle, ConnectionHandle, HandleOrHandleCap, LocalHandle, ObjectHandle,
};
use smos_common::obj_attributes::ObjAttributes;
use smos_common::sddf::{QueueType, VirtType};
use smos_common::server_connection::ServerConnection;
use smos_common::syscall::{
    sDDFInterface, NonRootServerInterface, ObjectServerInterface, ReplyWrapper, RootServerInterface,
};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_sddf::dma_region::DMARegion;
use smos_sddf::notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden};
use smos_sddf::queue::{ActiveQueue, FreeQueue, Queue, QueuePair};
use smos_sddf::sddf_bindings::{sddf_event_loop, sddf_init, sddf_notified, sddf_set_channel};
use smos_sddf::sddf_channel::sDDFChannel;
use smos_server::error::handle_error;
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::reply::{handle_reply, SMOSReply};
use smos_server::syscalls::{
    sDDFChannelRegisterBidirectional, sDDFProvideDataRegion, sDDFQueueRegister, ConnOpen,
    SMOS_Invocation,
};

extern crate alloc;
use alloc::vec::Vec;

const ntfn_buffer: *mut u8 = 0xB0000 as *mut u8;

const drv_free: usize = 0x3_000_000;
const drv_active: usize = 0x3_200_000;

const cli0_active: usize = 0x3_400_000;
const cli0_free: usize = 0x3_600_000;
const cli0_tx_dma_region: usize = 0x3_800_000;

const drv_queue_size: usize = 0x200_000;
const drv_queue_capacity: usize = 512;
const cli_queue_size: usize = 0x200_000;
const cli_queue_capacity: usize = 512;
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
    clients: [Client; 1],
}

extern "C" {
    pub static mut resources: Resources;
}

struct CliConn {
    id: usize,
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
    args: ConnOpen,
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

    if args.size != cli_queue_size {
        return Err(InvocationError::InvalidArguments);
    }

    match args.queue_type {
        QueueType::Active => {
            if client.active.is_some() {
                return Err(InvocationError::InvalidArguments);
            }

            client.active = Some(Queue::<ActiveQueue>::open(
                rs_conn,
                cli0_active,
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
                cli0_free,
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
        cli0_tx_dma_region,
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
        // @alwin: Put this in a generic function
        let (msg, mut badge) = if reply_msg_info.is_some() {
            listen_conn
                .ep()
                .reply_recv(reply_msg_info.unwrap(), reply.cap)
        } else {
            listen_conn.ep().recv(reply.cap)
        };

        if let EntryType::Invocation(id) = decode_entry_type(badge.try_into().unwrap()) {
            let (invocation, consumed_cap) = sel4::with_ipc_buffer(|buf| {
                SMOS_Invocation::new::<sDDFConnection>(buf, &msg, None, recv_slot)
            });

            // @alwin: Put this in a generic function
            if invocation.is_err() {
                if consumed_cap {
                    recv_slot.delete();
                }
                reply_msg_info = Some(sel4::with_ipc_buffer_mut(|buf| {
                    handle_error(buf, invocation.unwrap_err())
                }));
                continue;
            }

            let ret = if client.is_none() {
                match invocation.unwrap() {
                    SMOS_Invocation::ConnOpen(t) => {
                        handle_conn_open(&rs_conn, listen_conn.hndl(), id, t, &mut client)
                    }
                    _ => todo!(), // @alwin: Client calls something before opening connection
                }
            } else {
                match invocation.unwrap() {
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

            // @alwin: put this in a generic function
            if consumed_cap {
                recv_slot.delete();
                sel4::with_ipc_buffer_mut(|ipc_buf| {
                    ipc_buf.set_recv_slot(&recv_slot);
                });
            }

            reply_msg_info = match ret {
                Ok(x) => Some(sel4::with_ipc_buffer_mut(|buf| handle_reply(buf, x))),
                Err(x) => Some(sel4::with_ipc_buffer_mut(|buf| handle_error(buf, x))),
            };
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
        .conn_publish::<sDDFConnection>(ntfn_buffer, &cspace.to_absolute_cptr(ep_cptr), args[0])
        .expect("Could not publish as a server");

    /* Create the driver queue pair */
    let drv_queues = QueuePair::new(&rs_conn, &mut cspace, drv_active, drv_free, drv_queue_size)
        .expect("Failed to create driver queue pair");

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    /* Allocate a cap recieve slot */
    let mut recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let mut recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

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
    );
    sddf_set_channel(
        client.channel.unwrap().from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(client.channel.unwrap()),
    );

    unsafe {
        resources = Resources {
            tx_free_drv: drv_free as u64,
            tx_active_drv: drv_active as u64,
            drv_queue_size: drv_queue_capacity as u64,
            drv_ch: drv_channel.from_bit.unwrap(),
            clients: [Client {
                tx_free: cli0_free as u64,
                tx_active: cli0_active as u64,
                queue_size: cli_queue_capacity as u64,
                client_ch: client.channel.unwrap().from_bit.unwrap(),
                buffer_data_region_vaddr: cli0_tx_dma_region as u64,
                buffer_data_region_paddr: client.data.unwrap().paddr as u64,
            }],
        }
    }

    unsafe { sddf_init() };

    sddf_event_loop(listen_conn, reply);
}
