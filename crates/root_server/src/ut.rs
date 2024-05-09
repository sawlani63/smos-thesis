#![allow(non_snake_case)]

use sel4::CPtr;
use crate::cspace::{CSpace, CSpaceTrait};
use crate::err_rs;
use crate::page::PAGE_SIZE_4K;
use crate::mapping::map_frame;
use core::mem::size_of;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct UT {
	pub cap: sel4::cap::Untyped,
	valid: bool,
	size_bits: u8,
	next: Option<*mut UT>
}

#[derive(Copy, Clone)]
pub struct UTWrapper {
	ut: *mut UT,
}

impl UTWrapper {
	pub fn get_size_bits(self: &Self) -> usize {
		unsafe {
			(*self.ut).size_bits.into()
		}
	}

	pub fn get_cap(self: &Self) -> sel4::cap::Untyped {
		unsafe {
			(*self.ut).cap
		}
	}

	pub unsafe fn inner(self: &Self) -> *const UT {
		return self.ut as *const UT;
	}

	pub unsafe fn inner_mut(self: &Self) -> *mut UT {
		return self.ut;
	}
}

const N_UNTYPED_LISTS : usize = (sel4_sys::seL4_PageBits - sel4_sys::seL4_EndpointBits + 1) as usize;

pub struct UTTable {
	first_paddr: usize,
	untypeds: Option<*mut UT>,
	free_untypeds: [Option<*mut UT>; N_UNTYPED_LISTS],
	n_4k_untyped: usize,
	free_structures: Option<*mut UT>,
	pub next_free_vaddr: usize,
}

pub fn push(head: Option<*mut UT>, new: *mut UT) -> Option<*mut UT> {
	unsafe {
		(*new).next = head;
	}

	Some(new)
}

// This function consumes the wrapper, as it should not be used after being pushed
pub fn push_wrapper(head: Option<*mut UT>, new: UTWrapper) -> Option<*mut UT> {
	unsafe {
		(*new.inner_mut()).next = head;
		Some(new.inner_mut())
	}
}

pub fn pop(head: Option<*mut UT>) -> Result<(Option<*mut UT>, *mut UT), sel4::Error> {
	/* Check that the head is not null */
	let popped = head.ok_or(sel4::Error::NotEnoughMemory)?;
	unsafe {
		return Ok( ((*popped).next, popped) )
	}
}

