#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec::Vec;
use byteorder::ByteOrder;
use elf::endian::LittleEndian;
use smos_common::connection::{ObjectServerConnection, RootServerConnection};
use smos_common::local_handle::{
    HandleOrHandleCap, LocalHandle, ObjectHandle, ViewHandle, WindowHandle,
};
use smos_common::obj_attributes::ObjAttributes;
use smos_common::string::copy_terminated_rust_string_to_buffer;
use smos_common::syscall::{ObjectServerInterface, RootServerInterface};
use smos_common::util::{ROUND_DOWN, ROUND_UP};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::{smos_declare_main, Never};

use elf::ElfBytes;

const shared_buffer_region: *mut u8 = 0xA0002000 as *mut u8;
const elf_base: *const u8 = 0xB000000 as *const u8;

const PAGE_SIZE_4K: u64 = 4096;

fn rights_from_elf_flags(flags: u32) -> sel4::CapRights {
    let mut builder = sel4::CapRightsBuilder::none();

    // Can read
    if (flags & elf::abi::PF_R != 0 || flags & elf::abi::PF_X != 0) {
        builder = builder.read(true);
    }

    // Can write
    if (flags & elf::abi::PF_W != 0) {
        builder = builder.write(true);
    }

    return builder.build();
}

struct SegmentData {
    win_hndl: HandleOrHandleCap<WindowHandle>,
    obj_hndl: HandleOrHandleCap<ObjectHandle>,
    view_hndl: LocalHandle<ViewHandle>,
    size: usize,
    rights: sel4::CapRights,
}

