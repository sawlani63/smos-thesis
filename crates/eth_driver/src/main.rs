#![no_std]
#![no_main]

use smos_common::client_connection::ClientConnection;
use smos_common::connection::{ObjectServerConnection, RootServerConnection};
use smos_common::obj_attributes::ObjAttributes;
use smos_cspace::SMOSUserCSpace;
use smos_runtime::{smos_declare_main, Never};
extern crate alloc;
use crate::alloc::string::ToString;
use alloc::vec::Vec;
use smos_common::syscall::{ObjectServerInterface, RootServerInterface};

const ntfn_buffer: *mut u8 = 0xB0000 as *mut u8;
const regs_base: *const u32 = 0xB000000 as *const u32;
const recv_dma_reg: *const u8 = 0xB001000 as *const u8;

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) -> sel4::Result<Never> {
    sel4::debug_println!("Jello, I am eth0!!! Nice to meet u");

    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() == 1);

    /* Register as a server */
    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let listen_conn = rs_conn
        .conn_publish::<ObjectServerConnection>(
            ntfn_buffer,
            &cspace.to_absolute_cptr(ep_cptr),
            args[0],
        )
        .expect("Could not publish as server");

    /* Map in the ethernet registers */
    let reg_win_hndl = rs_conn
        .window_create(regs_base as usize, 4096, None)
        .expect("Failed to create window for eth registers");

    let eth_phys_addr: usize = 0xa003000;
    let eth_obj_hndl = rs_conn
        .obj_create(
            Some(&eth_phys_addr.to_string()),
            0x1000,
            sel4::CapRights::all(),
            ObjAttributes::DEVICE,
            None,
        )
        .expect("Could not create obect for eth registers");

    rs_conn
        .view(
            &reg_win_hndl,
            &eth_obj_hndl,
            0,
            0,
            4096,
            sel4::CapRights::all(),
        )
        .expect("Could not view eth registers");

    let regs = unsafe { regs_base.byte_add(0xe00) };

    /* check magic */
    let magic: u32 = unsafe { *regs };
    sel4::debug_println!("magic is {:x}", magic);

    /* Register for the IRQ  */
    let irq_handler_slot = cspace.alloc_slot().expect("Did not have enough slots");
    rs_conn.irq_register(
        &listen_conn.hndl(),
        79,
        true,
        &cspace.to_absolute_cptr(irq_handler_slot),
    );

    /* Map in the recieve DMA region */
    let dma_win_hndl = rs_conn
        .window_create(recv_dma_reg as usize, 0x2_200_000, None)
        .expect("Failed to create DMA region window");
    let dma_obj_hndl = rs_conn
        .obj_create(
            None,
            0x2_200_000,
            sel4::CapRights::all(),
            ObjAttributes::CONTIGUOUS,
            None,
        )
        .expect("Failed to create DMA object");
    let dma_view_hndl = rs_conn
        .view(
            &dma_win_hndl,
            &dma_obj_hndl,
            0,
            0,
            0x2_200_000,
            sel4::CapRights::all(),
        )
        .expect("Failed to view DMA object");

    let stat = rs_conn
        .obj_stat(&dma_obj_hndl)
        .expect("Failed to stat object");
    sel4::debug_println!(
        "DMA regions is at physical address: {:#x}",
        stat.paddr.unwrap()
    );

    unreachable!()
}
