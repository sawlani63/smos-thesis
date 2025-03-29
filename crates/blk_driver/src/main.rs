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
use smos_sddf::blk_config::{BlkConnectionResource, BlkDriverConfig, SDDF_BLK_MAGIC};
use smos_sddf::notification_channel::{BidirectionalChannel, NotificationChannel, PPCForbidden};
use smos_sddf::queue::{ActiveQueue, FreeQueue, SerialQueue, Queue};
use smos_sddf::sddf_bindings::{init, sddf_event_loop, sddf_set_channel};
use smos_sddf::sddf_channel::sDDFChannel;
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::event::{smos_serv_cleanup, smos_serv_decode_invocation, smos_serv_replyrecv};
use smos_server::reply::SMOSReply;
use smos_server::syscalls::{
    sDDFChannelRegisterBidirectional, sDDFQueueRegister, ConnOpen, SMOS_Invocation, sDDFProvideDataRegion
};

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;
const REGS_BASE: *const u32 = 0xB000000 as *const u32;

const VIRT_REQ: *const u8 = 0xC000000 as *const u8;
const VIRT_RESP: *const u8 = 0xC001000 as *const u8;

const VIRT_DATA: *const u8 = 0xD000000 as *const u8;
const REQ_BUF: *const u8 = 0xD200000 as *const u8;
const VIRTIO_BUF: *const u8 = 0xD201000 as *const u8;

const QUEUE_SIZE: usize = 4096;

const BLK_REGS_PADDR: usize = 0xa003a00 as usize;
const BLK_OFFSET: usize = 
    BLK_REGS_PADDR - ROUND_DOWN(BLK_REGS_PADDR as usize, sel4_sys::seL4_PageBits as usize);
const BLK_IRQ_NUMBER: usize = 77;

extern "C" {
    static mut device_resources: DeviceResources;
    static mut config: BlkDriverConfig;
}

#[derive(Copy, Clone)]
struct sDDFBlkDriverClient {
    id: usize,
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
    storage_info: Option<DMARegion>,
    req_queue: Option<Queue<SerialQueue>>,
    resp_queue: Option<Queue<SerialQueue>>,
    channel: Option<NotificationChannel<BidirectionalChannel, PPCForbidden>>,
}

impl sDDFClient for sDDFBlkDriverClient {
    fn new (
        id: usize,
        registration_handle: LocalHandle<ConnRegistrationHandle>,
    ) -> sDDFBlkDriverClient {
        return sDDFBlkDriverClient {
            id: id,
            conn_registration_hndl: registration_handle,
            storage_info: None,
            req_queue: None,
            resp_queue: None,
            channel: None,
        };
    }

    fn get_id(&self) -> usize {
        self.id
    }

    fn initialized(&self) -> bool {
        self.storage_info.is_some() && self.req_queue.is_some() && self.resp_queue.is_some()
    }
}

