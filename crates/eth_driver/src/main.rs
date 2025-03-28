#![no_std]
#![no_main]

extern crate alloc;
use core::mem::MaybeUninit;

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
use smos_common::util::ROUND_DOWN;
use smos_cspace::SMOSUserCSpace;
use smos_runtime::{smos_declare_main, Never};
use smos_sddf::config::RegionResource;
use smos_sddf::device_config::{
    DeviceIrqResource, DeviceRegionResource, DeviceResources, DEVICE_MAX_IRQS, DEVICE_MAX_REGIONS,
    SDDF_DEVICE_MAGIC,
};
use smos_sddf::device_region::{self, DeviceRegion};
use smos_sddf::dma_region::DMARegion;
use smos_sddf::driver_setup::sDDFClient;
use smos_sddf::driver_setup::sddf_driver_pre_init;
use smos_sddf::driver_setup::VirtRegistration;
use smos_sddf::irq_channel::IrqChannel;
use smos_sddf::net_config::{NetConnectionResource, NetDriverConfig, SDDF_NET_MAGIC};
use smos_sddf::notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden};
use smos_sddf::queue::{ActiveQueue, FreeQueue, Queue};
use smos_sddf::sddf_bindings::{init, sddf_event_loop, sddf_set_channel};
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

const ETH_REGS_PADDR: usize = 0xa003e00;
const HW_RING_BUFFER_SIZE: usize = 0x10_000;
const ETH_IRQ_NUMBER: usize = 79;
const RX_QUEUE_SIZE: usize = 0x200_000;
const RX_QUEUE_CAPACITY: usize = 512;
// const TX_QUEUE_SIZE: usize = 0x200_000;
const TX_QUEUE_CAPACITY: usize = 512;

const VIRTIO_NET_OFFSET: usize = 0xe00;

extern "C" {
    static mut device_resources: DeviceResources;
    static mut config: NetDriverConfig;
}

#[derive(Copy, Clone)]
struct sDDFNetDriverClient {
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    id: usize,
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    virt_type: Option<VirtType>,
    active: Option<Queue<ActiveQueue>>,
    free: Option<Queue<FreeQueue>>,
    channel: Option<NotificationChannel<BidirectionalChannel, PPCForbidden>>,
}

impl sDDFClient for sDDFNetDriverClient {
    fn new(
        id: usize,
        registration_handle: LocalHandle<ConnRegistrationHandle>,
    ) -> sDDFNetDriverClient {
        return sDDFNetDriverClient {
            id: id,
            conn_registration_hndl: registration_handle,
            virt_type: None,
            free: None,
            active: None,
            channel: None,
        };
    }

    fn get_id(&self) -> usize {
        self.id
    }

    fn initialized(&self) -> bool {
        self.free.is_some() && self.active.is_some()
    }
}

fn handle_virt_register(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    client: &mut sDDFNetDriverClient,
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
    client: &mut sDDFNetDriverClient,
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
        _ => return Err(InvocationError::InvalidArguments),
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
        _ => return Err(InvocationError::InvalidArguments), // Should never actually hit this case
    }

    return Ok(SMOSReply::sDDFQueueRegister);
}

fn check_done(clients: [Option<sDDFNetDriverClient>; 2]) -> bool {
    if clients[0].is_none() || clients[1].is_none() {
        return false;
    }

    if clients[0].as_ref().unwrap().initialized() && clients[1].as_ref().unwrap().initialized() {
        return true;
    }

    return false;
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
    let device_region = DeviceRegion::new(
        &rs_conn,
        REGS_BASE as usize,
        0x1000,
        ROUND_DOWN(ETH_REGS_PADDR, sel4_sys::seL4_PageBits.try_into().unwrap()),
    )
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
    let virts = sddf_driver_pre_init::<sDDFConnection, sDDFNetDriverClient, 2>(
        &rs_conn,
        &mut cspace,
        &listen_conn,
        &reply,
        recv_slot,
        Some(handle_virt_register),
        Some(handle_queue_register),
        None,
        None,
        check_done,
    );

    let (rx_virt, tx_virt) = match virts[0].unwrap().virt_type.as_ref().unwrap() {
        VirtType::Rx => (virts[0].unwrap(), virts[1].unwrap()),
        VirtType::Tx => (virts[1].unwrap(), virts[0].unwrap()),
    };

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
        config = NetDriverConfig {
            magic: SDDF_NET_MAGIC,
            virt_rx: NetConnectionResource {
                free_queue: RegionResource {
                    vaddr: rx_virt.free.unwrap().vaddr,
                    size: 0x200_000,
                },
                active_queue: RegionResource {
                    vaddr: rx_virt.active.unwrap().vaddr,
                    size: 0x200_000,
                },
                num_buffers: 512,
                id: rx_virt.channel.unwrap().from_bit.unwrap(),
            },
            virt_tx: NetConnectionResource {
                free_queue: RegionResource {
                    vaddr: tx_virt.free.unwrap().vaddr,
                    size: 0x200_000,
                },
                active_queue: RegionResource {
                    vaddr: tx_virt.active.unwrap().vaddr,
                    size: 0x200_000,
                },
                num_buffers: 512,
                id: tx_virt.channel.unwrap().from_bit.unwrap(),
            },
        };

        let mut device_regions: [MaybeUninit<DeviceRegionResource>; DEVICE_MAX_REGIONS] =
            MaybeUninit::uninit().assume_init();
        let mut device_irqs: [MaybeUninit<DeviceIrqResource>; DEVICE_MAX_IRQS] =
            MaybeUninit::uninit().assume_init();

        device_regions[0] = MaybeUninit::new(DeviceRegionResource {
            region: RegionResource {
                vaddr: device_region.vaddr + VIRTIO_NET_OFFSET as usize,
                size: device_region.size,
            },
            io_addr: ETH_REGS_PADDR,
        });
        device_regions[1] = MaybeUninit::new(DeviceRegionResource {
            region: RegionResource {
                vaddr: hw_ring_buffer.vaddr,
                size: hw_ring_buffer.size,
            },
            io_addr: hw_ring_buffer.paddr,
        });

        device_irqs[0] = MaybeUninit::new(DeviceIrqResource {
            id: irq_channel.bit,
        });

        device_resources = DeviceResources {
            magic: SDDF_DEVICE_MAGIC,
            num_regions: 2,
            num_irqs: 1,
            regions: device_regions,
            irqs: device_irqs,
        };
    }

    unsafe { init() };

    sddf_event_loop(listen_conn, reply);
}
