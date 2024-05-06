#![allow(non_snake_case)]

use sel4::ObjectBlueprint;
use crate::bitfield::{bf_clr_bit, bf_first_free, bf_set_bit};
use crate::page::{BIT, PAGE_SIZE_4K};
use core::mem::size_of;
use crate::bootstrap::{INITIAL_TASK_CNODE_SIZE_BITS};
use crate::bitfield::{bitfield_type, bitfield_init};
use crate::ut::UTWrapper;
use crate::util::MASK;
use crate::warn_rs;


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
	untyped: UTWrapper
}

// @alwin: Should this be public?
#[derive(Copy, Clone)]
pub struct BotLvlNodeT {
	pub n_cnodes: usize,
	pub untyped: UTWrapper,
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

	pub fn irq_control_get(self: &mut Self, cptr: usize, irq_control: sel4::cap::IrqControl,
						   irq: usize, edge_triggered : bool) -> Result<sel4::cap::IrqHandler, sel4::Error> {
		let irq_handler = sel4::CPtr::from_bits(cptr.try_into().unwrap()).cast::<sel4::cap_type::IrqHandler>();
		// @alwin: Edge triggered is expected to be a word instead of a bool for some reason. Submit a PR
		// to rust-sel4 to fix this

		// @alwin: Need some way of determining if an IRQ is a PPI or not here to do the right
		// invocation
		irq_control.irq_control_get_trigger(irq.try_into().unwrap(), edge_triggered.try_into().unwrap(),
												   &self.root_cnode.relative(irq_handler))
												   .or(Err(sel4::Error::InvalidArgument))?;

		// @alwin: Core number is hard-coded here
		// irq_control.irq_control_get_trigger_core(irq.try_into().unwrap(), edge_triggered.try_into().unwrap(),
		// 										 0, &self.root_cnode.relative(irq_handler))
		// 										 .or(Err(sel4::Error::InvalidArgument))?;

		return Ok(irq_handler);
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

	pub fn ensure_levels(self: &Self, _cptr: usize, _n_slots: usize) -> Result<usize, sel4::Error> {
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
		let top_index = bf_first_free(&self.top_bf)?;
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
		   	let bot_lvl = &mut self.get_bot_lvl_node(NODE_INDEX(cptr)).cnodes[CNODE_INDEX(cptr)];
		   	let bot_index = bf_first_free(&bot_lvl.bf)?;
		   	bf_set_bit(&mut bot_lvl.bf, bot_index);

	        /* check if there are any free slots left in this cnode */
		  	if bf_first_free(&bot_lvl.bf)? >= CNODE_SLOTS(CNODE_SIZE_BITS) - 1 {
		  		bf_set_bit(&mut self.top_bf, top_index)
			}

			cptr += bot_index;

			self.refill_watermark(used)?;
	   	} else {
	   		bf_set_bit(&mut self.top_bf, top_index)
	   	}

	   	return Ok(cptr);
	}

	pub fn delete(self: &mut Self, cptr: usize) -> Result<(), sel4::Error> {
		self.root_cnode.relative_bits_with_depth(cptr.try_into().unwrap(), sel4::WORD_SIZE).delete()
	}

	pub fn free_slot(self: &mut Self, cptr: usize) {
		if cptr == 0 {
			return;
		}

		if !self.two_level {
			if cptr > CNODE_SLOTS(self.top_level_size_bits) {
				warn_rs!("Attempting to delete slot greater than cspace bounds");
				return;
			}
			bf_clr_bit(&mut self.top_bf, cptr);
		} else {
			if cptr >CNODE_SLOTS(CNODE_SIZE_BITS + self.top_level_size_bits) {
				warn_rs!("Attempting to delete slot greater than cspace bounds");
				return;
			}

			bf_clr_bit(&mut self.top_bf, TOP_LVL_INDEX(cptr));
			let node = NODE_INDEX(cptr);
			if self.n_bot_lvl_nodes > node {
				let cnode = CNODE_INDEX(cptr);
				if self.get_bot_lvl_node(node).n_cnodes > cnode {
					bf_clr_bit(&mut self.get_bot_lvl_node(node).cnodes[cnode].bf, BOT_LVL_INDEX(cptr));
				} else {
					warn_rs!("Attempting to free unallocated cptr {}", cptr);
				}
			} else {
				warn_rs!("Attempting to free unallocated cptr {}", cptr);
			}
		}
	}

	pub fn get_bot_lvl_node(self: &Self, i : usize) -> &mut BotLvlNodeT {
		return unsafe{&mut *self.bot_lvl_nodes[i]};
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