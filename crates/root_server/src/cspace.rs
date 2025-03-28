#![allow(non_snake_case)]

use crate::bootstrap::INITIAL_TASK_CNODE_SIZE_BITS;
use crate::page::{BYTES_TO_4K_PAGES, PAGE_SIZE_4K};
use crate::ut::{UTTable, UTWrapper};
use crate::util::{alloc_retype, MASK};
use crate::warn_rs;
use alloc::boxed::Box;
use bitfield::{bf_clr_bit, bf_first_free, bf_set_bit, bitfield_init, bitfield_type};
use core::mem::size_of;
use sel4::sel4_cfg;
use sel4::{CNodeCapData, ObjectBlueprint};
use smos_common::util::{BIT, ROUND_DOWN};

pub const fn CNODE_SLOT_BITS(x: usize) -> usize {
    x - sel4_sys::seL4_SlotBits as usize
}

pub const fn CNODE_SLOTS(x: usize) -> usize {
    BIT(CNODE_SLOT_BITS(x))
}

pub const fn TOP_LVL_INDEX(cptr: usize) -> usize {
    cptr >> CNODE_SLOT_BITS(CNODE_SIZE_BITS)
}

pub const fn BOT_LVL_INDEX(cptr: usize) -> usize {
    cptr & MASK(CNODE_SLOT_BITS(CNODE_SIZE_BITS))
}

const fn NUM_BOT_LVL_NODES(bits: usize) -> usize {
    BYTES_TO_4K_PAGES(size_of::<BotLvlT>() * CNODE_SLOTS(bits))
}

pub const MAPPING_SLOTS: usize = 3;
pub const WATERMARK_SLOTS: usize = MAPPING_SLOTS + 1;
// @alwin: This was  bumped up to 13 from 12 because it's not big enough. Safe?
pub const CNODE_SIZE_BITS: usize = 13;
pub const BOT_LVL_PER_NODE: usize = (PAGE_SIZE_4K - sel4::WORD_SIZE * 3) / size_of::<BotLvlT>();

#[derive(Copy, Clone)]
pub struct BotLvlT {
    pub bf: bitfield_type!(CNODE_SLOTS(CNODE_SIZE_BITS)),
    #[allow(dead_code)] // @alwin: This should hopefully be removed later
    untyped: UTWrapper,
}

pub trait CSpaceTrait {
    fn untyped_retype(
        &self,
        ut: &sel4::cap::Untyped,
        blueprint: ObjectBlueprint,
        target: usize,
    ) -> Result<(), sel4::Error> {
        if self.is_two_level() {
            let cnode = target >> CNODE_SLOT_BITS(CNODE_SIZE_BITS);
            return ut.untyped_retype(
                &blueprint,
                &self.root_cnode().absolute_cptr_from_bits_with_depth(
                    cnode.try_into().unwrap(),
                    sel4::WORD_SIZE - CNODE_SLOT_BITS(CNODE_SIZE_BITS),
                ),
                target % CNODE_SLOTS(CNODE_SIZE_BITS),
                1,
            );
        } else {
            return ut.untyped_retype(
                &blueprint,
                &self.root_cnode().absolute_cptr_for_self(),
                target,
                1,
            );
        }
    }

    fn ensure_levels(&self, _cptr: usize, _n_slots: usize) -> Result<usize, sel4::Error> {
        todo!();
    }

    fn refill_watermark(self: &mut Self, used: usize) -> Result<(), sel4::Error> {
        for i in 0..WATERMARK_SLOTS {
            if used & BIT(i) != 0 {
                let slot = self.alloc_slot()?;
                self.set_watermark(i, slot);
                break;
            }
        }

        Ok(())
    }

    fn alloc_cap<T: sel4::CapType>(&mut self) -> Result<sel4::Cap<T>, sel4::Error> {
        let slot = self.alloc_slot()?;
        return Ok(sel4::CPtr::from_bits(slot.try_into().unwrap()).cast::<T>());
    }

