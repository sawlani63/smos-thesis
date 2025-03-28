use crate::cspace::{CSpace, CSpaceTrait};
use crate::frame_table::FrameTable;
use crate::mapping::map_frame;
use crate::object::{AnonymousMemoryObject, MAX_OBJ_SIZE};
use crate::page::PAGE_SIZE_4K;
use crate::ut::UTTable;
use crate::view::View;
use crate::window::Window;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use smos_common::obj_attributes::ObjAttributes;
use smos_common::util::ROUND_DOWN;

fn rights_from_elf_flags(flags: u32) -> sel4::CapRights {
    let mut builder = sel4::CapRightsBuilder::none();

    // Can read
    if flags & elf::abi::PF_R != 0 || flags & elf::abi::PF_X != 0 {
        builder = builder.read(true);
    }

    // Can write
    if flags & elf::abi::PF_W != 0 {
        builder = builder.write(true);
    }

    return builder.build();
}

fn overlapping_window(
    windows: &Vec<Rc<RefCell<Window>>>,
    start: usize,
    size: usize,
) -> Option<Rc<RefCell<Window>>> {
    for window in windows {
        if (start >= window.borrow().start && start < window.borrow().start + window.borrow().size)
            || (start + size >= window.borrow().start
                && start + size < window.borrow().start + window.borrow().size)
        {
            return Some(window.clone());
        }
    }

    return None;
}

fn handle_overlapping_segment(
    window: Rc<RefCell<Window>>,
    segment: &elf::segment::ProgramHeader,
    _data: &[u8],
) {
    log_rs!("Dealing with an overlapping region");
    // @alwin: We should make sure no segments actually overlap in terms of their precise virtual addresses.
    // This is a bit annoying because, windows are all page-aligned. For us to check that the elf segments
    // themselves don't overlap, we will need to keep some extra book-keeping and have more segments.

    // Check that the rights of this segment are the same as the existing window that overlaps with it
    if rights_from_elf_flags(segment.p_flags)
        != window.borrow().bound_view.as_ref().unwrap().borrow().rights
    {
        todo!();
    }

    /* If the segment doesn't have any data, we don't need to do anything. */
    if segment.p_filesz == 0 {
        return;
    }

    /* If the segment does contain data, we need to copy it into the right part of the frame,
    making sure we don't overwrite anything that has been written for the other segment */
    // @alwin: Cross this bridge if/when we get there :p
    todo!();
}

