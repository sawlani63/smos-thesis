use core::mem::size_of;
use crate::page::{PAGE_BITS_4K, PAGE_SIZE_4K, BIT, BYTES_TO_SIZE_BITS_PAGES, BYTES_TO_4K_PAGES, BYTES_TO_SIZE_BITS};
// use crate::cspace::{CSpace, CNODE_SLOTS, CNODE_SIZE_BITS, CNODE_SLOT_BITS};
use crate::cspace::{CNODE_SLOTS, CNODE_SIZE_BITS, CNODE_SLOT_BITS};
use crate::ut::{UT, UTRegion};
use crate::arith::{ROUND_UP};
use sel4::CPtr;

use sel4_config::sel4_cfg_usize;

const SOS_DMA_SIZE_BITS: u32 =  sel4_sys::seL4_LargePageBits;
const PHYSICAL_ADDRESS_LIMIT: usize = 0xdfffffff;
const MAX_PHYSICAL_SIZE_BITS: usize = 32;
const INITIAL_TASK_CNODE_SIZE_BITS: usize = 18;

fn untyped_in_range(untyped: &sel4::UntypedDesc) -> bool {
    untyped.paddr() <= PHYSICAL_ADDRESS_LIMIT && untyped.size_bits() <= MAX_PHYSICAL_SIZE_BITS
}

fn find_memory_bounds(bi: &sel4::BootInfo) -> UTRegion {
	let mut memory = UTRegion::new(PHYSICAL_ADDRESS_LIMIT, 0);

	for untyped in bi.untyped_list() {
		if !untyped_in_range(untyped) {
			continue;
		}

		memory.start = usize::min(memory.start, untyped.paddr());
		memory.end = usize::max(memory.end, untyped.paddr() + BIT(untyped.size_bits()));
	}

	assert!(memory.end > memory.start);
	return memory;
}

fn ut_pages_for_region(memory: &UTRegion) -> usize {
    BYTES_TO_4K_PAGES((memory.end - memory.start) / PAGE_SIZE_4K * size_of::<UT>())
}

fn calculate_ut_caps(bi : &sel4::BootInfo, size_bits: u32, bootinfo_avail_bytes: &mut [usize]) -> usize {
	let mut n_caps : usize = 0;
	for (i, untyped) in bi.untyped_list().iter().enumerate() {
		if !untyped_in_range(untyped) {
			continue;
		}

		if !untyped.is_device() {
			bootinfo_avail_bytes[i] += BIT(untyped.size_bits());
		}

		if untyped.size_bits() >= size_bits as usize {
			n_caps += BIT(untyped.size_bits() - size_bits as usize);
		}
	}

	return n_caps;
}

fn paddr_from_avail_bytes(bi: &sel4::BootInfo, i: usize, size_bits: usize, bootinfo_avail_bytes: &[usize]) -> usize {
	let mut taken: usize = 0;
	if !bi.untyped_list()[i].is_device() {
        taken = BIT(bi.untyped_list()[i].size_bits()) - bootinfo_avail_bytes[i];
	}
	taken = ROUND_UP(taken, BIT(size_bits));
	return bi.untyped_list()[i].paddr() + taken;
}

fn steal_untyped(bi: &sel4::BootInfo, size_bits: usize, bootinfo_avail_bytes: &mut [usize]) -> Option<(sel4::Cap<sel4::cap_type::Untyped>, usize)> {
	assert!(size_bits >= sel4_sys::seL4_PageBits as usize);
	assert!(size_bits <= sel4_sys::seL4_MaxUntypedBits as usize);


	for (i, untyped) in bi.untyped_list().iter().enumerate() {
		if untyped_in_range(untyped) && bootinfo_avail_bytes[i] >= BIT(size_bits) {
			let paddr = paddr_from_avail_bytes(bi, i, size_bits, bootinfo_avail_bytes);
			bootinfo_avail_bytes[i] -= BIT(size_bits);
			return Some((sel4::CPtr::from_bits((i + bi.untyped().start()).try_into().unwrap()).cast::<sel4::cap_type::Untyped>(), paddr));
		}
	}

	return None;
}

