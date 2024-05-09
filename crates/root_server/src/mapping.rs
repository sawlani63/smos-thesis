use sel4::CPtr;

use crate::cspace::{CSpace, CSpaceTrait, MAPPING_SLOTS};
use crate::page::{BIT, PAGE_SIZE_4K};
use crate::ut::UTTable;

fn retype_map_pt(cspace: &CSpace, vspace: sel4::cap::VSpace, vaddr: usize, ut: sel4::cap::Untyped, pt_slot: usize) -> Result<(), sel4::Error> {
	cspace.untyped_retype(&ut, sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::PT), pt_slot)?;
	let pt = sel4::CPtr::from_bits(pt_slot.try_into().unwrap()).cast::<sel4::cap_type::PT>();
	return pt.pt_map(vspace, vaddr, sel4::VmAttributes::DEFAULT);
}

pub fn map_frame(cspace: &mut CSpace, ut_table: &mut UTTable, frame_cap: sel4::cap::UnspecifiedFrame, vspace: sel4::cap::VSpace,
				 vaddr: usize, rights : sel4::CapRights, attributes: sel4::VmAttributes,
				 free_slots : Option<[usize; MAPPING_SLOTS]>) -> Result<usize, sel4::Error> {

	let mut err : Result<(), sel4::Error> = frame_cap.frame_map(vspace, vaddr, rights.clone(), attributes);
	let mut i = 0;
	let mut used : usize = 0;
	while i < MAPPING_SLOTS && err.is_err_and(|err| err == sel4::Error::FailedLookup) {
		let (_, ut) = ut_table.alloc_4k_untyped()?;

		let slot = {
			if let Some(free_slots_internal) = free_slots {
				used |= BIT(i);
				Ok(free_slots_internal[i])
			} else {
				cspace.alloc_slot()
			}
		}?;

		if slot == sel4_sys::seL4_RootCNodeCapSlots::seL4_CapNull.try_into().unwrap() {
			return Err(sel4::Error::InvalidCapability);
		}

		err = retype_map_pt(cspace, vspace, vaddr, ut.get_cap(), slot);

		if err.is_ok() {
			err = frame_cap.frame_map(vspace, vaddr, rights.clone(), attributes);
		}

		i += 1;
	}

	if err.is_ok() {
		return Ok(used);
	}
	return Err(err.err().unwrap());
}

const DEVICE_START: usize = 0xB0000000;
// @alwin: Maybe chuck this somewhere else instead so we don't have to use unsafe ?
static mut DEVICE_VIRT: usize = DEVICE_START;

pub fn map_device(cspace: &mut CSpace, ut_table: &mut UTTable, paddr: usize, size: usize)
	   -> Result<usize, sel4::Error> {

	// All of these unsafes are okay because the root server is single threaded and DEVICE_VIRT
	// is only changed in this function.
	let vstart = unsafe {
		DEVICE_VIRT
	};

	for curr in (paddr..paddr+size).step_by(PAGE_SIZE_4K) {
		let ut = ut_table.alloc_4k_device(curr)?;
		let frame_cptr = cspace.alloc_slot()?;
		cspace.untyped_retype(&ut.get_cap(), sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage), frame_cptr)?;
		let frame = CPtr::from_bits(frame_cptr.try_into().unwrap()).cast::<sel4::cap_type::SmallPage>();
		unsafe {
			map_frame(cspace, ut_table, frame.cast(), sel4::init_thread::slot::VSPACE.cap(), DEVICE_VIRT,
				  	  sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::NONE, None)?;
			DEVICE_VIRT += PAGE_SIZE_4K;
		}
	}

	return Ok(vstart);
}