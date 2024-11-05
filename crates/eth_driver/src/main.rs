#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec::Vec;
use bitflags::bitflags;
use smos_common::client_connection::ClientConnection;
use smos_common::connection::{sDDFConnection, RootServerConnection};
use smos_common::error::InvocationError;
use smos_common::local_handle::{
    ConnRegistrationHandle, ConnectionHandle, HandleOrHandleCap, LocalHandle, ObjectHandle,
};
use smos_common::sddf::{QueueType, VirtType};
use smos_common::server_connection::ServerConnection;
use smos_common::syscall::{ReplyWrapper, RootServerInterface};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::{smos_declare_main, Never};
use smos_sddf::device_region::DeviceRegion;
use smos_sddf::dma_region::DMARegion;
use smos_sddf::irq_channel::IrqChannel;
use smos_sddf::notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden};
use smos_sddf::queue::{ActiveQueue, FreeQueue, Queue};
use smos_sddf::sddf_bindings::{sddf_event_loop, sddf_init, sddf_set_channel};
use smos_sddf::sddf_channel::sDDFChannel;
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::event::{smos_serv_cleanup, smos_serv_decode_invocation, smos_serv_replyrecv};
use smos_server::reply::SMOSReply;
use smos_server::syscalls::{
    sDDFChannelRegisterBidirectional, sDDFQueueRegister, ConnOpen, SMOS_Invocation,
};

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;

const REGS_BASE: *const u32 = 0xB000000 as *const u32;

const VIRT_RX_ACTIVE: *const u8 = 0xC000000 as *const u8;
const VIRT_RX_FREE: *const u8 = 0xC200000 as *const u8;
const VIRT_TX_ACTIVE: *const u8 = 0xC400000 as *const u8;
const VIRT_TX_FREE: *const u8 = 0xC600000 as *const u8;

const HW_RING: *const u8 = 0xD000000 as *const u8;

const ETH_REGS_PADDR: usize = 0xa003000;
const HW_RING_BUFFER_SIZE: usize = 0x10_000;
const ETH_IRQ_NUMBER: usize = 79;
const RX_QUEUE_SIZE: usize = 0x200_000;
const RX_QUEUE_CAPACITY: usize = 512;
// const TX_QUEUE_SIZE: usize = 0x200_000;
const TX_QUEUE_CAPACITY: usize = 512;

#[repr(C)]
struct Resources {
    regs: u64,
    hw_ring_buffer_vaddr: u64,
    hw_ring_buffer_paddr: u64,
    rx_free: u64,
    rx_active: u64,
    tx_free: u64,
    tx_active: u64,
    rx_queue_size: usize,
    tx_queue_size: usize,
    irq_ch: u8,
    tx_ch: u8,
    rx_ch: u8,
}

bitflags! {
    /// Represents a set of flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    struct VirtRegistration: u32 {
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

extern "C" {
    static mut resources: Resources;
}

#[derive(Debug, Copy, Clone)]
struct Client {
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    id: usize,
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    virt_type: Option<VirtType>,
    active: Option<Queue<ActiveQueue>>,
    free: Option<Queue<FreeQueue>>,
    channel: Option<NotificationChannel<BidirectionalChannel, PPCForbidden>>,
    initalized: bool,
}

fn find_client_from_id<'a>(
    id: usize,
    clients: &'a mut [Option<Client>],
) -> Option<&'a mut Option<Client>> {
    for client in clients.iter_mut() {
        if client.as_ref().is_some() && client.as_ref().unwrap().id == id {
            return Some(client);
        }
    }

    return None;
}

fn find_client_slot<'a>(clients: &'a mut [Option<Client>]) -> Option<&'a mut Option<Client>> {
    for client in clients.iter_mut() {
        if client.as_ref().is_none() {
            return Some(client);
        }
    }

    return None;
}