pub fn pop_mut(head: Option<*mut UT>) -> Result<(Option<*mut UT>, *mut UT), sel4::Error> {
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
	pub fn alloc_4k_untyped(self: &mut Self) -> Result<(usize, UTWrapper), sel4::Error> {
		// @alwin: This should return an error
		let list = self.free_untypeds[SIZE_BITS_TO_INDEX(sel4_sys::seL4_PageBits.try_into().unwrap())];

		let res = pop(list)?;
		self.free_untypeds[SIZE_BITS_TO_INDEX(sel4_sys::seL4_PageBits.try_into().unwrap())] = res.0;

		return Ok((self.ut_to_paddr(res.1), UTWrapper{ ut: res.1 }));
	}

	pub fn map_frame_to_next_free_vaddr(self: &mut Self, cspace: &mut CSpace, frame: sel4::cap::SmallPage)
		   -> Result<usize, sel4::Error> {

		// @alwin: should the UT table store a vspace and use this instead of harcoding it?
		map_frame(cspace, self, frame.cast(), sel4::init_thread::slot::VSPACE.cap(), self.next_free_vaddr,
				  sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None)?;
		let prev = self.next_free_vaddr;
		self.next_free_vaddr += PAGE_SIZE_4K;
		return Ok(prev);
	}

	pub fn ensure_new_structures(self: &mut Self, cspace: &mut CSpace) -> Result<(), sel4::Error> {
		// @alwin: I wonder if this should be abstracted away to some "next" so there isn't unsightly
		// unsafe. Maybe if this will clean some other stuff up?
		if self.free_structures == None || unsafe { (*self.free_structures.unwrap()).next == None } {
			/* We need to allocate more spare UT objects */
			let (_, frame_ut) = self.alloc_4k_untyped()?;

	        /* allocate a slot to retype the frame into */
			let cptr = cspace.alloc_slot()?;

			/* Retype */
			cspace.untyped_retype(&frame_ut.get_cap(),
								  sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage),
								  cptr)?;

			let frame = CPtr::from_bits(cptr.try_into().unwrap()).cast::<sel4::cap_type::SmallPage>();

			// Map this page into our vspace
			let new_uts = self.map_frame_to_next_free_vaddr(cspace, frame)? as *mut UT;

			// Divide this page into however many UTs will fit into it and push these all to the
			// free structures list
			for i in 0..(PAGE_SIZE_4K / size_of::<UT>()) {
				self.free_structures = push(self.free_structures, new_uts.wrapping_add(i));
			}
		}

		Ok(())
	}

	pub fn alloc_4k_device(self: &mut Self, paddr: usize) -> Result<UTWrapper, sel4::Error> {
		let ut = self.paddr_to_ut(paddr);
		unsafe {
			if !(*ut).valid {
				return Err(sel4::Error::InvalidArgument);
			}
			return Ok(UTWrapper{ ut: ut });
		}
	}

	pub fn free(self: &mut Self, ut: UTWrapper) {
		self.free_untypeds[SIZE_BITS_TO_INDEX(ut.get_size_bits())] =
			push_wrapper(self.free_untypeds[SIZE_BITS_TO_INDEX(ut.get_size_bits())], ut)
	}

	pub fn alloc(self: &mut Self, cspace: &mut CSpace, size_bits: usize) -> Result<UTWrapper, sel4::Error> {
		/* Check we can handle the size */
		if size_bits > sel4_sys::seL4_PageBits.try_into().unwrap() {
			err_rs!("UT table can only allocate untypeds <= 4K in size");
			return Err(sel4::Error::InvalidArgument);
		}

		if size_bits < sel4_sys::seL4_EndpointBits.try_into().unwrap() {
			err_rs!("UT Table cannot alloc untyped < {:x} in size", sel4_sys::seL4_EndpointBits);
			return Err(sel4::Error::InvalidArgument)
		}

		if size_bits == sel4_sys::seL4_PageBits.try_into().unwrap() {
			return Ok(self.alloc_4k_untyped()?.1);
		}

		let head = self.free_untypeds[SIZE_BITS_TO_INDEX(size_bits)];
		if head.is_none() {
			let larger = self.alloc(cspace, size_bits + 1)?;

			self.ensure_new_structures(cspace).map_err(|e| {
				self.free(larger);
				e
			})?;

			// Pop the first free structure
			let new1 = match pop_mut(self.free_structures) {
    			Ok((new_head, popped)) => {
    				self.free_structures = new_head;
    				Ok(popped)
    			},
    			Err(e) => {
    				self.free(larger);
    				Err(e)
    			}
    		}?;

    		let cslot1 = cspace.alloc_slot().map_err(|e| {
    			self.free(larger);
    			self.free_structures = push(self.free_structures, new1);
    			e
    		})?;
    		unsafe {
    			(*new1).cap = CPtr::from_bits(cslot1.try_into().unwrap()).cast::<sel4::cap_type::Untyped>();
    			(*new1).size_bits = size_bits.try_into().unwrap();
    		}

    		// Pop the second free structure
			let new2 = match pop_mut(self.free_structures) {
    			Ok((new_head, popped)) => {
    				self.free_structures = new_head;
    				Ok(popped)
    			},
    			Err(e) => {
    				self.free(larger);
					self.free_structures = push(self.free_structures, new1);
    				cspace.free_slot(cslot1);
    				Err(e)
    			}
    		}?;

			let cslot2 = cspace.alloc_slot().map_err(|e| {
				self.free(larger);
				self.free_structures = push(self.free_structures, new1);
				cspace.free_slot(cslot1);
				self.free_structures = push(self.free_structures, new2);
				e
			})?;
    		unsafe {
    			(*new2).cap = CPtr::from_bits(cslot2.try_into().unwrap()).cast::<sel4::cap_type::Untyped>();
    			(*new2).size_bits = size_bits.try_into().unwrap();
    		}

    		// Untype the larger object into the two new smaller objects
    		if let Err(e) = cspace.untyped_retype(&larger.get_cap(),
    											  sel4::ObjectBlueprint::Untyped { size_bits: size_bits },
    											  cslot1) {

				self.free(larger);
				self.free_structures = push(self.free_structures, new1);
				cspace.free_slot(cslot1);
				self.free_structures = push(self.free_structures, new2);
				cspace.free_slot(cslot2);
				return Err(e)
    		}

    		if let Err(e) = cspace.untyped_retype(&larger.get_cap(),
    											  sel4::ObjectBlueprint::Untyped { size_bits: size_bits },
    											  cslot2) {
				self.free(larger);
				self.free_structures = push(self.free_structures, new1);
    			cspace.delete(cslot1)?;
				cspace.free_slot(cslot1);
				self.free_structures = push(self.free_structures, new2);
				cspace.free_slot(cslot2);
				return Err(e)
    		}

    		self.free_untypeds[SIZE_BITS_TO_INDEX(size_bits)] = push(self.free_untypeds[SIZE_BITS_TO_INDEX(size_bits)], new1);
    		self.free_untypeds[SIZE_BITS_TO_INDEX(size_bits)] = push(self.free_untypeds[SIZE_BITS_TO_INDEX(size_bits)], new2);
		}

		/* inv -> there must be at least one element in the list at this point */
		let res = pop(self.free_untypeds[SIZE_BITS_TO_INDEX(size_bits)]).unwrap();
		self.free_untypeds[SIZE_BITS_TO_INDEX(size_bits)] = res.0;
		return Ok(UTWrapper { ut: res.1 });
	}

	pub fn new(memory: usize, region: UTRegion, next_free_vaddr: usize) -> UTTable {
		UTTable { first_paddr: region.start, untypeds: Some(memory as *mut UT),
				  free_untypeds: [None; N_UNTYPED_LISTS], n_4k_untyped: 0, free_structures: None,
				  next_free_vaddr : next_free_vaddr}
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