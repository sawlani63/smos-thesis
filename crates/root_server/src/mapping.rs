use crate::cspace::{CSpace, MAPPING_SLOTS};
use crate::page::BIT;
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
		let (paddr, ut) = ut_table.alloc_4k_untyped()?;

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

		err = retype_map_pt(cspace, vspace, vaddr, ut.cap, slot);

		if err.is_ok() {
			err = frame_cap.frame_map(vspace, vaddr, rights.clone(), attributes);
		}
	}

	if err.is_ok() {
		return Ok(used);
	}
	return Err(err.err().unwrap());
}