fn handle_conn_open(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    id: usize,
    args: &ConnOpen,
    clients: &mut [Option<Client>],
) -> Result<SMOSReply, InvocationError> {
    let slot = find_client_slot(clients).ok_or(InvocationError::InsufficientResources)?;

    /* The eth driver does not support a shared buffer */
    if args.shared_buf_obj.is_some() {
        return Err(InvocationError::InvalidArguments);
    }

    let registration_handle = rs_conn
        .conn_register(publish_hndl, id)
        .expect("@alwin: Can this be an assertion?");

    *slot = Some(Client {
        id: id,
        conn_registration_hndl: registration_handle,
        virt_type: None,
        free: None,
        active: None,
        channel: None,
        initalized: false,
    });

    return Ok(SMOSReply::ConnOpen);
}

fn handle_virt_register(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    client: &mut Client,
    virt_reg_status: &mut VirtRegistration,
    args: &sDDFChannelRegisterBidirectional,
) -> Result<SMOSReply, InvocationError> {
    if args.virt_type.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    let virt_type = args.virt_type.unwrap();

    /* We can only have one Tx and one Rx virtualizer */
    if !(*virt_reg_status & virt_type.into()).is_empty() {
        return Err(InvocationError::InvalidArguments);
    }

    /* Try and open the channel */
    let channel = NotificationChannel::<BidirectionalChannel, PPCForbidden>::open(
        rs_conn,
        cspace,
        publish_hndl,
        args.channel_hndl_cap.into(),
    )?;

    client.channel = Some(channel);
    client.virt_type = Some(virt_type);

    *virt_reg_status |= virt_type.into();

    return Ok(SMOSReply::sDDFChannelRegisterBidirectional {
        hndl_cap: client.channel.unwrap().from_hndl_cap.unwrap(),
    });
}

fn handle_queue_register(
    rs_conn: &RootServerConnection,
    client: &mut Client,
    args: &sDDFQueueRegister,
) -> Result<SMOSReply, InvocationError> {
    if client.virt_type.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    // @alwin: Different cases for rx and tx
    if args.size != RX_QUEUE_SIZE {
        return Err(InvocationError::InvalidArguments);
    }

    let window_vaddr = match args.queue_type {
        QueueType::Active => {
            if client.active.is_some() {
                return Err(InvocationError::InvalidArguments);
            }

            match client.virt_type {
                Some(VirtType::Rx) => VIRT_RX_ACTIVE,
                Some(VirtType::Tx) => VIRT_TX_ACTIVE,
                _ => panic!("Should not hit this"),
            }
        }
        QueueType::Free => {
            if client.free.is_some() {
                return Err(InvocationError::InvalidArguments);
            }

            match client.virt_type {
                Some(VirtType::Rx) => VIRT_RX_FREE,
                Some(VirtType::Tx) => VIRT_TX_FREE,
                _ => panic!("Should not hit this"),
            }
        }
    };

    match args.queue_type {
        QueueType::Free => {
            client.free = Some(Queue::<FreeQueue>::open(
                rs_conn,
                window_vaddr as usize,
                args.size,
                HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
            )?);
        }
        QueueType::Active => {
            client.active = Some(Queue::<ActiveQueue>::open(
                rs_conn,
                window_vaddr as usize,
                args.size,
                HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
            )?);
        }
    }

    if client.free.is_some() && client.active.is_some() {
        client.initalized = true;
    }

    return Ok(SMOSReply::sDDFQueueRegister);
}