// pub fn smos_bootstrap(bi: &sel4::BootInfo) -> CSpace{
pub fn smos_bootstrap(bi: &sel4::BootInfo) -> Result<(), sel4::Error>{
	// let mut cspace = CSpace::new();
	let mut bootinfo_avail_bytes : [usize; sel4_cfg_usize!(MAX_NUM_BOOTINFO_UNTYPED_CAPS) ] = [0; sel4_cfg_usize!(MAX_NUM_BOOTINFO_UNTYPED_CAPS)];

    /* this cspace is bootstrapping itself */
	// cspace.bootstrap = None;

	// The initial CNode
	let mut init_task_cnode_cptr = sel4::init_thread::slot::CNODE.cap().relative_self();
    let mut init_task_cnode = sel4::init_thread::slot::CNODE.cap();

    /* use three slots from the current boot cspace */
	assert!(bi.empty().end() - bi.empty().start() >= 2);

	// This is where the new level on CNode will go
	let lvl1_cptr = bi.empty().start();
    /* We will temporarily store the boot cptr here, and remove it before we finish */
	let boot_cptr = 0;

    /* work out the number of slots used by the cspace we are provided on on init */
	let mut n_slots: usize = bi.empty().start() - 1;

    /* we need enough memory to create and map the ut table - first all the frames */
	let memory = find_memory_bounds(bi);
	let ut_pages = ut_pages_for_region(&memory);
	n_slots += ut_pages;
    /* track how much memory we need here */
	let mut size : usize = ut_pages * PAGE_SIZE_4K;

    /* account for the number of page tables we need - plus a buffer of 1 */
	let n_pts = (ut_pages >> sel4_sys::seL4_PageTableIndexBits) + 1;
	size += n_pts * BIT(sel4_sys::seL4_PageTableBits as usize);
	n_slots += n_pts;

    /* and the other paging structures */
    size += BIT(sel4_sys::seL4_PUDBits as usize);
    size += BIT(sel4_sys::seL4_PageDirBits as usize);
    n_slots += 2;

    /* 1 cptr for dma */
    n_slots += 1;

    /* now work out the number of slots required to retype the untyped memory provided by
     * boot info into 4K untyped objects. We aren't going to initialise these objects yet,
     * but before we have bootstrapped the frame table we cannot allocate memory from it --
     * to avoid this circular dependency we create enough cnodes here to cover our initial
     * requirements, up until the frame table is created*/
    n_slots += calculate_ut_caps(bi, sel4_sys::seL4_PageBits, &mut bootinfo_avail_bytes);

    /* subtract what we don't need for dma */
    n_slots -= BIT((SOS_DMA_SIZE_BITS - sel4_sys::seL4_PageBits) as usize);

    /* now work out how many 2nd level nodes are required - with a buffer */
    let n_cnodes = n_slots / CNODE_SLOTS + 2;
    size += (n_cnodes * BIT(CNODE_SIZE_BITS)) + BIT(INITIAL_TASK_CNODE_SIZE_BITS);

    let (ut_cptr, _) = steal_untyped(bi, BYTES_TO_SIZE_BITS(size) + 1, &mut bootinfo_avail_bytes).expect("Not enough memory");
    // cspace.root_cnode = sel4::BootInfo::init_thread_cnode();

    /* create the new level 1 cnode from the untyped we found */
    let mut blueprint = sel4::ObjectBlueprint::CNode{
    	size_bits: CNODE_SLOT_BITS(INITIAL_TASK_CNODE_SIZE_BITS)
    };
    ut_cptr.untyped_retype(&blueprint, &sel4::init_thread::slot::CNODE.cap().relative_self(),
    					   lvl1_cptr, 1).expect("Could not create new top-level CNode");
    let mut lvl1_cnode_cptr = init_task_cnode.relative_bits_with_depth(lvl1_cptr as u64, sel4::WORD_SIZE);
    let mut lvl1_cnode = CPtr::from_bits(lvl1_cnode_cptr.path().bits()).cast::<sel4::cap_type::CNode>();

    /* now create the 2nd level nodes, directly in the node we just created */
    let mut chunk: usize = 0;
    let mut total: usize = n_cnodes;
    blueprint = sel4::ObjectBlueprint::CNode{
    	size_bits: CNODE_SLOT_BITS(CNODE_SIZE_BITS),
    };

    while total > 0 {
    	chunk = usize::min(sel4_cfg_usize!(RETYPE_FAN_OUT_LIMIT), total);
    	ut_cptr.untyped_retype(&blueprint, &lvl1_cnode_cptr, n_cnodes - total, chunk)?;
    	total -= chunk;
    }
    let depth : usize = CNODE_SLOT_BITS(INITIAL_TASK_CNODE_SIZE_BITS) + CNODE_SLOT_BITS(CNODE_SIZE_BITS);

    /* copy the old root cnode to cptr 0 in the new cspace */
    let init_task_cnode_self = init_task_cnode.relative_bits_with_depth(sel4_sys::seL4_RootCNodeCapSlots::seL4_CapInitThreadCNode.into(), sel4::WORD_SIZE);
    let mut init_task_cnode_copy = lvl1_cnode.relative_bits_with_depth(boot_cptr, depth);
    init_task_cnode_copy.copy(&init_task_cnode_self, sel4::CapRightsBuilder::all().build())?;

    /* mint a cap to our new cnode at seL4_CapInitThreadCnode in the new cspace with the correct guard */
    let cap_data = sel4_sys::seL4_CNode_CapData::new(0, (sel4::WORD_SIZE - depth) as u64);
    let lvl1_self_cptr = lvl1_cnode.relative_bits_with_depth(sel4_sys::seL4_RootCNodeCapSlots::seL4_CapInitThreadCNode.into(), depth); // @alwin: This should be seL4_CapInitThreadCNode probably
    lvl1_self_cptr.mint(&lvl1_cnode_cptr, sel4::CapRightsBuilder::all().build(), cap_data.get_guardSize())?;

    /* Set the new CNode as our default top-level CNode */
    // @alwin: Does this really need to be unsafe?
    unsafe {
    	let x = (*bi.ipc_buffer()).inner_mut().seL4_TCB_SetSpace(sel4::init_thread::slot::TCB.cptr().bits(), 0, lvl1_cptr.try_into().unwrap(), cap_data.get_guardSize() , sel4::init_thread::slot::VSPACE.cptr().bits(), 0);
    }

    /* Redefine the CPtrs's relative to the new top-level CSpace */
    lvl1_cnode_cptr = sel4::init_thread::slot::CNODE.cap().relative_self();
    lvl1_cnode = sel4::init_thread::slot::CNODE.cap();
    init_task_cnode_cptr = lvl1_cnode.relative_bits_with_depth(boot_cptr, sel4::WORD_SIZE);
    init_task_cnode = CPtr::from_bits(init_task_cnode_cptr.path().bits()).cast::<sel4::cap_type::CNode>();;

    /* Copy capabilities over from the initial cspace to the new cspace */
    for i in 1..bi.empty().start() {
    	match i.try_into().unwrap() {
    	    sel4_sys::seL4_RootCNodeCapSlots::seL4_CapInitThreadCNode |
    	    sel4_sys::seL4_RootCNodeCapSlots::seL4_CapIOPortControl |
    	    sel4_sys::seL4_RootCNodeCapSlots::seL4_CapIOSpace |
    	    sel4_sys::seL4_RootCNodeCapSlots::seL4_CapSMMUSIDControl |
    	    sel4_sys::seL4_RootCNodeCapSlots::seL4_CapSMMUCBControl => {
    	    	continue;
    	    },
    	    _ => {}
    	}

    	let _ = lvl1_cnode.relative_bits_with_depth(i.try_into().unwrap(), sel4::WORD_SIZE).move_(&init_task_cnode.relative_bits_with_depth(i.try_into().unwrap(), sel4::WORD_SIZE));
    }

   	/* Remove the original cnode -- it's empty and we need slot 0 to be free as it acts
     * as the NULL capability and should be empty, or any invocation of seL4_CapNull will
     * invoke this cnode.
     */
  	init_task_cnode_cptr.delete();

    /* Next, allocate and map enough paging structures and frames to create the
     * untyped table */

    /* set the levels to 2 so we can use cspace_untyped_retype */
  	// cspace.two_level = true;

	/* allocate the PUD */
	let first_free_slot = bi.empty().start();
	blueprint = sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::PT);
	ut_cptr.untyped_retype(&blueprint, &lvl1_cnode_cptr, first_free_slot, 1)?;


	// @alwin: todo: This should return a CSpace
	return Ok(());
}