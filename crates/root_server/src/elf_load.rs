use crate::cspace::{CSpace, CSpaceTrait};
use crate::mapping::map_frame;
use crate::page::PAGE_SIZE_4K;
use crate::ut::UTTable;
use crate::frame_table::FrameTable;
use crate::arith::ROUND_DOWN;

fn rights_from_elf_flags(flags: u32) -> sel4::CapRights {
	let mut builder = sel4::CapRightsBuilder::none();

	// Can read
	if (flags & elf::abi::PF_R != 0 || flags & elf::abi::PF_X != 0 ) {
		builder = builder.read(true);
	}

	// Can write
	if (flags & elf::abi::PF_W != 0) {
		builder = builder.write(true);
	}

	return builder.build();
}

fn load_segment_into_vspace(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable,
							vspace: sel4::cap::VSpace, segment: &elf::segment::ProgramHeader, data: &[u8]) -> Result<(), sel4::Error>{
    let mut pos = 0;
    let mut curr_vaddr = segment.p_vaddr;
    let mut curr_offset = 0;

    while pos < segment.p_memsz {
        let loadee_vaddr = ROUND_DOWN(curr_vaddr.try_into().unwrap(), PAGE_SIZE_4K);
        let frame = frame_table.alloc_frame(cspace, ut_table).ok_or(sel4::Error::NotEnoughMemory)?;

        let loadee_frame = sel4::CPtr::from_bits(cspace.alloc_slot()?.try_into().unwrap()).cast::<sel4::cap_type::UnspecifiedFrame>();
        cspace.root_cnode().relative(loadee_frame).copy(&cspace.root_cnode().relative(frame.get_cap()), sel4::CapRightsBuilder::all().build());

        match map_frame(cspace, ut_table, loadee_frame, vspace, loadee_vaddr, rights_from_elf_flags(segment.p_flags), sel4::VmAttributes::DEFAULT, None) {
            Ok(_) => {},
            Err(e) => match e {
            	// @alwin: check that the overlapping pages have same permissions
                sel4::Error::DeleteFirst => {
                	cspace.delete(loadee_frame.bits().try_into().unwrap());
                	cspace.free_slot(loadee_frame.bits().try_into().unwrap());
                	frame_table.free_frame(frame);
                },
                _ => return Err(e)
            },
        }

        /* Copy over the data */
        let frame_data = frame_table.frame_data(frame);
        let mut frame_offset : usize = 0;

        let leading_zeroes : usize = curr_vaddr % PAGE_SIZE_4K.try_into().unwrap();
        frame_data[frame_offset..leading_zeroes].fill(0);
        frame_offset += leading_zeroes;

        let segment_bytes = PAGE_SIZE_4K - leading_zeroes;
        if (pos < segment.p_filesz) {
        	let file_bytes = usize::min(segment_bytes, (segment.p_filesz - pos).try_into().unwrap());
        	frame_data[frame_offset..frame_offset+file_bytes].copy_from_slice(&data[curr_offset..]);
        	frame_offset += file_bytes;

        	/* Fill in the rest of the frame with zeroes */
        	let trailing_zeroes = PAGE_SIZE_4K - (leading_zeroes + file_bytes);
        	frame_data[frame_offset..PAGE_SIZE_4K].fill(0);
        } else {
        	frame_data.fill(0);
        }

        pos += segment_bytes.try_into().unwrap();
        curr_vaddr += segment_bytes.try_into().unwrap();
        curr_offset += segment_bytes.try_into().unwrap();
    }

    Ok(())
}

pub fn load_elf(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable, vspace: sel4::cap::VSpace, elf: elf::ElfBytes<elf::endian::AnyEndian>) -> Result<(), sel4::Error>{
    for segment in elf.segments().ok_or(sel4::Error::InvalidArgument)?.iter() {
        if segment.p_type != elf::abi::PT_LOAD {
            continue;
        }

        let data = elf.segment_data(&segment).expect("Could not get segment data");
        load_segment_into_vspace(cspace, ut_table, frame_table, vspace, &segment, data)?;
    }

    Ok(())
}