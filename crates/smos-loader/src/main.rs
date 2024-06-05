#![no_std]
#![no_main]

use smos_runtime::{smos_declare_main, Never}
use smos_cspace::SMOSUserCSpace;
use smos_common::connection::{RootServerConnection}

const SERVER_NAME = "BOOT_FS";
const FILE_NAME = "test_app"

const shared_buffer_region: *mut u8 = 0xA0002000;
const elf_base: *const u8 = 0xB000000;

#[smos_declare_main]
// @alwin: How the heck am I adding argc and argv
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) -> sel4::Result<Never> {
    /* Expected arguments
            1. The name of the file server where this file is expected to be
            2. File name to run
    */

    /* Check that argc == 1*/

    sel4::debug_println("Hello world! I am loading the executable ...")

    /* Set up connection to file server */
    let fs_ep_slot = cspace.alloc_slot();
    let fs_conn = rs_conn.conn_create(SERVER_NAME,  fs_ep_slot).expect("Failed to connect to specified server") // @alwin: This should come from args

    let shared_buffer_win_hndl = rs_conn.window_create(shared_buffer_region, 4096, None);

    let shared_buf_obj_cap = cspace.alloc_slot();
    let shared_buf_obj = rs_conn.obj_create(None, 4096, sel4::CapRights::all(), Some(cspace.to_absolute_cptr(shared_buf_obj_hndl_cap)));

    rs_conn.view(shared_buffer_win_hndl, shared_buf_obj, 0, 0, 4096, sel4::CapRights::all())

    fs_conn.conn_open(shared_buf_obj, shared_buffer_region);

    /* How should conn_open() work */


    // User passes in a window with a view inside 
    fs_conn.conn_open();
    fs_conn.test_simple()

    /* Open the file*/
    let elf_base: *const u8 = ;
    let file_hndl = fs_conn.obj_open(FILE_NAME /*, attributes */ );
    let file_size = fs_conn.obj_stat().size;

    /* Create a window */
    let elf_window_hdnl_cap = cspace.alloc_slot();
    rs_conn.window_create(elf_base, file_size, Some(cspace.to_absolute_cptr(elf_window_hdnl_cap)));

    /* Map the object into the window */
    fs_conn.view(elf_window_hdnl_cap, file_hndl, 0, 0, file_size, /* attributes */);

    /* Load the ELF file */
    let elf_bytes = unsafe { core::slice::from_raw_parts(elf_base, file_size) };
    let elf = ElfBytes::<elf::endian::AnyEndian>::minimal_parse(elf_bytes).expect("Invalid elf file")

    /* @alwin: The C impl does some syscall table stuff */
    for segment in elf.segments().execpt("Couldn't get segments").iter() {
        if segment.p_type != elf::ABI::PT_LOAD && segment.p_type != elf::abi::PT_TLS {
            continue;
        }

        if (segment.p_filesz > segment.p_memsz) {
            sel4::debug_println!("Invalid ELF file");
            return;
        }

        bool readable = segment & elf::abi::PF_R != 0 || segment & elf::abi::PF_X != 0;
        bool witeable = segment & elf::abi::PF_W != 0;

        let total_size = p.p_memsz + (p.p_vaddr % PAGE_SIZE_4K) + (PAGE_SIZE_4K - ((vaddr + p.p_memsz) % PAGE_SIZE_4K))

        let segment_hndl = rs_conn.window_create(ROUND_DOWN(segment.p_vaddr, PAGE_SIZE_4K), total_size, None);
        let mem_obj_hndl = rs_conn.obj_create(/* type? */, total_size, None);
        let view_hndl = rs_conn.vew(segment_hndl, mem_obj_hndl, 0, 0, total_size, /* attributes */);

        let segment = unsafe {core::slice::from_raw_parts(segment.p_vaddr, p.p_memsz)}

        segment[..segment.p_filesz].copy_from_slice(elf.segment_data(&segment).expect("Could not get segment data")[..segment.p_filesz]);

        rs_conn.unview(view_hndl);
        /* @alwin: view again with the correct permissions */
    }

    /* @alwin: what to do with the stack and heap? I think just when smos_load_complete is invoked, reset the stack and heap regions in the root server */

    /* Get the ELF entrypoint */
    let start_vaddr = elf.ehdr.e_entry;

    rs_conn.window_destroy(elf_window_hdnl_cap);

    fs_conn.obj_close(file_hndl);

    rs_conn.conn_destroy(fs_conn);

    rs_conn.load_complete(elf)
}
