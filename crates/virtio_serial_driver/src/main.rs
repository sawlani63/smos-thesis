#![no_std]
#![no_main]

extern crate alloc;
use core::mem::MaybeUninit;

use alloc::vec::Vec;
use smos_common::client_connection::ClientConnection;
use smos_common::connection::RootServerConnection;
use smos_common::local_handle::ConnectionHandle;
use smos_common::local_handle::HandleOrHandleCap;
use smos_common::local_handle::LocalHandle;
use smos_common::local_handle::ObjectHandle;
use smos_common::syscall::RootServerInterface;
use smos_common::util::ROUND_DOWN;
use smos_common::{
    connection::sDDFConnection, error::InvocationError, local_handle::ConnRegistrationHandle,
    sddf::VirtType,
};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_runtime::Never;
use smos_sddf::device_config::DeviceIrqResource;
use smos_sddf::device_config::DeviceRegionResource;
use smos_sddf::device_config::DEVICE_MAX_IRQS;
use smos_sddf::device_config::DEVICE_MAX_REGIONS;
use smos_sddf::device_region::DeviceRegion;
use smos_sddf::driver_setup::sDDFClient;
use smos_sddf::driver_setup::sddf_driver_pre_init;
use smos_sddf::driver_setup::VirtRegistration;
use smos_sddf::queue::Queue;
use smos_sddf::sddf_bindings::init;
use smos_sddf::sddf_bindings::sddf_event_loop;
use smos_sddf::sddf_bindings::sddf_set_channel;
use smos_sddf::sddf_channel::sDDFChannel;
use smos_sddf::{
    config::RegionResource,
    device_config::{DeviceResources, SDDF_DEVICE_MAGIC},
    dma_region::DMARegion,
    irq_channel::IrqChannel,
    notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden},
    queue::{ActiveQueue, SerialQueue},
    serial_config::{SerialConnectionResource, SerialDriverConfig, SDDF_SERIAL_MAGIC},
};
use smos_server::reply::SMOSReply;
use smos_server::syscalls::sDDFProvideDataRegion;
use smos_server::syscalls::{sDDFChannelRegisterBidirectional, sDDFQueueRegister};

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;
const REGS_BASE: *const u32 = 0xB000000 as *const u32;

const VIRT_RX_QUEUE: *const u8 = 0xC000000 as *const u8;
const RX_QUEUE_SIZE: usize = 4096;
const VIRT_TX_QUEUE: *const u8 = 0xC001000 as *const u8;

const VIRT_RX_DATA: *const u8 = 0xD000000 as *const u8;
const VIRT_TX_DATA: *const u8 = 0xD200000 as *const u8;

const RX_VIRTIO_BUF: *const u8 = 0xE000000 as *const u8;
const TX_VIRTIO_BUF: *const u8 = 0xE001000 as *const u8;

const SERIAL_REGS_PADDR: usize = 0xa003c00 as usize;
const SERIAL_OFFSET: usize =
    SERIAL_REGS_PADDR - ROUND_DOWN(SERIAL_REGS_PADDR as usize, sel4_sys::seL4_PageBits as usize);
const SERIAL_IRQ_NUMBER: usize = 78;
const HW_RING: *const u8 = 0xE002000 as *const u8;
const HW_RING_BUFFER_SIZE: usize = 0x10_000;

extern "C" {
    static mut device_resources: DeviceResources;
    static mut config: SerialDriverConfig;
}

#[derive(Copy, Clone)]
struct sDDFSerialDriverClient {
    id: usize,
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    virt_type: Option<VirtType>,
    queue: Option<Queue<SerialQueue>>,
    data: Option<DMARegion>,
    channel: Option<NotificationChannel<BidirectionalChannel, PPCForbidden>>,
}

impl sDDFClient for sDDFSerialDriverClient {
    fn new(
        id: usize,
        registration_handle: LocalHandle<ConnRegistrationHandle>,
    ) -> sDDFSerialDriverClient {
        return sDDFSerialDriverClient {
            id: id,
            conn_registration_hndl: registration_handle,
            virt_type: None,
            queue: None,
            data: None,
            channel: None,
        };
    }

    fn get_id(&self) -> usize {
        self.id
    }

    fn initialized(&self) -> bool {
        self.queue.is_some() && self.data.is_some()
    }
}

