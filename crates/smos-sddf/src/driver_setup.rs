use crate::dma_region::DMARegion;
use bitflags::bitflags;
use smos_common::connection::sDDFConnection;
use smos_common::local_handle::ConnRegistrationHandle;
use smos_common::sddf::VirtType;
use smos_common::server_connection::ServerConnection;
use smos_common::syscall::ReplyWrapper;
use smos_common::syscall::RootServerInterface;
use smos_common::{
    connection::RootServerConnection,
    error::InvocationError,
    local_handle::{ConnectionHandle, LocalHandle},
};
use smos_cspace::SMOSUserCSpace;
use smos_server::event::decode_entry_type;
use smos_server::event::smos_serv_cleanup;
use smos_server::event::smos_serv_decode_invocation;
use smos_server::event::smos_serv_replyrecv;
use smos_server::event::EntryType;
use smos_server::syscalls::sDDFChannelRegisterBidirectional;
use smos_server::syscalls::sDDFQueueRegister;
use smos_server::syscalls::ConnOpen;
use smos_server::syscalls::SMOS_Invocation;
use smos_server::{reply::SMOSReply, syscalls::sDDFProvideDataRegion};

pub trait sDDFClient {
    fn new(id: usize, conn_handle: LocalHandle<ConnRegistrationHandle>) -> Self;
    fn get_id(&self) -> usize;
    fn initialized(&self) -> bool;
}

bitflags! {
    /// Represents a set of flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct VirtRegistration: u32 { // @alwin: Rethink this
        /// The value `A`, at bit position `0`.
        const Tx = 0b00000001;
        /// The value `B`, at bit position `1`.
        const Rx = 0b00000010;
    }
}

impl Into<VirtRegistration> for VirtType {
    fn into(self) -> VirtRegistration {
        match self {
            VirtType::Tx => VirtRegistration::Tx,
            VirtType::Rx => VirtRegistration::Rx,
        }
    }
}

fn find_client_from_id<'a, V: sDDFClient>(
    id: usize,
    clients: &'a mut [Option<V>],
) -> Option<&'a mut Option<V>> {
    for client in clients.iter_mut() {
        if client.as_ref().is_some() && client.as_ref().unwrap().get_id() == id {
            return Some(client);
        }
    }

    return None;
}

fn find_client_slot<'a, V: sDDFClient>(clients: &'a mut [Option<V>]) -> Option<&'a mut Option<V>> {
    for client in clients.iter_mut() {
        if client.as_ref().is_none() {
            return Some(client);
        }
    }

    return None;
}

fn handle_conn_open<V: sDDFClient>(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    id: usize,
    args: &ConnOpen,
    clients: &mut [Option<V>],
) -> Result<SMOSReply, InvocationError> {
    let slot = find_client_slot(clients).ok_or(InvocationError::InsufficientResources)?;

    /* The eth driver does not support a shared buffer */
    if args.shared_buf_obj.is_some() {
        return Err(InvocationError::InvalidArguments);
    }

    let registration_handle = rs_conn
        .conn_register(publish_hndl, id)
        .expect("@alwin: Can this be an assertion?");

    *slot = Some(V::new(id, registration_handle));

    return Ok(SMOSReply::ConnOpen);
}

// fn handle_provide_data_region(
//     rs_conn: &RootServerConnection,
//     client: &mut CliConn,
//     args: &sDDFProvideDataRegion,
// ) -> Result<SMOSReply, InvocationError> {
//     if client.active.is_none() || client.free.is_none() {
//         return Err(InvocationError::InvalidArguments);
//     }
//     client.data = Some(DMARegion::open(
//         rs_conn,
//         HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
//         CLI0_TX_DMA_REGION,
//         0x200_000,
//     )?);
//     client.initialized = true;

//     return Ok(SMOSReply::sDDFProvideDataRegion);
// }