fn handle_virt_register (
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    cspace: &mut SMOSUserCSpace,
    client: &mut sDDFBlkDriverClient,
    virt_reg_status: &mut VirtRegistration,
    args: &sDDFChannelRegisterBidirectional,
) -> Result<SMOSReply, InvocationError> {

    //@chirag any error checks

    /* Try and open the channel */
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

fn handle_queue_register (
    rs_conn: &RootServerConnection,
    client: &mut sDDFBlkDriverClient,
    args: &sDDFQueueRegister,
) -> Result<SMOSReply, InvocationError> {
    if args.size != QUEUE_SIZE {
        return Err(InvocationError::InvalidArguments);
    }

    match args.queue_type {
        QueueType::Request => {
            if client.req_queue.is_some() {
                return Err(InvocationError::InvalidArguments);
            }

            client.req_queue = Some(Queue::<SerialQueue>::open(
                rs_conn,
                VIRT_REQ as usize,
                args.size,
                HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
            )?);
        }
        QueueType::Response => {
            if client.resp_queue.is_some() {
                return Err(InvocationError::InvalidArguments);
            }

            client.resp_queue = Some(Queue::<SerialQueue>::open(
                rs_conn,
                VIRT_RESP as usize,
                args.size,
                HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
            )?);
        }
        _ => return Err(InvocationError::InvalidArguments), // Should never actually hit this case
    }

    return Ok(SMOSReply::sDDFQueueRegister);
}

fn handle_provide_data_region (
    rs_conn: &RootServerConnection,
    client: &mut sDDFBlkDriverClient,
    args: &sDDFProvideDataRegion,
) -> Result<SMOSReply, InvocationError> {
    if client.req_queue.is_none() || client.resp_queue.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    client.storage_info = Some(DMARegion::open(
        rs_conn,
        HandleOrHandleCap::<ObjectHandle>::from(args.hndl_cap),
        VIRT_DATA as usize,
        args.size,
    )?);

    return Ok(SMOSReply::sDDFProvideDataRegion);
}

fn check_done(clients: [Option<sDDFBlkDriverClient>; 2]) -> bool {
    if clients[0].is_none() {
        return false;
    }

    if clients[0].as_ref().unwrap().initialized() {
        return true;
    }

    return false;
}

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) -> sel4::Result<Never> {
    sel4::debug_println!("Hello, I am blk0!!!");

    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 1);

    /* Register as a server */
    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let listen_conn = rs_conn
        .conn_publish::<sDDFConnection>(
            NTFN_BUFFER,
            &cspace.to_absolute_cptr(ep_cptr),
            args[0]
        )
        .expect("Failed to publish as a server");

    /* Map in the blk registers */
    let device_region = DeviceRegion::new (
        &rs_conn,
        REGS_BASE as usize,
        0x1000,
        ROUND_DOWN (
            BLK_REGS_PADDR as usize,
            sel4_sys::seL4_PageBits.try_into().unwrap(),
        ),
    )
    .expect("Failed to create blk device region");

    /* Register for the IRQ */
    let irq_channel = IrqChannel::new (
        &rs_conn,
        &mut cspace,
        &listen_conn.hndl(),
        BLK_IRQ_NUMBER,
    )
    .expect("Failed to register for blk irq");

    /* Map in the requests buffer */
    let requests_buffer = DMARegion::new (
        &rs_conn,
        &mut cspace,
        REQ_BUF as usize,
        0x1000,
        false,
    )
    .expect("Failed to create requests buffer DMA region");

    let virt_char_buf = DMARegion::new(
        &rs_conn,
        &mut cspace,
        VIRTIO_BUF as usize,
        0x1000,
        false,
    )
    .expect("Failed to allocate virtIO buf");

    /* Allocate a reply cap */
    let reply_cptr = cspace.alloc_slot().expect("Could not allocate a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    /* Allocate a cap receive slot */
    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate a slot");
    let recv_slot = cspace.to_absolute_cptr(recv_slot_inner);

    let virts = sddf_driver_pre_init::<sDDFConnection, sDDFBlkDriverClient, 2> (
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

    sddf_set_channel (
        irq_channel.bit as usize,
        None,
        sDDFChannel::IrqChannel(irq_channel),
    )
    .expect("Failed to set up IRQ channel");

    sddf_set_channel (
        virts[0].unwrap().channel.unwrap().from_bit.unwrap() as usize,
        None,
        sDDFChannel::NotificationChannelBi(virts[0].unwrap().channel.unwrap()),
    )
    .expect("Failed to set up Virt channel");

    unsafe {
        config = BlkDriverConfig {
            magic: SDDF_BLK_MAGIC,
            virt: BlkConnectionResource {
                storage_info: RegionResource {
                    vaddr: virts[0].unwrap().storage_info.unwrap().vaddr,
                    size: virts[0].unwrap().storage_info.unwrap().size,
                },
                req_queue: RegionResource {
                    vaddr: virts[0].unwrap().req_queue.unwrap().vaddr,
                    size: virts[0].unwrap().req_queue.unwrap().size,
                },
                resp_queue: RegionResource {
                    vaddr: virts[0].unwrap().resp_queue.unwrap().vaddr,
                    size: virts[0].unwrap().resp_queue.unwrap().size,
                },
                num_buffers: 1024,
                id: virts[0].unwrap().channel.unwrap().from_bit.unwrap(),
            },
        };

        let mut device_regions: [MaybeUninit<DeviceRegionResource>; DEVICE_MAX_REGIONS] =
            MaybeUninit::uninit().assume_init();
        let mut device_irqs: [MaybeUninit<DeviceIrqResource>; DEVICE_MAX_IRQS] =
            MaybeUninit::uninit().assume_init();

        device_regions[0] = MaybeUninit::new(DeviceRegionResource {
            region: RegionResource {
                vaddr: device_region.vaddr + BLK_OFFSET,
                size: device_region.size,
            },
            io_addr: BLK_REGS_PADDR,
        });
        device_regions[1] = MaybeUninit::new(DeviceRegionResource {
            region: RegionResource {
                vaddr: virt_char_buf.vaddr,
                size: virt_char_buf.size,
            },
            io_addr: virt_char_buf.paddr,
        });
        device_regions[2] = MaybeUninit::new(DeviceRegionResource {
            region: RegionResource {
                vaddr: requests_buffer.vaddr,
                size: requests_buffer.size,
            },
            io_addr: requests_buffer.paddr,
        });

        device_irqs[0] = MaybeUninit::new(DeviceIrqResource {
            id: irq_channel.bit,
        });

        device_resources = DeviceResources {
            magic: SDDF_DEVICE_MAGIC,
            num_regions: 3,
            num_irqs: 1,
            regions: device_regions,
            irqs: device_irqs,
        };
    }

    unsafe { init() };

    sddf_event_loop(listen_conn, reply);
}