    fn alloc_slot(&mut self) -> Result<usize, sel4::Error> {
        let top_index = bf_first_free(self.top_bf()).map_err(|_| sel4::Error::NotEnoughMemory)?;
        if self.is_two_level() && top_index > CNODE_SLOTS(self.top_lvl_size_bits())
            || top_index >= CNODE_SLOTS(self.top_lvl_size_bits())
        {
            return Err(sel4::Error::InvalidCapability);
        }

        let mut cptr = top_index;
        if self.is_two_level() {
            let mut used = 0;
            cptr = cptr << CNODE_SLOT_BITS(CNODE_SIZE_BITS);

            /* ensure the bottom level cnode is present */
            if self.n_bot_lvl_nodes() <= NODE_INDEX(cptr)
                || unsafe { self.get_bot_lvl_node(NODE_INDEX(cptr)).n_cnodes } <= CNODE_INDEX(cptr)
            {
                used = self.ensure_levels(cptr, MAPPING_SLOTS)?;
            }

            /* now allocate a bottom level index */
            let bot_lvl =
                unsafe { &mut self.get_bot_lvl_node(NODE_INDEX(cptr)).cnodes[CNODE_INDEX(cptr)] };
            let bot_index = bf_first_free(&bot_lvl.bf).map_err(|_| sel4::Error::NotEnoughMemory)?;
            bf_set_bit(&mut bot_lvl.bf, bot_index);

            /* check if there are any free slots left in this cnode */
            if bf_first_free(&bot_lvl.bf).map_err(|_| sel4::Error::NotEnoughMemory)?
                >= CNODE_SLOTS(CNODE_SIZE_BITS) - 1
            {
                bf_set_bit(self.top_bf_mut(), top_index)
            }

            cptr += bot_index;

            self.refill_watermark(used)?;
        } else {
            bf_set_bit(&mut self.top_bf_mut(), top_index)
        }

        return Ok(cptr);
    }

    fn delete(&self, cptr: usize) -> Result<(), sel4::Error> {
        self.root_cnode()
            .absolute_cptr_from_bits_with_depth(cptr.try_into().unwrap(), sel4::WORD_SIZE)
            .delete()
    }

    fn delete_cap<T: sel4::CapType>(&self, cap: sel4::Cap<T>) -> Result<(), sel4::Error> {
        self.delete(cap.bits().try_into().unwrap())
    }

    fn free_slot(&mut self, cptr: usize) {
        if cptr == 0 {
            return;
        }

        if !self.is_two_level() {
            if cptr > CNODE_SLOTS(self.top_lvl_size_bits()) {
                warn_rs!("Attempting to delete slot greater than cspace bounds");
                return;
            }
            bf_clr_bit(self.top_bf_mut(), cptr);
        } else {
            if cptr > CNODE_SLOTS(CNODE_SIZE_BITS + self.top_lvl_size_bits()) {
                warn_rs!("Attempting to delete slot greater than cspace bounds");
                return;
            }

            bf_clr_bit(&mut self.top_bf_mut(), TOP_LVL_INDEX(cptr));
            let node = NODE_INDEX(cptr);
            if self.n_bot_lvl_nodes() > node {
                let cnode = CNODE_INDEX(cptr);
                if unsafe { self.get_bot_lvl_node(node).n_cnodes } > cnode {
                    bf_clr_bit(
                        unsafe { &mut self.get_bot_lvl_node(node).cnodes[cnode].bf },
                        BOT_LVL_INDEX(cptr),
                    );
                } else {
                    warn_rs!("Attempting to free unallocated cptr {}", cptr);
                }
            } else {
                warn_rs!("Attempting to free unallocated cptr {}", cptr);
            }
        }
    }

    fn free_cap<T: sel4::CapType>(&mut self, cap: sel4::Cap<T>) {
        self.free_slot(cap.bits().try_into().unwrap());
    }

    fn is_two_level(&self) -> bool;
    fn root_cnode(&self) -> sel4::cap::CNode;
    fn set_watermark(&mut self, idx: usize, cptr: usize);
    fn top_lvl_size_bits(&self) -> usize;
    fn n_bot_lvl_nodes(&self) -> usize;
    fn top_bf_mut<'b, 'a: 'b>(&'a mut self) -> &'b mut [u64];
    unsafe fn get_bot_lvl_node<'b, 'a: 'b>(&'a self, i: usize) -> &'b mut BotLvlNodeT;
    fn top_bf<'b, 'a: 'b>(&'a self) -> &'b [u64];
}

