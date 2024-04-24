use sel4::ObjectBlueprint;

use crate::err_rs;
use crate::cspace::CSpace;
use crate::ut::{UTTable, UTWrapper};

pub const fn ALIGN_DOWN(x : usize, n : usize) -> usize {
	return x & !(n - 1);
}

pub const fn ALIGN_UP(x: usize, n: usize) -> usize {
	(x + n - 1) & !(n - 1)
}

const fn BIT(n : usize) -> usize {
	1 << n
}

pub const fn MASK(n: usize) -> usize {
	BIT(n) - 1
}

pub fn alloc_retype(cspace: &mut CSpace, ut_table: &mut UTTable, blueprint: ObjectBlueprint) -> Result<(usize, UTWrapper), sel4::Error> {
	let ut = ut_table.alloc(cspace, blueprint.physical_size_bits()).map_err(|_| {
		err_rs!("No memory for object of size {}", blueprint.physical_size_bits());
		sel4::Error::NotEnoughMemory
	})?;

	let cptr = cspace.alloc_slot().map_err(|_| {
		err_rs!("Failed to allocate slot");
		ut_table.free(ut);
		sel4::Error::InvalidCapability
	})?;

	cspace.untyped_retype(&ut.get_cap(), blueprint, cptr).map_err(|_| {
		err_rs!("Failed to retype untyped");
		ut_table.free(ut);
		cspace.free_slot(cptr);
		sel4::Error::IllegalOperation
	})?;

	return Ok((cptr, ut));
}