pub fn sddf_driver_pre_init<T: ServerConnection, V: sDDFClient + Copy, const N: usize>(
    rs_conn: &RootServerConnection,
    cspace: &mut SMOSUserCSpace,
    listen_conn: &T,
    reply: &ReplyWrapper,
    recv_slot: sel4::AbsoluteCPtr,
    virt_register_handler: Option<
        fn(
            &RootServerConnection,
            &LocalHandle<ConnectionHandle>,
            &mut SMOSUserCSpace,
            &mut V,
            &mut VirtRegistration,
            &sDDFChannelRegisterBidirectional,
        ) -> Result<SMOSReply, InvocationError>,
    >,
    queue_register_handler: Option<
        fn(&RootServerConnection, &mut V, &sDDFQueueRegister) -> Result<SMOSReply, InvocationError>,
    >,
    provide_data_region_handler: Option<
        fn(
            &RootServerConnection,
            &mut V,
            &sDDFProvideDataRegion,
        ) -> Result<SMOSReply, InvocationError>,
    >,
    get_data_region_handler: Option<(
        fn(&mut V, &DMARegion) -> Result<SMOSReply, InvocationError>,
        &DMARegion,
    )>, // @alwin: fixme
    check_done: fn([Option<V>; N]) -> bool,
) -> [Option<V>; N] {
    let mut reply_msg_info = None;

    /* Specify the slot in which we should recieve caps */
    sel4::with_ipc_buffer_mut(|ipc_buf| {
        ipc_buf.set_recv_slot(&recv_slot);
    });

    let mut clients: [Option<V>; N] = [None; N];
    let mut virt_reg = VirtRegistration::empty();

    loop {
        let (msg, badge) = smos_serv_replyrecv(listen_conn, reply, reply_msg_info);

        match decode_entry_type(badge.try_into().unwrap()) {
            EntryType::Fault(_) => panic!("Driver does not expect to handle faults"),
            EntryType::Notification(_) => {
                // We ignore notifications in this pre-init phase
                reply_msg_info = None;
            }
            EntryType::Invocation(id) => {
                let client = find_client_from_id(id, &mut clients);

                let invocation =
                    smos_serv_decode_invocation::<sDDFConnection>(&msg, recv_slot, None);
                if let Err(e) = invocation {
                    reply_msg_info = e;
                    continue;
                }

                if client.is_none() && !matches!(invocation, Ok(SMOS_Invocation::ConnOpen(_))) {
                    todo!();
                } else if client.is_some() && matches!(invocation, Ok(SMOS_Invocation::ConnOpen(_)))
                {
                    todo!();
                }

                let ret = if matches!(invocation, Ok(SMOS_Invocation::ConnOpen(_))) {
                    match invocation.as_ref().unwrap() {
                        SMOS_Invocation::ConnOpen(t) => {
                            handle_conn_open(&rs_conn, listen_conn.hndl(), id, &t, &mut clients)
                        }
                        _ => panic!("No invocations besides conn_open should be handled here"),
                    }
                } else {
                    let client_unwrapped = client.unwrap().as_mut().unwrap();

                    match invocation.as_ref().unwrap() {
                        SMOS_Invocation::ConnOpen(_) => {
                            panic!("conn_open should never be handled here")
                        }
                        SMOS_Invocation::sDDFChannelRegisterBidirectional(t) => {
                            if virt_register_handler.is_none() {
                                todo!()
                            } else {
                                virt_register_handler.unwrap()(
                                    rs_conn,
                                    listen_conn.hndl(),
                                    cspace,
                                    client_unwrapped,
                                    &mut virt_reg,
                                    &t,
                                )
                            }
                        }
                        SMOS_Invocation::sDDFQueueRegister(t) => {
                            if queue_register_handler.is_none() {
                                todo!()
                            } else {
                                queue_register_handler.unwrap()(rs_conn, client_unwrapped, t)
                            }
                        }
                        SMOS_Invocation::sDDFProvideDataRegion(t) => {
                            if provide_data_region_handler.is_none() {
                                todo!()
                            } else {
                                provide_data_region_handler.unwrap()(rs_conn, client_unwrapped, &t)
                            }
                        }
                        SMOS_Invocation::sDDFGetDataRegion => {
                            if get_data_region_handler.is_none() {
                                todo!()
                            } else {
                                let (handler, mem) = get_data_region_handler.as_ref().unwrap();
                                handler(client_unwrapped, mem)
                            }
                        }
                        _ => panic!("Should not get any other invocations"),
                    }
                };

                /* We delete any cap that was recieved. If a handler wants to hold onto a cap, it
                is their responsibility to copy it somewhere else */
                reply_msg_info = smos_serv_cleanup(invocation.unwrap(), recv_slot, ret);
            }
        }

        if check_done(clients) {
            reply.cap.send(reply_msg_info.unwrap());
            return clients;
        }
    }
}
