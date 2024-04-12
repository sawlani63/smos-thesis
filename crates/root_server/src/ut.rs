pub struct UT<'a> {
	// cap: sel4::cap::untyped,
	valid: bool,
	size_bits: u8,
	next: Option<&'a UT<'a>>
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
