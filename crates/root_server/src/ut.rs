use core::mem::size_of;

use sel4::cap::Untyped;
use sel4::CPtr;

use crate::page::PAGE_SIZE_4K;

#[derive(Copy, Clone)]
#[repr(C)]
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

pub fn push(head: Option<*mut UT>, new: *mut UT) -> Option<*mut UT> {
	unsafe {
		(*new).next = head;
	}

	Some(new)
}

pub fn pop(head: Option<*mut UT>) -> Result<(Option<*mut UT>, *const UT), sel4::Error> {
	/* Check that the head is not null */
	let popped = head.ok_or(sel4::Error::NotEnoughMemory)?;
	unsafe {
		return Ok( ((*popped).next, popped) )
	}
}

const fn SIZE_BITS_TO_INDEX(x: usize) -> usize {
	x - sel4_sys::seL4_EndpointBits as usize
}

impl UTTable {
	pub fn alloc_4k_untyped(self: &mut Self) -> Result<(usize, UT), sel4::Error> {
		// @alwin: This should return an error
		let list = self.free_untypeds[SIZE_BITS_TO_INDEX(sel4_sys::seL4_PageBits.try_into().unwrap())];

		let res = pop(list)?;
		self.free_untypeds[SIZE_BITS_TO_INDEX(sel4_sys::seL4_PageBits.try_into().unwrap())] = res.0;

		unsafe {
			return Ok((self.ut_to_paddr(res.1), *(res.1).clone()));
		}
	}

	pub fn alloc_4k_device(self: &mut Self, paddr: usize) -> Result<UT, sel4::Error> {
		let ut = self.paddr_to_ut(paddr);
		unsafe {
			if !(*ut).valid {
				return Err(sel4::Error::InvalidArgument);
			}
			return Ok((*ut).clone());
		}
	}

	pub fn new(memory: usize, region: UTRegion) -> UTTable {
		UTTable { first_paddr: region.start, untypeds: Some(memory as *mut UT),
				  free_untypeds: [None; N_UNTYPED_LISTS], n_4k_untyped: 0, free_structures: None}
	}

	fn paddr_to_ut(self: &Self, paddr: usize) -> *mut UT {
		return self.untypeds.unwrap().wrapping_add((paddr - self.first_paddr) / PAGE_SIZE_4K);
	}

	fn ut_to_paddr(self: &Self, ut: *const UT) -> usize {
		(unsafe { <isize as TryInto<usize>>::try_into(ut.offset_from(self.untypeds.unwrap())).unwrap() }) * PAGE_SIZE_4K + self.first_paddr
	}

	pub fn add_untyped_range(self: &mut Self, paddr: usize, mut cptr: usize, n_pages: usize, device: bool) {
		for i in 0..n_pages {
			let node = self.paddr_to_ut(paddr + (i * PAGE_SIZE_4K));
			unsafe {
				(*node).cap = CPtr::from_bits(cptr.try_into().unwrap()).cast::<sel4::cap_type::Untyped>();
				(*node).valid = true;
			}
			cptr += 1;
			if !device {
				unsafe {(*node).size_bits = sel4_sys::seL4_PageBits.try_into().unwrap();}
				let list = self.free_untypeds[SIZE_BITS_TO_INDEX(sel4_sys::seL4_PageBits as usize)];
				self.free_untypeds[SIZE_BITS_TO_INDEX(sel4_sys::seL4_PageBits as usize)] = push(list, node);
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