fn load_segment_into_vspace(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
    frame_table: &mut FrameTable,
    windows: &mut Vec<Rc<RefCell<Window>>>,
    vspace: sel4::cap::VSpace,
    segment: &elf::segment::ProgramHeader,
    data: &[u8],
) -> Result<(), sel4::Error> {
    let mut pos: usize = 0;
    let mut curr_vaddr: usize = segment.p_vaddr.try_into().unwrap();
    let mut curr_offset: usize = 0;
    let total_size = segment.p_memsz
        + (segment.p_vaddr % PAGE_SIZE_4K as u64)
        + (PAGE_SIZE_4K as u64 - ((segment.p_vaddr + segment.p_memsz) % PAGE_SIZE_4K as u64));

    log_rs!(
        "Loading segment of type {} => 0x{:x} - 0x{:x}",
        segment.p_type,
        segment.p_vaddr,
        segment.p_vaddr + segment.p_memsz
    );

    match overlapping_window(
        windows,
        segment.p_vaddr.try_into().unwrap(),
        segment.p_memsz.try_into().unwrap(),
    ) {
        Some(win) => {
            handle_overlapping_segment(win, segment, data);
            return Ok(());
        }
        None => {}
    };

    if total_size as usize > MAX_OBJ_SIZE {
        err_rs!("@alwin: Deal with the case where segment size is to large");
        todo!();
    }

    /* Create a window corresponding to this segment */
    let window = Rc::new(RefCell::new(Window {
        start: ROUND_DOWN(
            segment.p_vaddr.try_into().unwrap(),
            sel4_sys::seL4_PageBits.try_into().unwrap(),
        ),
        size: total_size.try_into().unwrap(),
        bound_view: None,
    }));

    /* Create a memory object corresponding to this segment */
    let object = Rc::new(RefCell::new(AnonymousMemoryObject::new(
        total_size.try_into().unwrap(),
        rights_from_elf_flags(segment.p_flags),
        ObjAttributes::DEFAULT,
    )));

    /* Create a view corresponding to this segment */
    let view = Rc::new(RefCell::new(View::new(
        window.clone(),
        Some(object.clone()),
        None,
        rights_from_elf_flags(segment.p_flags),
        0,
        0,
    )));

    window.borrow_mut().bound_view = Some(view.clone());
    object.borrow_mut().associated_views.push(view.clone());

    /* This counter keeps track of the index into the memory object */
    let mut i: usize = 0;

    while pos < segment.p_memsz.try_into().unwrap() {
        let loadee_vaddr = ROUND_DOWN(
            curr_vaddr.try_into().unwrap(),
            sel4_sys::seL4_PageBits.try_into().unwrap(),
        );

        let frame_ref = frame_table
            .alloc_frame(cspace, ut_table)
            .ok_or(sel4::Error::NotEnoughMemory)?;
        let orig_frame_cap = frame_table.frame_from_ref(frame_ref).get_cap();
        object
            .borrow_mut()
            .insert_frame_at(i * PAGE_SIZE_4K, (orig_frame_cap, frame_ref))
            .expect("Failed to insert frame into object");

        let loadee_frame = sel4::CPtr::from_bits(cspace.alloc_slot()?.try_into().unwrap())
            .cast::<sel4::cap_type::UnspecifiedPage>();
        cspace
            .root_cnode()
            .absolute_cptr(loadee_frame)
            .copy(
                &cspace
                    .root_cnode()
                    .absolute_cptr(frame_table.frame_from_ref(frame_ref).get_cap()),
                sel4::CapRightsBuilder::all().build(),
            )
            .expect("Failed to copy frame capability into loadee frame cslot");
        view.borrow_mut()
            .insert_cap_at(i * PAGE_SIZE_4K, loadee_frame.cast())
            .expect("Failed to insert into view");

        match map_frame(
            cspace,
            ut_table,
            loadee_frame,
            vspace,
            loadee_vaddr,
            view.borrow().rights.clone(),
            sel4::VmAttributes::DEFAULT,
            None,
        ) {
            Ok(_) => {}
            Err(e) => match e {
                // @alwin: This won't get triggered as you would expect because overmapping does
                // not return an error on aarch64.
                sel4::Error::DeleteFirst => {
                    cspace
                        .delete(loadee_frame.bits().try_into().unwrap())
                        .unwrap();
                    cspace.free_slot(loadee_frame.bits().try_into().unwrap());
                    frame_table.free_frame(frame_ref);
                }
                _ => return Err(e),
            },
        }

        /* Copy over the data */
        let frame_data = frame_table.frame_data(frame_ref);
        let mut frame_offset: usize = 0;

        let leading_zeroes: usize = curr_vaddr as usize % PAGE_SIZE_4K;
        frame_data[frame_offset..leading_zeroes].fill(0);
        frame_offset += leading_zeroes;

        let segment_bytes = PAGE_SIZE_4K - leading_zeroes;
        if pos < segment.p_filesz.try_into().unwrap() {
            /* Copy data from the ELF into the region */
            let file_bytes = usize::min(segment_bytes, segment.p_filesz as usize - pos);
            frame_data[frame_offset..frame_offset + file_bytes]
                .copy_from_slice(&data[curr_offset..curr_offset + file_bytes]);
            frame_offset += file_bytes;

            /* Fill in the rest of the frame with zeroes */
            frame_data[frame_offset..PAGE_SIZE_4K].fill(0);
        } else {
            frame_data.fill(0);
        }

        pos += segment_bytes;
        curr_vaddr += segment_bytes;
        curr_offset += segment_bytes;
        i += 1
    }

    /* Add the window to the vector */
    windows.push(window);

    Ok(())
}

pub fn load_elf(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
    frame_table: &mut FrameTable,
    vspace: sel4::cap::VSpace,
    elf: &elf::ElfBytes<elf::endian::AnyEndian>,
) -> Result<Vec<Rc<RefCell<Window>>>, sel4::Error> {
    let mut windows = Vec::<Rc<RefCell<Window>>>::new();

    for segment in elf.segments().ok_or(sel4::Error::InvalidArgument)?.iter() {
        if segment.p_type != elf::abi::PT_LOAD && segment.p_type != elf::abi::PT_TLS {
            continue;
        }

        let data = elf
            .segment_data(&segment)
            .expect("Could not get segment data");
        load_segment_into_vspace(
            cspace,
            ut_table,
            frame_table,
            &mut windows,
            vspace,
            &segment,
            data,
        )?;
    }

    Ok(windows)
}