// @alwin: Should this be public?
#[derive(Copy, Clone)]
pub struct BotLvlNodeT {
    pub n_cnodes: usize,
    pub untyped: UTWrapper,
    pub frame: sel4::cap::SmallPage,
    pub cnodes: [BotLvlT; BOT_LVL_PER_NODE],
}

#[allow(dead_code)] //@alwin: This was in the C impl, might not be needed here
struct CSpaceAlloc {
    map_frame: fn(
        usize,
        sel4::cap::UnspecifiedPage,
        [sel4::AbsoluteCPtr; MAPPING_SLOTS],
    ) -> (usize, usize),
    alloc_4k_ut: fn(usize) -> (sel4::AbsoluteCPtr, usize),
    free_4k_ut: fn(usize, usize),
    cookie: usize,
}

#[derive(Clone, Debug)]
pub struct UserCSpace {
    root_cnode: sel4::cap::CNode,
    pub top_bf: bitfield_type!(CNODE_SLOTS(CNODE_SIZE_BITS)),
    // bootstrap: &'a CSpace<'a>,
    top_lvl_size_bits: usize,
    pub two_level: bool,
    pub n_bot_lvl_nodes: usize,
    bot_lvl_nodes: Option<Box<[*mut BotLvlNodeT; NUM_BOT_LVL_NODES(CNODE_SIZE_BITS)]>>,
    untyped: UTWrapper,
    pub watermark: [usize; WATERMARK_SLOTS],
}

impl CSpaceTrait for UserCSpace {
    fn is_two_level(self: &Self) -> bool {
        return self.two_level;
    }

    fn root_cnode(&self) -> sel4::cap::CNode {
        return self.root_cnode;
    }

    fn set_watermark(&mut self, idx: usize, cptr: usize) {
        assert!(idx < self.watermark.len());
        self.watermark[idx] = cptr;
    }

    fn top_lvl_size_bits(&self) -> usize {
        return self.top_lvl_size_bits;
    }

    fn n_bot_lvl_nodes(&self) -> usize {
        return self.n_bot_lvl_nodes;
    }

    fn top_bf_mut<'b, 'a: 'b>(&'a mut self) -> &'b mut [u64] {
        return &mut self.top_bf;
    }

    unsafe fn get_bot_lvl_node<'b, 'a: 'b>(&'a self, i: usize) -> &'b mut BotLvlNodeT {
        assert!((self.bot_lvl_nodes.as_ref().unwrap())[i] != core::ptr::null_mut());
        return unsafe { &mut *(self.bot_lvl_nodes.as_ref().unwrap())[i] };
    }

    fn top_bf<'b, 'a: 'b>(&'a self) -> &'b [u64] {
        return &self.top_bf;
    }
}

