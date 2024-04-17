use crate::{bootstrap, page::{BIT, PAGE_SIZE_4K}};
use core::mem::size_of;
use bit_vec::BitVec;
use crate::bootstrap::{INITIAL_TASK_CNODE_SIZE_BITS, INITIAL_TASK_CSPACE_BITS, INITIAL_TASK_CSPACE_SLOTS};

pub const fn CNODE_SLOT_BITS(x : usize) -> usize {
	x - sel4_sys::seL4_SlotBits as usize
}

pub const fn CNODE_SLOTS(x: usize) -> usize {
	BIT(CNODE_SLOT_BITS(x))
}

pub const fn TOP_LVL_INDEX(cptr : usize) -> usize {
	cptr >> CNODE_SLOT_BITS(CNODE_SIZE_BITS)
}

pub const MAPPING_SLOTS: usize = 3;
pub const WATERMARK_SLOTS: usize = MAPPING_SLOTS + 1;
pub const CNODE_SIZE_BITS: usize = 12;
pub const BOT_LVL_PER_NODE : usize = (PAGE_SIZE_4K - sel4::WORD_SIZE * 3) / size_of::<BotLvlT>();

// const BOT_LVL_PER_NODE: usize = (PAGE_SIZE_4K - (sel4_sys::seL4_WordSizeBits * 3) as usize) / size_of::<BotLvlT>();

struct BotLvlT {
	pub bf : BitField,
	// untyped : todo!() /* ??? */
}

// @alwin: Should this be public?
#[derive(Copy, Clone)]
pub struct BotLvlNodeT {
	pub n_cnodes: sel4_sys::seL4_Word,
	// untyped: todo!() /* ??? */,
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
	pub top_bf: BitField,
	bot_lvl_nodes: [*mut BotLvlNodeT; INITIAL_TASK_CSPACE_SLOTS / BOT_LVL_PER_NODE + 1],
	// untyped: todo!()/* ?? */, // @alwin: Add this back when I figure out what it should be
	pub bootstrap: Option<&'a CSpace<'a>>,
	// alloc: CSpaceAlloc, // @alwin: Add this back when I figure out what it should be
	// watermark: [sel4::CPtr; WATERMARK_SLOTS] // @alwin: Add this back when I figure out what it should be
}

pub const fn CNODE_INDEX(cptr : usize ) -> usize {
	cptr / CNODE_SLOTS(CNODE_SIZE_BITS) / BOT_LVL_PER_NODE
}


impl<'a> CSpace<'a> {
	pub fn new(root_cnode: sel4::cap::CNode, two_level: bool, top_level_size_bits: usize, bootstrap: Option<&'a CSpace<'a>>, /* alloc : CSpaceAlloc */) -> Self {
		return CSpace { root_cnode: root_cnode,
						two_level: two_level,
						top_level_size_bits: top_level_size_bits,
						top_bf: BitField::new(CNODE_SLOTS(INITIAL_TASK_CNODE_SIZE_BITS)),
						bot_lvl_nodes:
							[
								BotLvlNodeT {
									n_cnodes: 0,
									frame: None,
									cnodes: [BotLvlT {bf: BitField::new(BITFIELD_SIZE(CNODE_SIZE_BITS))} ; BOT_LVL_PER_NODE ]
								};
								INITIAL_TASK_CSPACE_SLOTS / BOT_LVL_PER_NODE + 1
							],
						bootstrap: bootstrap,
						// alloc: alloc,
						// watermark: [0] 
					};
	}

	pub fn get_bot_lvl_node(self: &Self, i : usize) -> &mut BotLvlNodeT {
		return unsafe{&mut *self.bot_lvl_nodes[i]};
	}

	pub fn init_bot_lvl_node(self: &Self, i : usize, ptr: *mut BotLvlNodeT) {
		// @alwin: is this the best place for this to be?
		unsafe {
			core::ptr::write_bytes(ptr, 0, PAGE_SIZE_4K);
		}
		self.bot_lvl_nodes[i] = ptr;
	}
}