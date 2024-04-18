use core::mem::size_of;

use sel4::cap::Untyped;
use sel4::CPtr;

use crate::page::PAGE_SIZE_4K;

#[derive(Copy, Clone)]
pub struct UT {
	pub cap: sel4::cap::Untyped,
	valid: bool,
	size_bits: u8,
	next: Option<*mut UT>
}

const N_UNTYPED_LISTS : usize = (sel4_sys::seL4_PageBits - sel4_sys::seL4_EndpointBits + 1) as usize;

pub struct UTTable {
	first_paddr: usize,
	untypeds: Option<*mut UT>,
	free_untypeds: [Option<*mut UT>; N_UNTYPED_LISTS],
	n_4k_untyped: usize,
	free_structures: Option<*mut UT>
}

pub fn push(head: &mut Option<*mut UT>, new: &mut UT) {
	new.next = *head;
	*head = Some(new as *mut UT);
}

const fn SIZE_BITS_TO_INDEX(x: usize) -> usize {
	x - sel4_sys::seL4_EndpointBits as usize
}

impl UTTable {
	pub fn alloc_4k_untyped(self: &Self) -> Result<(usize, UT), sel4::Error> {
		todo!();
	}

	pub fn new(memory: usize, region: UTRegion) -> UTTable {
		UTTable { first_paddr: region.start, untypeds: Some(memory as *mut UT),
				  free_untypeds: [None; N_UNTYPED_LISTS], n_4k_untyped: 0, free_structures: None}
	}

	fn paddr_to_ut(self: &Self, paddr: usize) -> &mut UT {
		return unsafe{ &mut *self.untypeds.unwrap().wrapping_add((paddr - self.first_paddr) / PAGE_SIZE_4K * size_of::<UT>()) };
	}

	pub fn add_untyped_range(self: &Self, paddr: usize, cptr: usize, n_pages: usize, device: bool) {
		let mut list = &mut self.free_untypeds[SIZE_BITS_TO_INDEX(sel4_sys::seL4_PageBits as usize) as usize];
		for i in 0..n_pages {
			let node = self.paddr_to_ut(paddr + (i * PAGE_SIZE_4K));
			node.cap = CPtr::from_bits(cptr.try_into().unwrap()).cast::<sel4::cap_type::Untyped>();
			node.valid = true;
			if !device {
				node.size_bits = sel4_sys::seL4_PageBits.try_into().unwrap();
				push(&mut list, &mut node);
				self.n_4k_untyped += 1;
			}
		}
	}
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

pub fn ut_add_untyped_range(paddr: usize, cptr: usize, n_pages: usize, device: bool) {
	todo!();
}