fn handle_virt_register(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    client: &mut sDDFSerialDriverClient,
    virt_reg_status: &mut VirtRegistration,
    args: &sDDFChannelRegisterBidirectional,
) -> Result<SMOSReply, InvocationError> {
    if args.virt_type.is_none() {
        sel4::debug_println!("hello there!");
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
    client: &mut sDDFSerialDriverClient,
    args: &sDDFQueueRegister,
) -> Result<SMOSReply, InvocationError> {
    if client.virt_type.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    // @alwin: Different cases for rx and tx
    if args.size != RX_QUEUE_SIZE {
        return Err(InvocationError::InvalidArguments);
    }

    if client.queue.is_some() {
        return Err(InvocationError::InvalidArguments);
    }

    let window_vaddr = match client.virt_type {
        Some(VirtType::Tx) => VIRT_TX_QUEUE,
        Some(VirtType::Rx) => VIRT_RX_QUEUE,
        _ => panic!("Should not hit this"),
    };

    client.queue = Some(Queue::<SerialQueue>::open(
        rs_conn,
        window_vaddr as usize,
        args.size,
        HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
    )?);

    return Ok(SMOSReply::sDDFQueueRegister);
}

fn handle_provide_data_region(
    rs_conn: &RootServerConnection,
    client: &mut sDDFSerialDriverClient,
    args: &sDDFProvideDataRegion,
) -> Result<SMOSReply, InvocationError> {
    if client.queue.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    client.data = Some(DMARegion::open(
        rs_conn,
        HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
        if client.virt_type == Some(VirtType::Tx) {
            VIRT_TX_DATA as usize
        } else {
            VIRT_RX_DATA as usize
        },
        args.size,
    )?);

    return Ok(SMOSReply::sDDFProvideDataRegion);
}

fn check_done(clients: [Option<sDDFSerialDriverClient>; 2]) -> bool {
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
    sel4::debug_println!("Hello, I am serial0!!!");

    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 1);

    /* Register as a server */
    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let listen_conn = rs_conn
        .conn_publish::<sDDFConnection>(NTFN_BUFFER, &cspace.to_absolute_cptr(ep_cptr), args[0])
        .expect("Failed to publish as server");

    /* Map in the serial registers */
    // @alwin: Will this be on the same page as the virtio-net device. PLEASE NO!
    let device_region = DeviceRegion::new(
        &rs_conn,
        REGS_BASE as usize,
        0x1000,
        ROUND_DOWN(
            SERIAL_REGS_PADDR as usize,
            sel4_sys::seL4_PageBits.try_into().unwrap(),
        ),
    )
    .expect("Failed to create eth device region");

    /* Register for the IRQ */
    let irq_channel = IrqChannel::new(
        &rs_conn,
        &mut cspace,
        &listen_conn.hndl(),
        SERIAL_IRQ_NUMBER,
    )
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

    let rx_char_buf = DMARegion::new(&rs_conn, &mut cspace, RX_VIRTIO_BUF as usize, 0x1000, false)
        .expect("Failed to allocate rx buf");

    let tx_char_buf = DMARegion::new(&rs_conn, &mut cspace, TX_VIRTIO_BUF as usize, 0x1000, false)
        .expect("Failed to allocate tx buf");

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    /* Allocate a cap recieve slot */
    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

    /* Wait for connections to be established with the rx and tx virtualizers */
    let virts = sddf_driver_pre_init::<sDDFConnection, sDDFSerialDriverClient, 2>(
        &rs_conn,
        &mut cspace,
        &listen_conn,
        &reply,
        recv_slot,
        Some(handle_virt_register),
        Some(handle_queue_register),
        Some(handle_provide_data_region),
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
        config = SerialDriverConfig {
            magic: SDDF_SERIAL_MAGIC,
            rx: SerialConnectionResource {
                queue: RegionResource {
                    vaddr: rx_virt.queue.unwrap().vaddr,
                    size: rx_virt.queue.unwrap().size,
                },
                data: RegionResource {
                    vaddr: rx_virt.data.unwrap().vaddr,
                    size: rx_virt.data.unwrap().size,
                },
                id: rx_virt.channel.unwrap().from_bit.unwrap(),
            },
            tx: SerialConnectionResource {
                queue: RegionResource {
                    vaddr: tx_virt.queue.unwrap().vaddr,
                    size: tx_virt.queue.unwrap().size,
                },
                data: RegionResource {
                    vaddr: tx_virt.data.unwrap().vaddr,
                    size: tx_virt.data.unwrap().size,
                },
                id: tx_virt.channel.unwrap().from_bit.unwrap(),
            },
            default_baud: 12800,
            rx_enabled: true,
        };

        let mut device_regions: [MaybeUninit<DeviceRegionResource>; DEVICE_MAX_REGIONS] =
            MaybeUninit::uninit().assume_init();
        let mut device_irqs: [MaybeUninit<DeviceIrqResource>; DEVICE_MAX_IRQS] =
            MaybeUninit::uninit().assume_init();

        device_regions[0] = MaybeUninit::new(DeviceRegionResource {
            region: RegionResource {
                vaddr: device_region.vaddr + SERIAL_OFFSET,
                size: device_region.size,
            },
            io_addr: SERIAL_REGS_PADDR,
        });
        device_regions[1] = MaybeUninit::new(DeviceRegionResource {
            region: RegionResource {
                vaddr: hw_ring_buffer.vaddr,
                size: hw_ring_buffer.size,
            },
            io_addr: hw_ring_buffer.paddr,
        });
        device_regions[2] = MaybeUninit::new(DeviceRegionResource {
            region: RegionResource {
                vaddr: rx_char_buf.vaddr,
                size: rx_char_buf.size,
            },
            io_addr: rx_char_buf.paddr,
        });
        device_regions[3] = MaybeUninit::new(DeviceRegionResource {
            region: RegionResource {
                vaddr: tx_char_buf.vaddr,
                size: tx_char_buf.size,
            },
            io_addr: tx_char_buf.paddr,
        });

        device_irqs[0] = MaybeUninit::new(DeviceIrqResource {
            id: irq_channel.bit,
        });

        device_resources = DeviceResources {
            magic: SDDF_DEVICE_MAGIC,
            num_regions: 4,
            num_irqs: 1,
            regions: device_regions,
            irqs: device_irqs,
        };
    }

    unsafe { init() };

    sddf_event_loop(listen_conn, reply);
}
