use sel4::ObjectBlueprint;

use crate::{bitfield::{bf_first_free, bf_set_bit}, bootstrap, page::{BIT, PAGE_SIZE_4K}};
use core::{cell::RefMut, mem::size_of, ptr::{null, null_mut}};
use crate::bootstrap::{INITIAL_TASK_CNODE_SIZE_BITS, INITIAL_TASK_CSPACE_BITS, INITIAL_TASK_CSPACE_SLOTS};
use crate::bitfield::{bitfield_type, BITFIELD_SIZE, bitfield_init};
use crate::ut::UT;
use crate::util::MASK;

pub const fn CNODE_SLOT_BITS(x : usize) -> usize {
	x - sel4_sys::seL4_SlotBits as usize
}

pub const fn CNODE_SLOTS(x: usize) -> usize {
	BIT(CNODE_SLOT_BITS(x))
}

pub const fn TOP_LVL_INDEX(cptr : usize) -> usize {
	cptr >> CNODE_SLOT_BITS(CNODE_SIZE_BITS)
}

pub const fn BOT_LVL_INDEX(cptr: usize) -> usize {
	cptr & MASK(CNODE_SLOT_BITS(CNODE_SIZE_BITS))
}


pub const MAPPING_SLOTS: usize = 3;
pub const WATERMARK_SLOTS: usize = MAPPING_SLOTS + 1;
// @alwin: This was  bumped up to 13 from 12 because it's not big enough. Safe?
pub const CNODE_SIZE_BITS: usize = 13;
pub const BOT_LVL_PER_NODE : usize = (PAGE_SIZE_4K - sel4::WORD_SIZE * 3) / size_of::<BotLvlT>();

#[derive(Copy, Clone)]
pub struct BotLvlT {
	pub bf : bitfield_type!(CNODE_SLOTS(CNODE_SIZE_BITS)),
	untyped: UT
}

// @alwin: Should this be public?
#[derive(Copy, Clone)]
pub struct BotLvlNodeT {
	pub n_cnodes: usize,
	pub untyped: UT,
	pub frame: sel4::cap::SmallPage,
	pub cnodes: [BotLvlT; BOT_LVL_PER_NODE]
}

struct CSpaceAlloc {
	map_frame: fn(usize, sel4::cap::UnspecifiedFrame, [sel4::AbsoluteCPtr; MAPPING_SLOTS]) -> (usize, usize),
	alloc_4k_ut: fn(usize) -> (sel4::AbsoluteCPtr, usize),
	free_4k_ut: fn(usize, usize),
	cookie: usize,
}

pub struct CSpace<'a> {
	pub root_cnode: sel4::cap::CNode,
	pub two_level: bool,
	top_level_size_bits: usize,
	pub top_bf: bitfield_type!(CNODE_SLOTS(INITIAL_TASK_CNODE_SIZE_BITS)),
	bot_lvl_nodes: &'a mut [*mut BotLvlNodeT],
	pub n_bot_lvl_nodes: usize,
	// untyped: todo!()/* ?? */, // @alwin: Add this back when I figure out what it should be
	pub bootstrap: Option<&'a CSpace<'a>>,
	// alloc: CSpaceAlloc, // @alwin: Add this back when I figure out what it should be
	pub watermark: [usize; WATERMARK_SLOTS]
}

pub const fn NODE_INDEX(cptr : usize ) -> usize {
	cptr / CNODE_SLOTS(CNODE_SIZE_BITS) / BOT_LVL_PER_NODE
}

pub const fn CNODE_INDEX(cptr : usize ) -> usize {
	cptr / CNODE_SLOTS(CNODE_SIZE_BITS) % BOT_LVL_PER_NODE
}

impl<'a> CSpace<'a> {
	pub fn new(root_cnode: sel4::cap::CNode, two_level: bool, top_level_size_bits: usize,
			   bot_lvl_nodes: &'a mut [*mut BotLvlNodeT], bootstrap: Option<&'a CSpace<'a>>,
			   /* alloc : CSpaceAlloc */) -> Self {

		return CSpace { root_cnode: root_cnode,
						two_level: two_level,
						top_level_size_bits: top_level_size_bits,
						top_bf: bitfield_init!(CNODE_SLOTS(INITIAL_TASK_CNODE_SIZE_BITS)),
						n_bot_lvl_nodes: 0,
						bot_lvl_nodes: bot_lvl_nodes,
						bootstrap: bootstrap,
						// alloc: alloc,
						watermark: [0; WATERMARK_SLOTS]
					};
	}

