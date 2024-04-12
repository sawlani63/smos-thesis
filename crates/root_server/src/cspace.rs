use crate::page::{PAGE_SIZE_4K, BIT};
use core::mem::size_of;

pub const fn CNODE_SLOT_BITS(x : usize) -> usize {
	x - sel4_sys::seL4_SlotBits as usize
}

const MAPPING_SLOTS: usize = 3;
const WATERMARK_SLOTS: usize = MAPPING_SLOTS + 1;
pub const CNODE_SIZE_BITS: usize = 12;

// const BOT_LVL_PER_NODE: usize = (PAGE_SIZE_4K - (sel4_sys::seL4_WordSizeBits * 3) as usize) / size_of::<BotLvlT>();
pub const CNODE_SLOTS: usize = BIT(CNODE_SLOT_BITS(CNODE_SIZE_BITS));

// struct BotLvlT {
// 	bf : [bool; CNODE_SLOTS],
// 	untyped : todo!() /* ??? */
// }

// struct BotLvlNodeT {
// 	n_cnodes: sel4_sys::seL4_Word,
// 	untyped: todo!() /* ??? */,
// 	frame: sel4::cap::UnspecifiedFrame,
// 	cnodes:  [BotLvlT; BOT_LVL_PER_NODE]
// }

// pub struct CSpace<'a> {
// 	pub root_cnode: sel4::CNode,
// 	pub two_level: bool,
// 	top_level_size_bits: i32,
// 	top_bf: todo!()/* ?? */,
// 	bot_lvl_nodes: todo!()/* ?? */,
// 	untyped: todo!()/* ?? */,
// 	pub bootstrap: Option<&'a CSpace<'a>>,
// 	alloc: todo!()/* cspace_alloc_t */,
// 	watermark: [sel4::CPtr; WATERMARK_SLOTS]
// }

// impl<'a> CSpace<'a> {
// 	pub fn new() -> Self {
// 		Self { root_cnode: (), two_level: (), top_level_size_bits: (), top_bf: (), bot_lvl_nodes: (), untyped: (), bootstrap: (), alloc: (), watermark: () }
// 	}
// }