impl UserCSpace {
    pub fn new(
        bootstrap: &mut CSpace,
        ut_table: &mut UTTable,
        two_lvl: bool,
    ) -> Result<Self, sel4::Error> {
        // @alwin: this c version of this doesn't look right
        let bot_lvl = if two_lvl {
            Some(
                Box::<[*mut BotLvlNodeT; NUM_BOT_LVL_NODES(CNODE_SIZE_BITS)]>::new(
                    [core::ptr::null_mut(); NUM_BOT_LVL_NODES(CNODE_SIZE_BITS)],
                ),
            )
        } else {
            None
        };
        let untyped = alloc_retype::<sel4::cap_type::CNode>(
            bootstrap,
            ut_table,
            ObjectBlueprint::CNode {
                size_bits: CNODE_SLOT_BITS(sel4_sys::seL4_PageBits.try_into().unwrap()),
            },
        )?;

        // Mint the cnode cap with that guard and make it the cap to the root_cnode this cspace --
        // this means that objects in this cspace can be directly invoked with depth seL4_WordBits */
        // @alwin: I think the reason this works is because I increased the size of CNODE_SIZE_BITS from 12 to 13.
        // 2^12 is one page, which is what is allocated, while 2^13 is more than this. This design is kind of problematic
        // actually, as the maximally sized one level cspace doesn't really have many slots. I guess you could always use
        // a multilevel cspace.
        let depth = sel4::WORD_SIZE
            - ((CNODE_SLOT_BITS(CNODE_SIZE_BITS - 1)) * (if two_lvl { 2 } else { 1 }));
        let guard = CNodeCapData::new(0, depth);
        let root_cnode = bootstrap.alloc_slot()?;
        bootstrap
            .root_cnode
            .absolute_cptr_from_bits_with_depth(root_cnode.try_into().unwrap(), sel4::WORD_SIZE)
            .mint(
                &bootstrap.root_cnode.absolute_cptr(untyped.0),
                sel4::CapRightsBuilder::all().build(),
                guard.into_word(),
            )
            .expect("Failed to mint root cnode cap");

        bootstrap
            .delete(untyped.0.bits().try_into().unwrap())
            .expect("Failed to delete untyped");
        bootstrap.free_slot(untyped.0.bits().try_into().unwrap());

        if two_lvl {
            todo!();
        }

        let mut new_cspace = UserCSpace {
            root_cnode: sel4::CPtr::from_bits(root_cnode.try_into().unwrap()).cast(),
            top_bf: bitfield_init!(CNODE_SLOTS(CNODE_SIZE_BITS)),
            // bootstrap: bootstrap,
            top_lvl_size_bits: CNODE_SIZE_BITS,
            two_level: two_lvl,
            n_bot_lvl_nodes: 0,
            bot_lvl_nodes: bot_lvl,
            untyped: untyped.1,
            watermark: [0; WATERMARK_SLOTS],
        };

        // @alwin: This allocates capNull, is this necessary with optional and result types?
        assert!(new_cspace.alloc_slot()? == 0);

        return Ok(new_cspace);
    }

    pub fn destroy(&mut self, bootstrap: &mut CSpace, ut_table: &mut UTTable) {
        /* We have to go through and delete everything because we don't know what the user
        does with the cspace */
        for i in 1..BIT(CNODE_SIZE_BITS) {
            self.root_cnode
                .absolute_cptr_from_bits_with_depth(i.try_into().unwrap(), sel4::WORD_SIZE)
                .delete()
                .expect("Failed to delete cap");
        }

        ut_table.free(self.untyped);

        bootstrap
            .root_cnode()
            .absolute_cptr(self.root_cnode)
            .delete()
            .expect("Failed to delete root cnode");
    }
}

pub struct CSpace<'a> {
    pub root_cnode: sel4::cap::CNode,
    pub two_level: bool,
    top_lvl_size_bits: usize,
    pub top_bf: bitfield_type!(CNODE_SLOTS(INITIAL_TASK_CNODE_SIZE_BITS)),
    bot_lvl_nodes: &'a mut [*mut BotLvlNodeT],
    pub n_bot_lvl_nodes: usize,
    pub watermark: [usize; WATERMARK_SLOTS],
}

pub const fn NODE_INDEX(cptr: usize) -> usize {
    cptr / CNODE_SLOTS(CNODE_SIZE_BITS) / BOT_LVL_PER_NODE
}

pub const fn CNODE_INDEX(cptr: usize) -> usize {
    cptr / CNODE_SLOTS(CNODE_SIZE_BITS) % BOT_LVL_PER_NODE
}

impl<'c> CSpaceTrait for CSpace<'c> {
    fn is_two_level(self: &Self) -> bool {
        return self.two_level;
    }

    fn root_cnode(&self) -> sel4::cap::CNode {
        return self.root_cnode;
    }

    fn set_watermark(&mut self, idx: usize, cptr: usize) {
        assert!(idx < self.watermark.len());
        self.watermark[idx] = cptr;
    }

    fn top_lvl_size_bits(&self) -> usize {
        return self.top_lvl_size_bits;
    }

    fn n_bot_lvl_nodes(&self) -> usize {
        return self.n_bot_lvl_nodes;
    }