	pub fn untyped_retype(self: &Self, ut: &sel4::cap::Untyped, blueprint: ObjectBlueprint,
						  target: usize) -> Result<(), sel4::Error> {

		if self.two_level {
			let cnode = target >> CNODE_SLOT_BITS(CNODE_SIZE_BITS);
			return ut.untyped_retype(&blueprint,
									 &self.root_cnode.relative_bits_with_depth(cnode.try_into().unwrap(),
					  				 sel4::WORD_SIZE - CNODE_SLOT_BITS(CNODE_SIZE_BITS)),
					  				 target % CNODE_SLOTS(CNODE_SIZE_BITS), 1);
		} else {
			return ut.untyped_retype(&blueprint, &self.root_cnode.relative_self(), target, 1)
		}
	}

	pub fn ensure_levels(self: &Self, cptr: usize, n_slots: usize) -> Result<usize, sel4::Error> {
		todo!();
	}

	pub fn refill_watermark(self: &mut Self, used: usize) -> Result<(), sel4::Error> {
		for i in 0..WATERMARK_SLOTS {
			if used & BIT(i) != 0 {
				self.watermark[i] = self.alloc_slot()?;
				break;
			}
		}

		Ok(())
	}

	pub fn alloc_slot(self: &mut Self) -> Result<usize, sel4::Error> {
		let top_index = bf_first_free(&self.top_bf);
		if self.two_level && top_index > CNODE_SLOTS(self.top_level_size_bits) ||
		   top_index >= CNODE_SLOTS(self.top_level_size_bits) {
		   		return Err(sel4::Error::InvalidCapability);
	   	}

	   	let mut cptr = top_index;
		if self.two_level {
	   		let mut used = 0;
	   		cptr = cptr << CNODE_SLOT_BITS(CNODE_SIZE_BITS);

        	/* ensure the bottom level cnode is present */
	   		if self.n_bot_lvl_nodes <= NODE_INDEX(cptr) ||
			   self.get_bot_lvl_node(NODE_INDEX(cptr)).n_cnodes <= CNODE_INDEX(cptr) {

				used = self.ensure_levels(cptr, MAPPING_SLOTS)?;
		   	}

	        /* now allocate a bottom level index */
		   	let mut bot_lvl = &mut self.get_bot_lvl_node(NODE_INDEX(cptr)).cnodes[CNODE_INDEX(cptr)];
		   	let bot_index = bf_first_free(&bot_lvl.bf);
		   	bf_set_bit(&mut bot_lvl.bf, bot_index);

	        /* check if there are any free slots left in this cnode */
		  	if bf_first_free(&bot_lvl.bf) >= CNODE_SLOTS(CNODE_SIZE_BITS) - 1 {
		  		bf_set_bit(&mut self.top_bf, top_index)
			}

			cptr += bot_index;

			self.refill_watermark(used);
	   	} else {
	   		bf_set_bit(&mut self.top_bf, top_index)
	   	}

	   	return Ok(cptr);
	}

	pub fn get_bot_lvl_node(self: &Self, i : usize) -> &mut BotLvlNodeT {
		return unsafe{&mut *self.bot_lvl_nodes[i]};
	}

	pub fn set_bot_lvl_node(self: &mut Self, i : usize, ptr: *mut BotLvlNodeT)  {
		self.bot_lvl_nodes[i] = ptr;
	}

	pub fn init_bot_lvl_node(self: &mut Self, i : usize, ptr: *mut BotLvlNodeT) {
		// @alwin: is this the best place for this to happen?
		unsafe {
			// cast to u8 for memset equivalent
			core::ptr::write_bytes(ptr as *mut u8, 0, PAGE_SIZE_4K);
			self.bot_lvl_nodes[i] = ptr;
		}
	}
}