#[smos_declare_main]
// @alwin: How the heck am I adding argc and argv
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) -> sel4::Result<Never> {
    /* Expected arguments
            0. File name to run
            1. The name of the file server where this file is expected to be
    */

    /* Check that argc == 2 */
    let args: Vec<&str> = smos_runtime::args::args().collect();
    assert!(args.len() >= 2);
    sel4::debug_println!(
        "Hello world! I am loading the executable {} from {}",
        args[0],
        args[1]
    );

    /* Set up connection to file server */
    let fs_ep_slot = cspace
        .alloc_slot()
        .expect("Failed to allocate slot for FS connection");
    let mut fs_conn = rs_conn
        .conn_create::<ObjectServerConnection>(&cspace.to_absolute_cptr(fs_ep_slot), args[1])
        .expect("Failed to connect to specified server"); // @alwin: This should come from args

    /* Set up shared buffer with FS */
    let shared_buffer_win_hndl = rs_conn
        .window_create(shared_buffer_region as usize, 4096, None)
        .expect("Failed to create window for shared buffer");
    let shared_buf_obj_hndl_cap = cspace
        .alloc_slot()
        .expect("Failed to allocate slot for shared buffer");
    let shared_buf_obj = rs_conn
        .obj_create(
            None,
            4096,
            sel4::CapRights::all(),
            ObjAttributes::DEFAULT,
            Some(cspace.to_absolute_cptr(shared_buf_obj_hndl_cap)),
        )
        .expect("Failed to create shared buffer object");
    let shared_buf_view = rs_conn
        .view(
            &shared_buffer_win_hndl,
            &shared_buf_obj,
            0,
            0,
            4096,
            sel4::CapRights::all(),
        )
        .expect("Failed to map shared buffer object");

    /* Open connection to file server */
    fs_conn.conn_open(Some((shared_buf_obj.clone(), (shared_buffer_region, 4096))));

    /* Open the ELF file*/
    let file_hndl = fs_conn
        .obj_open(args[0], sel4::CapRights::read_only(), None)
        .expect("Failed to open the file");
    let file_size = fs_conn
        .obj_stat(&file_hndl)
        .expect("Failed to get stat")
        .size;

    /* Create a window */
    let elf_window_hndl_slot = cspace
        .alloc_slot()
        .expect("Failed to alloc slot for elf window");
    let elf_window_hndl_cap = rs_conn
        .window_create(
            elf_base as usize,
            ROUND_UP(file_size, sel4_sys::seL4_PageBits as usize),
            Some(cspace.to_absolute_cptr(elf_window_hndl_slot)),
        )
        .expect("Failed to create window for ELF file");

    /* Map the ELF file into the window */
    let elf_view = fs_conn
        .view(
            &elf_window_hndl_cap,
            &file_hndl,
            0,
            0,
            file_size,
            sel4::CapRights::read_only(),
        )
        .expect("Failed to view");

    /* Load the ELF file */
    let elf_bytes = unsafe { core::slice::from_raw_parts(elf_base, file_size) };
    let elf =
        ElfBytes::<elf::endian::AnyEndian>::minimal_parse(elf_bytes).expect("Invalid elf file");

    let mut segment_mappings: Vec<SegmentData> = Vec::new();
    /* @alwin: The C impl does some syscall table stuff */
    for segment in elf.segments().expect("Couldn't get segments").iter() {
        if segment.p_type != elf::abi::PT_LOAD && segment.p_type != elf::abi::PT_TLS {
            continue;
        }

        if (segment.p_filesz > segment.p_memsz) {
            panic!("Invalid ELF file");
        }

        let readable =
            segment.p_flags & elf::abi::PF_R != 0 || segment.p_flags & elf::abi::PF_X != 0;
        let witeable = segment.p_flags & elf::abi::PF_W != 0;

        let total_size = segment.p_memsz
            + (segment.p_vaddr % PAGE_SIZE_4K)
            + (PAGE_SIZE_4K - ((segment.p_vaddr + segment.p_memsz) % PAGE_SIZE_4K));

        let segment_hndl = rs_conn.window_create(
            ROUND_DOWN(
                segment.p_vaddr as usize,
                sel4_sys::seL4_PageBits.try_into().unwrap(),
            ),
            total_size as usize,
            None,
        );
        // @alwin: Trying to handle overlapping windows but kind of dodgy. Should probably try and make another window from the next page instead
        // as well as having a more specific DeleteFirst error of some kind. Also be careful with checking perms
        if segment_hndl.is_ok() {
            let mem_obj_hndl = rs_conn
                .obj_create(None, total_size as usize, sel4::CapRights::all(), ObjAttributes::DEFAULT, None)
                .expect("Failed to create object");
            let view_hndl = rs_conn
                .view(
                    &segment_hndl.as_ref().unwrap(),
                    &mem_obj_hndl,
                    0,
                    0,
                    total_size as usize,
                    sel4::CapRights::all(),
                )
                .expect("Failed to create view");
            segment_mappings.push(SegmentData {
                win_hndl: segment_hndl.unwrap(),
                obj_hndl: mem_obj_hndl,
                view_hndl: view_hndl,
                size: total_size as usize,
                rights: rights_from_elf_flags(segment.p_flags),
            })
        }

        let segment_data = unsafe {
            core::slice::from_raw_parts_mut(segment.p_vaddr as *mut u8, segment.p_memsz as usize)
        };
        segment_data[..(segment.p_filesz as usize)].copy_from_slice(
            &elf.segment_data(&segment)
                .expect("Could not get segment data")[..(segment.p_filesz as usize)],
        );
    }

    /* Remap all the views the correct permissions */
    for segment in segment_mappings {
        if segment.rights == sel4::CapRights::all() {
            continue;
        }

        // @alwin: This should probably be a single more efficient operation which downgrades the
        // permissions of an existing view instead.
        rs_conn.unview(segment.view_hndl);
        rs_conn.view(
            &segment.win_hndl,
            &segment.obj_hndl,
            0,
            0,
            segment.size,
            segment.rights,
        );
    }

    /* Create a stack */
    // @alwin: Put this stuff somewhere else
    const STACK_TOP: usize = 0xA0000000;
    const STACK_PAGES: usize = 100;
    let stack_win_hndl = rs_conn
        .window_create(
            STACK_TOP - STACK_PAGES * PAGE_SIZE_4K as usize,
            STACK_PAGES * PAGE_SIZE_4K as usize,
            None,
        )
        .expect("Could not make stack window");
    let stack_obj_hndl = rs_conn
        .obj_create(
            None,
            STACK_PAGES * PAGE_SIZE_4K as usize,
            sel4::CapRights::all(),
            ObjAttributes::DEFAULT,
            None,
        )
        .expect("Could not make stack object");
    let stack_view = rs_conn
        .view(
            &stack_win_hndl,
            &stack_obj_hndl,
            0,
            0,
            STACK_PAGES * PAGE_SIZE_4K as usize,
            sel4::CapRights::all(),
        )
        .expect("Could not make stack view");

    let mut curr_sp = STACK_TOP as *mut u8;
    let mut argv: Vec<u64> = Vec::new();
    let mut envp: Vec<u64> = Vec::new();

    let envp_ptr = {
        /* Write STACK_TOP */
        curr_sp = unsafe { curr_sp.sub(8) };
        byteorder::LittleEndian::write_u64_into(&[0xA0000000], unsafe {
            core::slice::from_raw_parts_mut(curr_sp, 8)
        });
        envp.push(curr_sp as u64);

        /* Write IPC buffer address */
        curr_sp = unsafe { curr_sp.sub(8) };
        byteorder::LittleEndian::write_u64_into(
            &[smos_runtime::env::ipc_buffer() as u64],
            unsafe { core::slice::from_raw_parts_mut(curr_sp, 8) },
        );
        envp.push(curr_sp as u64);

        /* Write the address of the shared buffer between to the RS */
        curr_sp = unsafe { curr_sp.sub(8) };
        byteorder::LittleEndian::write_u64_into(
            &[smos_runtime::env::rs_shared_buf() as u64],
            unsafe { core::slice::from_raw_parts_mut(curr_sp, 8) },
        );
        envp.push(curr_sp as u64);

        /* Add null terminator to envp */
        envp.push(0);

        /* Write envp array to stack */
        curr_sp = unsafe { curr_sp.sub(envp.len() * 8) };
        byteorder::LittleEndian::write_u64_into(&envp, unsafe {
            core::slice::from_raw_parts_mut(curr_sp, envp.len() * 8)
        });

        curr_sp
    };

    let argv_ptr = if args[2..].len() > 0 {
        /* Copy args onto the stack */
        for arg in &args[2..] {
            curr_sp = unsafe { curr_sp.sub(arg.as_bytes().len() + 1) };
            argv.push(curr_sp as u64);
            copy_terminated_rust_string_to_buffer(
                unsafe { core::slice::from_raw_parts_mut(curr_sp, arg.as_bytes().len() + 1) },
                arg,
            );
        }

        /* Pad to word alignment */
        curr_sp = unsafe { curr_sp.sub(curr_sp as usize % 8) };

        /* Write argv array to stack */
        curr_sp = unsafe { curr_sp.sub(argv.len() * 8) };
        byteorder::LittleEndian::write_u64_into(&argv, unsafe {
            core::slice::from_raw_parts_mut(curr_sp, argv.len() * 8)
        });

        curr_sp
    } else {
        core::ptr::null()
    };

    /* Write ptr to envp on the stack */
    curr_sp = unsafe { curr_sp.sub(8) };
    byteorder::LittleEndian::write_u64_into(&[envp_ptr as u64], unsafe {
        core::slice::from_raw_parts_mut(curr_sp, 8)
    });

    /* Write ptr to argv on the stack*/
    curr_sp = unsafe { curr_sp.sub(8) };
    byteorder::LittleEndian::write_u64_into(&[argv_ptr as u64], unsafe {
        core::slice::from_raw_parts_mut(curr_sp, 8)
    });

    /* Write argc to stack */
    curr_sp = unsafe { curr_sp.sub(4) };
    byteorder::LittleEndian::write_u32_into(&[argv.len().try_into().unwrap()], unsafe {
        core::slice::from_raw_parts_mut(curr_sp, 4)
    });

    /* Get the ELF entrypoint */
    let start_vaddr = elf.ehdr.e_entry;

    /* Clean up the ELF file */
    // @alwin: Closing the object might gc any views that are associated with it?
    fs_conn.unview(elf_view);
    fs_conn.obj_close(file_hndl);
    rs_conn.window_destroy(elf_window_hndl_cap);

    /* Clean up the FS connection */
    // @alwin: This conn_close is not really mandatory, as conn_destroy will notify the
    // server, but relying on this can result in race conditions unless the server is higher prio
    // than the client and has a budget/period such that it WILL run before the
    // client can do anything else. In this particular example, if the client calls obj_destroy
    // while the server still has the shared buffer mapped in (it hasn't handled the notification
    // from the RS, the operation will fail.
    fs_conn.conn_close();
    // @alwin: Really only this one should be necessary, as this will result in a ntfn to the server
    rs_conn.conn_destroy(fs_conn);

    /* Clean up the shared buffer */
    rs_conn.unview(shared_buf_view);
    rs_conn.obj_destroy(shared_buf_obj);
    rs_conn.window_destroy(shared_buffer_win_hndl);

    sel4::debug_println!("About to jump to executable at addr {:x}", start_vaddr);

    /* Jump to the real executable */
    rs_conn
        .load_complete(start_vaddr as usize, curr_sp as usize)
        .expect("Failed to complete load");

    unreachable!()
}