    fn top_bf_mut<'b, 'a: 'b>(&'a mut self) -> &'b mut [u64] {
        return &mut self.top_bf;
    }

    // Safety: bot_lvl_nodes[i] must have been set prior to calling this function
    unsafe fn get_bot_lvl_node<'b, 'a: 'b>(&'a self, i: usize) -> &'b mut BotLvlNodeT {
        assert!(self.bot_lvl_nodes[i] != core::ptr::null_mut());
        return unsafe { &mut *self.bot_lvl_nodes[i] };
    }

    fn top_bf<'b, 'a: 'b>(&'a self) -> &'b [u64] {
        return &self.top_bf;
    }
}

impl<'a> CSpace<'a> {
    pub fn new(
        root_cnode: sel4::cap::CNode,
        two_level: bool,
        top_lvl_size_bits: usize,
        bot_lvl_nodes: &'a mut [*mut BotLvlNodeT],
        _bootstrap: Option<&'a CSpace<'a>>,
        /* alloc : CSpaceAlloc */
    ) -> Self {
        return CSpace {
            root_cnode: root_cnode,
            two_level: two_level,
            top_lvl_size_bits: top_lvl_size_bits,
            top_bf: bitfield_init!(CNODE_SLOTS(INITIAL_TASK_CNODE_SIZE_BITS)),
            n_bot_lvl_nodes: 0,
            bot_lvl_nodes: bot_lvl_nodes,
            // bootstrap: bootstrap,
            // alloc: alloc,
            watermark: [0; WATERMARK_SLOTS],
        };
    }

    // @alwin: is conditional compilation the way to go here?
    #[sel4_cfg(ENABLE_SMP_SUPPORT)]
    fn irq_control_get_ppi(
        &mut self,
        irq_control: sel4::cap::IrqControl,
        irq: usize,
        edge_triggered: bool,
        irq_handler: sel4::cap::IrqHandler,
    ) -> Result<(), sel4::Error> {
        // @alwin: Core number is currently hardcoded here
        irq_control
            .irq_control_get_trigger_core(
                irq.try_into().unwrap(),
                edge_triggered.into(),
                0,
                &self.root_cnode.absolute_cptr(irq_handler),
            )
            .or(Err(sel4::Error::InvalidArgument))
    }

    #[sel4_cfg(not(ENABLE_SMP_SUPPORT))]
    fn irq_control_get_ppi(
        &mut self,
        irq_control: sel4::cap::IrqControl,
        irq: usize,
        edge_triggered: bool,
        irq_handler: sel4::cap::IrqHandler,
    ) -> Result<(), sel4::Error> {
        irq_control
            .irq_control_get_trigger(
                irq.try_into().unwrap(),
                edge_triggered.into(),
                &self.root_cnode.absolute_cptr(irq_handler),
            )
            .or(Err(sel4::Error::InvalidArgument))
    }

    pub fn irq_control_get(
        self: &mut Self,
        cptr: usize,
        irq_control: sel4::cap::IrqControl,
        irq: usize,
        edge_triggered: bool,
    ) -> Result<sel4::cap::IrqHandler, sel4::Error> {
        let irq_handler =
            sel4::CPtr::from_bits(cptr.try_into().unwrap()).cast::<sel4::cap_type::IrqHandler>();

        if irq >= 16 && irq < 32 {
            self.irq_control_get_ppi(irq_control, irq, edge_triggered, irq_handler)
                .expect("@alwin: Should this be an assert?");
        } else if irq >= 32 && irq < 1021 {
            irq_control
                .irq_control_get_trigger(
                    irq.try_into().unwrap(),
                    edge_triggered.into(),
                    &self.root_cnode.absolute_cptr(irq_handler),
                )
                .or(Err(sel4::Error::InvalidArgument))?;
        } else {
            warn_rs!("Passed in an unsupported IRQ number");
            return Err(sel4::Error::InvalidArgument);
        }

        return Ok(irq_handler);
    }

    pub fn init_bot_lvl_node(self: &mut Self, i: usize, ptr: *mut BotLvlNodeT) {
        unsafe {
            // cast to u8 for memset equivalent
            core::ptr::write_bytes(ptr as *mut u8, 0, PAGE_SIZE_4K);
            self.bot_lvl_nodes[i] = ptr;
        }
    }
}
