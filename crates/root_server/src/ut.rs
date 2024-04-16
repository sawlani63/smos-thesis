use sel4::cap::Untyped;

pub struct UT<'a> {
	// cap: sel4::cap::untyped,
	valid: bool,
	size_bits: u8,
	next: Option<&'a UT<'a>>
}

const N_UNTYPED_LISTS : usize = (sel4_sys::seL4_PageBits - sel4_sys::seL4_EndpointBits + 1) as usize;

pub struct UTTable {
	first_paddr: usize,
	// untypeds: Vec<UT>,
	// free_untypeds: [Vec<UT>; N_UNTYPED_LISTS],
	n_4k_untyped: usize,
	// free_structures: Vec<UT>

	// @alwin: These are almost defintely not meant to be vectors but they are for now.
}

pub struct UTRegion {
	pub start: usize,
	pub end: usize
}

impl UTRegion {
	pub fn new(start: usize, end: usize) -> Self {
		return Self { start: start, end: end };
	}
}

pub fn ut_init(memory: usize, region: UTRegion) -> UT<'static> {
	todo!();
}

pub fn ut_add_untyped_range(paddr: usize, cptr: usize, n_pages: usize, device: bool) {
	todo!();
}

