#![no_std]
#![no_main]

use core::ffi::{c_char, CStr};
use smos_common::client_connection::ClientConnection;
use smos_common::connection::{sDDFConnection, ObjectServerConnection, RootServerConnection};
use smos_common::obj_attributes::ObjAttributes;
use smos_common::sddf::{QueueType, VirtType};
use smos_common::syscall::{
    sDDFInterface, NonRootServerInterface, ObjectServerInterface, RootServerInterface,
};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_sddf::dma_region::DMARegion;
use smos_sddf::notification_channel::{
    BidirectionalChannel, NotificationChannel, PPCForbidden, SendOnlyChannel,
};
use smos_sddf::queue::QueuePair;
extern crate alloc;
use alloc::vec::Vec;
use smos_common::error::InvocationError;
use smos_common::local_handle::{
    ConnRegistrationHandle, ConnectionHandle, HandleOrHandleCap, LocalHandle, ObjectHandle,
};
use smos_common::server_connection::ServerConnection;
use smos_common::syscall::ReplyWrapper;
use smos_sddf::irq_channel::IrqChannel;
use smos_sddf::queue::{ActiveQueue, FreeQueue, Queue};
use smos_sddf::sddf_bindings::{
    sddf_event_loop_ppc, sddf_init, sddf_notified, sddf_protected, sddf_set_channel,
};
use smos_sddf::sddf_channel::sDDFChannel;
use smos_server::error::handle_error;
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::reply::{handle_reply, SMOSReply};
use smos_server::syscalls::SMOS_Invocation;
use smos_server::syscalls::{
    sDDFChannelRegisterRecvOnly, sDDFProvideDataRegion, sDDFQueueRegister, ConnOpen,
};

const ntfn_buffer: *mut u8 = 0xB0000 as *mut u8;
const regs_base: *const u32 = 0xB000000 as *const u32;
const timer_id: usize = 2;

#[repr(C)]
struct Resources {
    irq_id: u8,
}

extern "C" {
    pub static mut resources: Resources;
}

struct Client {
    id: usize,
    timer_id: usize,
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    channel: Option<NotificationChannel<SendOnlyChannel, PPCForbidden>>,
}

fn handle_conn_open(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    id: usize,
    args: ConnOpen,
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
        timer_id: timer_id,
        conn_registration_hndl: registration_handle,
        channel: None,
    });

    return Ok(SMOSReply::ConnOpen);
}

fn handle_client_register(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    client: &mut Client,
    args: &sDDFChannelRegisterRecvOnly,
) -> Result<SMOSReply, InvocationError> {
    let channel = NotificationChannel::<SendOnlyChannel, PPCForbidden>::open(
        rs_conn,
        cspace,
        args.channel_hndl_cap.into(),
    )?;

    client.channel = Some(channel);

    return Ok(SMOSReply::sDDFChannelRegisterRecvOnly);
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
                    SMOS_Invocation::sDDFChannelRegisterRecvOnly(t) => handle_client_register(
                        rs_conn,
                        listen_conn.hndl(),
                        cspace,
                        &mut client.as_mut().unwrap(),
                        &t,
                    ),
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

        if client.as_ref().unwrap().channel.is_some() {
            reply.cap.send(reply_msg_info.unwrap());
            return client.unwrap();
        }
    }
}

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) {
    sel4::debug_println!("Hello, I am the timer driver");

    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 1);

    /* Register as a server */
    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let listen_conn = rs_conn
        .conn_publish::<sDDFConnection>(ntfn_buffer, &cspace.to_absolute_cptr(ep_cptr), args[0])
        .expect("Could not publish as server");

    /* Register for the timer irq */
    let irq_channel = IrqChannel::new(&rs_conn, &mut cspace, &listen_conn.hndl(), 30)
        .expect("Failed to register for timer irq");

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    /* Allocate a cap recieve slot */
    let mut recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let mut recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

    /* Allow client to connect */
    let client = pre_init(&rs_conn, &mut cspace, &listen_conn, &reply, recv_slot);

    sddf_set_channel(
        irq_channel.bit as usize,
        None,
        sDDFChannel::IrqChannel(irq_channel),
    );
    sddf_set_channel(
        2, // @alwin: This probably shouldn't be hardcoded
        Some(client.id),
        sDDFChannel::NotificationChannelSend(client.channel.unwrap()),
    );

    unsafe {
        resources = Resources {
            irq_id: irq_channel.bit,
        }
    }
    unsafe { sddf_init() }

    sddf_event_loop_ppc(listen_conn, reply);
}