fn pre_init<T: ServerConnection>(
    rs_conn: &RootServerConnection,
    cspace: &mut SMOSUserCSpace,
    listen_conn: &T,
    reply: &ReplyWrapper,
    recv_slot: sel4::AbsoluteCPtr,
) -> (Client, Client) {
    let mut reply_msg_info = None;

    /* Specify the slot in which we should recieve caps */
    sel4::with_ipc_buffer_mut(|ipc_buf| {
        ipc_buf.set_recv_slot(&recv_slot);
    });

    let mut clients: [Option<Client>; 2] = [None; 2];
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
                            handle_virt_register(
                                rs_conn,
                                listen_conn.hndl(),
                                cspace,
                                client_unwrapped,
                                &mut virt_reg,
                                &t,
                            )
                        }
                        SMOS_Invocation::sDDFQueueRegister(t) => {
                            handle_queue_register(rs_conn, client_unwrapped, &t)
                        }
                        _ => panic!("Should not get any other invocations"),
                    }
                };

                /* We delete any cap that was recieved. If a handler wants to hold onto a cap, it
                is their responsibility to copy it somewhere else */
                reply_msg_info = smos_serv_cleanup(invocation.unwrap(), recv_slot, ret);
            }
        }

        if clients[0].is_none() || clients[1].is_none() {
            continue;
        }

        if clients[0].as_ref().unwrap().initalized && clients[1].as_ref().unwrap().initalized {
            reply.cap.send(reply_msg_info.unwrap());
            return match clients[0].as_ref().unwrap().virt_type {
                Some(VirtType::Rx) => (clients[0].unwrap(), clients[1].unwrap()),
                Some(VirtType::Tx) => (clients[1].unwrap(), clients[0].unwrap()),
                None => panic!("Should not reach this with either client beign uninitialized"),
            };
        }
    }
}

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) -> sel4::Result<Never> {
    sel4::debug_println!("Jello, I am eth0!!! Nice to meet u");

    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 1);

    /* Register as a server */
    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let listen_conn = rs_conn
        .conn_publish::<sDDFConnection>(NTFN_BUFFER, &cspace.to_absolute_cptr(ep_cptr), args[0])
        .expect("Could not publish as server");

    /* Map in the ethernet registers */
    let _device_region = DeviceRegion::new(&rs_conn, REGS_BASE as usize, 0x1000, ETH_REGS_PADDR)
        .expect("Failed to create eth device region");

    /* Register for the IRQ  */
    let irq_channel = IrqChannel::new(&rs_conn, &mut cspace, &listen_conn.hndl(), ETH_IRQ_NUMBER)
        .expect("Failed to register for eth irq");

    /* Map in the hardware ring buffer */
    let hw_ring_buffer = DMARegion::new(
        &rs_conn,
        &mut cspace,
        HW_RING as usize,
        HW_RING_BUFFER_SIZE,
        false,
    )
    .expect("Failed to create hw ring buffer DMA region");

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    /* Allocate a cap recieve slot */
    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

    /* Wait for connections to be established with the rx and tx virtualizers */
    let (rx_virt, tx_virt) = pre_init(&rs_conn, &mut cspace, &listen_conn, &reply, recv_slot);

    sddf_set_channel(
        irq_channel.bit as usize,
        None,
        sDDFChannel::IrqChannel(irq_channel),
    )
    .expect("Failed to set up IRQ channel");
    sddf_set_channel(
        tx_virt.channel.unwrap().from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(tx_virt.channel.unwrap()),
    )
    .expect("Failed to set up TX Virt channel");
    sddf_set_channel(
        rx_virt.channel.unwrap().from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(rx_virt.channel.unwrap()),
    )
    .expect("Failed to set up RX Virt channel");

    unsafe {
        resources = Resources {
            regs: REGS_BASE as u64,
            hw_ring_buffer_vaddr: hw_ring_buffer.vaddr as u64,
            hw_ring_buffer_paddr: hw_ring_buffer.paddr as u64,
            rx_free: rx_virt.free.unwrap().vaddr as u64,
            rx_active: rx_virt.active.unwrap().vaddr as u64,
            tx_free: tx_virt.free.unwrap().vaddr as u64,
            tx_active: tx_virt.active.unwrap().vaddr as u64,
            rx_queue_size: RX_QUEUE_CAPACITY,
            tx_queue_size: TX_QUEUE_CAPACITY,
            irq_ch: irq_channel.bit,
            tx_ch: tx_virt.channel.unwrap().from_bit.unwrap(),
            rx_ch: rx_virt.channel.unwrap().from_bit.unwrap(),
        }
    }

    unsafe { sddf_init() };

    sddf_event_loop(listen_conn, reply);
}
