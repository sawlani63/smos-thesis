#![no_std]
#![no_main]

use smos_common::client_connection::ClientConnection;
use smos_common::connection::{sDDFConnection, RootServerConnection};
use smos_common::syscall::RootServerInterface;
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_sddf::notification_channel::{NotificationChannel, PPCForbidden, SendOnlyChannel};
extern crate alloc;
use alloc::vec::Vec;
use smos_common::error::InvocationError;
use smos_common::local_handle::{ConnRegistrationHandle, ConnectionHandle, LocalHandle};
use smos_common::server_connection::ServerConnection;
use smos_common::syscall::ReplyWrapper;
use smos_sddf::irq_channel::IrqChannel;
use smos_sddf::sddf_bindings::{sddf_event_loop_ppc, sddf_init, sddf_set_channel};
use smos_sddf::sddf_channel::sDDFChannel;
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::event::{smos_serv_cleanup, smos_serv_decode_invocation, smos_serv_replyrecv};
use smos_server::reply::SMOSReply;
use smos_server::syscalls::SMOS_Invocation;
use smos_server::syscalls::{sDDFChannelRegisterRecvOnly, ConnOpen};

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;
#[allow(dead_code)]
const REGS_BASE: *const u32 = 0xB000000 as *const u32;
const TIMER_ID: usize = 2;

#[repr(C)]
struct Resources {
    irq_id: u8,
}

extern "C" {
    static mut resources: Resources;
}

struct Client {
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    id: usize,
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    timer_id: usize,
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    channel: Option<NotificationChannel<SendOnlyChannel, PPCForbidden>>,
}

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
        timer_id: TIMER_ID,
        conn_registration_hndl: registration_handle,
        channel: None,
    });

    return Ok(SMOSReply::ConnOpen);
}

fn handle_client_register(
    rs_conn: &RootServerConnection,
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
                    SMOS_Invocation::sDDFChannelRegisterRecvOnly(t) => {
                        handle_client_register(rs_conn, cspace, &mut client.as_mut().unwrap(), &t)
                    }
                    _ => panic!("Should not get any other invocations!"),
                }
            };

            reply_msg_info = smos_serv_cleanup(invocation.unwrap(), recv_slot, ret);
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
        .conn_publish::<sDDFConnection>(NTFN_BUFFER, &cspace.to_absolute_cptr(ep_cptr), args[0])
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
    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

    /* Allow client to connect */
    let client = pre_init(&rs_conn, &mut cspace, &listen_conn, &reply, recv_slot);

    sddf_set_channel(
        irq_channel.bit as usize,
        None,
        sDDFChannel::IrqChannel(irq_channel),
    )
    .expect("Could not set up IRQ Channel");
    sddf_set_channel(
        2, // @alwin: This probably shouldn't be hardcoded
        Some(client.id),
        sDDFChannel::NotificationChannelSend(client.channel.unwrap()),
    )
    .expect("Could not set up channel with client");

    unsafe {
        resources = Resources {
            irq_id: irq_channel.bit,
        }
    }
    unsafe { sddf_init() }

    sddf_event_loop_ppc(listen_conn, reply);
}
