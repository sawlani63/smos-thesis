use crate::cspace::{
    BotLvlNodeT, CSpace, CSpaceTrait, BOT_LVL_INDEX, BOT_LVL_PER_NODE, CNODE_INDEX,
    CNODE_SIZE_BITS, CNODE_SLOTS, CNODE_SLOT_BITS, NODE_INDEX, TOP_LVL_INDEX, WATERMARK_SLOTS,
};
use crate::dma::{DMAPool, DMA_RESERVATION_NUM_PAGES, DMA_RESERVATION_SIZE_BITS};
use crate::page::{BYTES_TO_4K_PAGES, BYTES_TO_SIZE_BITS, PAGE_SIZE_4K};
use crate::ut::{UTRegion, UTTable, UT};
use crate::util::{ALIGN_DOWN, ALIGN_UP};
use crate::vmem_layout::UT_TABLE;
use bitfield::bf_set_bit;
use core::mem::size_of;
use core::ptr::addr_of_mut;
use sel4::CPtr;
use smos_common::util::BIT;
use smos_common::util::ROUND_UP;

use sel4_config::sel4_cfg_usize;

const PHYSICAL_ADDRESS_LIMIT: usize = 0xdfffffff;
const MAX_PHYSICAL_SIZE_BITS: usize = 32;
pub const INITIAL_TASK_CNODE_SIZE_BITS: usize = 18;
pub const INITIAL_TASK_CSPACE_BITS: usize =
    CNODE_SLOT_BITS(INITIAL_TASK_CNODE_SIZE_BITS) + CNODE_SLOT_BITS(CNODE_SIZE_BITS);
pub const INITIAL_TASK_CSPACE_SLOTS: usize = BIT(INITIAL_TASK_CSPACE_BITS);

static mut BOT_LVL_NODES: [*mut BotLvlNodeT; INITIAL_TASK_CSPACE_SLOTS / BOT_LVL_PER_NODE + 1] =
    [core::ptr::null_mut(); INITIAL_TASK_CSPACE_SLOTS / BOT_LVL_PER_NODE + 1];

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
    BYTES_TO_4K_PAGES(((memory.end - memory.start) / PAGE_SIZE_4K) * size_of::<UT>())
}

fn calculate_ut_caps(
    bi: &sel4::BootInfo,
    size_bits: u32,
    bootinfo_avail_bytes: &mut [usize],
) -> usize {
    let mut n_caps: usize = 0;
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

fn paddr_from_avail_bytes(
    bi: &sel4::BootInfo,
    i: usize,
    size_bits: usize,
    bootinfo_avail_bytes: &[usize],
) -> usize {
    let mut taken: usize = 0;
    if !bi.untyped_list()[i].is_device() {
        taken = BIT(bi.untyped_list()[i].size_bits()) - bootinfo_avail_bytes[i];
    }

    taken = ROUND_UP(taken, size_bits);
    return bi.untyped_list()[i].paddr() + taken;
}

fn steal_untyped(
    bi: &sel4::BootInfo,
    size_bits: usize,
    bootinfo_avail_bytes: &mut [usize],
) -> Option<(sel4::Cap<sel4::cap_type::Untyped>, usize)> {
    assert!(size_bits >= sel4_sys::seL4_PageBits as usize);
    assert!(size_bits <= sel4_sys::seL4_MaxUntypedBits as usize);

    for (i, untyped) in bi.untyped_list().iter().enumerate() {
        if untyped_in_range(untyped) && bootinfo_avail_bytes[i] >= BIT(size_bits) {
            let paddr = paddr_from_avail_bytes(bi, i, size_bits, bootinfo_avail_bytes);
            bootinfo_avail_bytes[i] -= BIT(size_bits);
            return Some((
                sel4::CPtr::from_bits((i + bi.untyped().start()).try_into().unwrap())
                    .cast::<sel4::cap_type::Untyped>(),
                paddr,
            ));
        }
    }

    return None;
}

struct BootstrapCSpace {
    next_free_vaddr: usize,
    vspace: sel4::cap::VSpace,
}

// pub fn smos_bootstrap(bi: &sel4::BootInfo) -> CSpace{
#[allow(unused_assignments)]
pub fn smos_bootstrap(bi: &sel4::BootInfo) -> Result<(CSpace, UTTable, DMAPool), sel4::Error> {
    let mut bootinfo_avail_bytes: [usize; sel4_cfg_usize!(MAX_NUM_BOOTINFO_UNTYPED_CAPS)] =
        [0; sel4_cfg_usize!(MAX_NUM_BOOTINFO_UNTYPED_CAPS)];

    let mut bootstrap_data = BootstrapCSpace {
        next_free_vaddr: UT_TABLE,
        vspace: sel4::init_thread::slot::VSPACE.cap(),
    };

    // The initial CNode
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
    let mut size: usize = ut_pages * PAGE_SIZE_4K;

    /* account for the number of page tables we need - plus a buffer of 1 */
    let n_pts = (ut_pages >> sel4_sys::seL4_PageTableIndexBits) + 1;
    size += n_pts * BIT(sel4_sys::seL4_PageTableBits as usize);
    n_slots += n_pts;

    /* We will need a few more for the page tables of the DMA region */
    let n_dma_pts = DMA_RESERVATION_NUM_PAGES >> sel4_sys::seL4_PageTableIndexBits;
    size += n_dma_pts * BIT(sel4_sys::seL4_PageTableBits as usize);
    n_slots += n_dma_pts;

    /* and the other paging structures */
    size += BIT(sel4_sys::seL4_PUDBits as usize);
    size += BIT(sel4_sys::seL4_PageDirBits as usize);
    n_slots += 2;

    /* NUM_PAGES cptrs for dma */
    n_slots += DMA_RESERVATION_NUM_PAGES;

    /* now work out the number of slots required to retype the untyped memory provided by
     * boot info into 4K untyped objects. We aren't going to initialise these objects yet,
     * but before we have bootstrapped the frame table we cannot allocate memory from it --
     * to avoid this circular dependency we create enough cnodes here to cover our initial
     * requirements, up until the frame table is created*/
    n_slots += calculate_ut_caps(bi, sel4_sys::seL4_PageBits, &mut bootinfo_avail_bytes);

    /* subtract what we don't need for dma */
    n_slots -= BIT((DMA_RESERVATION_SIZE_BITS - sel4_sys::seL4_PageBits) as usize);

    /* now work out how many 2nd level nodes are required - with a buffer */
    let n_cnodes = n_slots / CNODE_SLOTS(CNODE_SIZE_BITS) + 2;
    size += (n_cnodes * BIT(CNODE_SIZE_BITS)) + BIT(INITIAL_TASK_CNODE_SIZE_BITS);

    let (ut, _) = steal_untyped(bi, BYTES_TO_SIZE_BITS(size) + 1, &mut bootinfo_avail_bytes)
        .expect("Not enough memory");

    /* create the new level 1 cnode from the untyped we found */
    let mut blueprint = sel4::ObjectBlueprint::CNode {
        size_bits: CNODE_SLOT_BITS(INITIAL_TASK_CNODE_SIZE_BITS),
    };
    ut.untyped_retype(
        &blueprint,
        &sel4::init_thread::slot::CNODE
            .cap()
            .absolute_cptr_for_self(),
        lvl1_cptr,
        1,
    )
    .expect("Could not create new top-level CNode");
    let mut lvl1_cnode_cptr =
        init_task_cnode.absolute_cptr_from_bits_with_depth(lvl1_cptr as u64, sel4::WORD_SIZE);
    let mut lvl1_cnode =
        CPtr::from_bits(lvl1_cnode_cptr.path().bits()).cast::<sel4::cap_type::CNode>();

    /* now create the 2nd level nodes, directly in the node we just created */
    let mut chunk: usize;
    let mut total: usize = n_cnodes;
    blueprint = sel4::ObjectBlueprint::CNode {
        size_bits: CNODE_SLOT_BITS(CNODE_SIZE_BITS),
    };

    while total > 0 {
        chunk = usize::min(sel4_cfg_usize!(RETYPE_FAN_OUT_LIMIT), total);
        ut.untyped_retype(&blueprint, &lvl1_cnode_cptr, n_cnodes - total, chunk)?;
        total -= chunk;
    }
    let depth: usize =
        CNODE_SLOT_BITS(INITIAL_TASK_CNODE_SIZE_BITS) + CNODE_SLOT_BITS(CNODE_SIZE_BITS);

    /* copy the old root cnode to cptr 0 in the new cspace */
    let init_task_cnode_self = init_task_cnode.absolute_cptr_from_bits_with_depth(
        sel4_sys::seL4_RootCNodeCapSlots::seL4_CapInitThreadCNode.into(),
        sel4::WORD_SIZE,
    );
    let init_task_cnode_copy = lvl1_cnode.absolute_cptr_from_bits_with_depth(boot_cptr, depth);
    init_task_cnode_copy.copy(&init_task_cnode_self, sel4::CapRightsBuilder::all().build())?;

    /* mint a cap to our new cnode at seL4_CapInitThreadCnode in the new cspace with the correct guard */
    let cap_data = sel4::CNodeCapData::new(0, sel4::WORD_SIZE - depth);
    let lvl1_self_cptr = lvl1_cnode.absolute_cptr_from_bits_with_depth(
        sel4_sys::seL4_RootCNodeCapSlots::seL4_CapInitThreadCNode.into(),
        depth,
    );
    lvl1_self_cptr.mint(
        &lvl1_cnode_cptr,
        sel4::CapRightsBuilder::all().build(),
        (sel4::WORD_SIZE - depth).try_into().unwrap(),
    )?;

    /* Set the new CNode as our default top-level CNode */
    sel4::init_thread::slot::TCB
        .cap()
        .tcb_set_space(
            CPtr::from_bits(0),
            lvl1_cnode,
            cap_data,
            sel4::init_thread::slot::VSPACE.cap(),
        )
        .expect("Failed to set CSpace of root task");

    /* Redefine the CPtrs's relative to the new top-level CSpace */
    lvl1_cnode_cptr = sel4::init_thread::slot::CNODE
        .cap()
        .absolute_cptr_for_self();
    lvl1_cnode = sel4::init_thread::slot::CNODE.cap();
    let init_task_cnode_cptr =
        lvl1_cnode.absolute_cptr_from_bits_with_depth(boot_cptr, sel4::WORD_SIZE);
    init_task_cnode =
        CPtr::from_bits(init_task_cnode_cptr.path().bits()).cast::<sel4::cap_type::CNode>();

    /* Copy capabilities over from the initial cspace to the new cspace */
    for i in 1..bi.empty().start() {
        match i.try_into().unwrap() {
            sel4_sys::seL4_RootCNodeCapSlots::seL4_CapInitThreadCNode
            | sel4_sys::seL4_RootCNodeCapSlots::seL4_CapIOPortControl
            | sel4_sys::seL4_RootCNodeCapSlots::seL4_CapIOSpace
            | sel4_sys::seL4_RootCNodeCapSlots::seL4_CapSMMUSIDControl
            | sel4_sys::seL4_RootCNodeCapSlots::seL4_CapSMMUCBControl
            | sel4_sys::seL4_RootCNodeCapSlots::seL4_CapSMC => {
                continue;
            }
            _ => {}
        }

        if let Err(e) = lvl1_cnode
            .absolute_cptr_from_bits_with_depth(i.try_into().unwrap(), sel4::WORD_SIZE)
            .move_(
                &init_task_cnode
                    .absolute_cptr_from_bits_with_depth(i.try_into().unwrap(), sel4::WORD_SIZE),
            )
        {
            sel4::debug_println!("When copying capability {} - Encountered error {}", i, e);
        }
    }

    /* Remove the original cnode -- it's empty and we need slot 0 to be free as it acts
     * as the NULL capability and should be empty, or any invocation of seL4_CapNull will
     * invoke this cnode.
     */
    init_task_cnode_cptr.delete()?;

    /* Next, allocate and map enough paging structures and frames to create the
     * untyped table */

    /* Get the first free slot in the vspace  */
    let mut first_free_slot = bi.empty().start();

    let mut cspace = unsafe {
        CSpace::new(
            lvl1_cnode,
            true,
            INITIAL_TASK_CNODE_SIZE_BITS,
            addr_of_mut!(BOT_LVL_NODES).as_mut().unwrap(),
            None, /* alloc */
        )
    };

    /* Allocate the PUD */
    cspace.untyped_retype(
        &ut,
        sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::PT),
        first_free_slot,
    )?;

    /* Map the PUD */
    let pud = CPtr::from_bits(first_free_slot.try_into().unwrap()).cast::<sel4::cap_type::PT>();
    pud.pt_map(
        sel4::init_thread::slot::VSPACE.cap(),
        UT_TABLE,
        sel4::VmAttributes::DEFAULT,
    )?;
    first_free_slot += 1;

    /* Now allocate the PD */
    cspace.untyped_retype(
        &ut,
        sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::PT),
        first_free_slot,
    )?;

    /* Map the PD */
    let pd = CPtr::from_bits(first_free_slot.try_into().unwrap()).cast::<sel4::cap_type::PT>();
    pd.pt_map(
        sel4::init_thread::slot::VSPACE.cap(),
        UT_TABLE,
        sel4::VmAttributes::DEFAULT,
    )?;
    first_free_slot += 1;

    /* Now the PTs */
    for i in 0..((ut_pages >> sel4_sys::seL4_PageTableIndexBits) + 1) {
        cspace.untyped_retype(
            &ut,
            sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::PT),
            first_free_slot,
        )?;
        let vaddr = UT_TABLE
            + i * (BIT(
                (sel4_sys::seL4_PageTableIndexBits + sel4_sys::seL4_PageBits)
                    .try_into()
                    .unwrap(),
            ));

        let pt = CPtr::from_bits(first_free_slot.try_into().unwrap()).cast::<sel4::cap_type::PT>();
        pt.pt_map(
            sel4::init_thread::slot::VSPACE.cap(),
            vaddr,
            sel4::VmAttributes::DEFAULT,
        )
        .expect("Failed to map page table into RS address space");
        first_free_slot += 1;
    }

    /* and pages to cover the UT table */
    let slots_per_cnode = CNODE_SLOTS(CNODE_SIZE_BITS);
    for _i in 0..ut_pages {
        cspace.untyped_retype(
            &ut,
            sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage),
            first_free_slot,
        )?;
        let page = CPtr::from_bits(first_free_slot.try_into().unwrap())
            .cast::<sel4::cap_type::SmallPage>();
        page.frame_map(
            sel4::init_thread::slot::VSPACE.cap(),
            bootstrap_data.next_free_vaddr,
            sel4::CapRightsBuilder::all().build(),
            sel4::VmAttributes::DEFAULT,
        )?;
        first_free_slot += 1;
        bootstrap_data.next_free_vaddr += PAGE_SIZE_4K;
    }

    /* before we add all the 4k untypeds to the ut table, steal 64MB that can be used for DMA */
    let (dma_ut, dma_paddr) = steal_untyped(
        bi,
        DMA_RESERVATION_SIZE_BITS.try_into().unwrap(),
        &mut bootinfo_avail_bytes,
    )
    .ok_or(sel4::Error::NotEnoughMemory)?;

    let mut dma_pages: [sel4::cap::UnspecifiedPage; DMA_RESERVATION_NUM_PAGES] =
        [CPtr::from_bits(0).cast(); DMA_RESERVATION_NUM_PAGES];
    for i in 0..DMA_RESERVATION_NUM_PAGES {
        cspace.untyped_retype(
            &dma_ut,
            sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage),
            first_free_slot,
        )?;
        dma_pages[i] = CPtr::from_bits(first_free_slot.try_into().unwrap())
            .cast::<sel4::cap_type::UnspecifiedPage>();
        first_free_slot += 1;
    }

    let mut ut_table = UTTable::new(UT_TABLE, memory, bootstrap_data.next_free_vaddr);

    for (i, untyped) in bi.untyped_list().iter().enumerate() {
        if !untyped_in_range(untyped) {
            continue;
        }

        let mut n_caps = BIT(untyped.size_bits()) / PAGE_SIZE_4K;
        if !untyped.is_device() {
            n_caps = bootinfo_avail_bytes[i] / PAGE_SIZE_4K;
        }

        let paddr = paddr_from_avail_bytes(
            bi,
            i,
            sel4_sys::seL4_PageBits.try_into().unwrap(),
            &bootinfo_avail_bytes,
        );
        if n_caps > 0 {
            ut_table.add_untyped_range(paddr, first_free_slot, n_caps, untyped.is_device())
        }
        while n_caps > 0 {
            let cnode = first_free_slot / slots_per_cnode;
            //     /* we can only retype the amount that will fit in a 2nd lvl cnode */
            let retype = usize::min(
                sel4_cfg_usize!(RETYPE_FAN_OUT_LIMIT),
                usize::min(n_caps, slots_per_cnode - first_free_slot % slots_per_cnode),
            );

            let this_ut = CPtr::from_bits((bi.untyped().start() + i).try_into().unwrap())
                .cast::<sel4::cap_type::Untyped>();
            let blueprint = sel4::ObjectBlueprint::Untyped {
                size_bits: sel4_sys::seL4_PageBits.try_into().unwrap(),
            };

            this_ut.untyped_retype(
                &blueprint,
                &lvl1_cnode.absolute_cptr_from_bits_with_depth(
                    cnode.try_into().unwrap(),
                    sel4::WORD_SIZE - CNODE_SLOT_BITS(CNODE_SIZE_BITS),
                ),
                first_free_slot % CNODE_SLOTS(CNODE_SIZE_BITS),
                retype,
            )?;
            first_free_slot += retype;
            n_caps -= retype;
        }
    }

    let n_bot_lvl =
        usize::max(first_free_slot / slots_per_cnode + 1, n_cnodes) / BOT_LVL_PER_NODE + 1;
    for i in 0..n_bot_lvl {
        let (_, node_ut) = ut_table.alloc_4k_untyped()?;
        cspace.untyped_retype(
            &node_ut.get_cap(),
            sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage),
            first_free_slot,
        )?;
        let page = CPtr::from_bits(first_free_slot.try_into().unwrap())
            .cast::<sel4::cap_type::SmallPage>();
        page.frame_map(
            sel4::init_thread::slot::VSPACE.cap(),
            ut_table.next_free_vaddr,
            sel4::CapRightsBuilder::all().build(),
            sel4::VmAttributes::DEFAULT,
        )?;
        cspace.init_bot_lvl_node(i, ut_table.next_free_vaddr as *mut BotLvlNodeT);
        ut_table.next_free_vaddr += PAGE_SIZE_4K;

        let bot_lvl_node = unsafe { cspace.get_bot_lvl_node(i) };
        bot_lvl_node.untyped = node_ut;
        bot_lvl_node.frame = page;
        cspace.n_bot_lvl_nodes += 1;
        first_free_slot += 1;
    }

    let dma_vaddr = ALIGN_UP(
        ut_table.next_free_vaddr + PAGE_SIZE_4K,
        BIT(sel4_sys::seL4_LargePageBits.try_into().unwrap()),
    );

    /* Map in the extra page tables to cover the DMA region */
    // @alwin: Is this a hack? I don't THINK so?
    for i in 0..(DMA_RESERVATION_NUM_PAGES >> sel4_sys::seL4_PageTableIndexBits) {
        cspace.untyped_retype(
            &ut,
            sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::PT),
            first_free_slot,
        )?;
        let vaddr = dma_vaddr
            + i * (BIT(
                (sel4_sys::seL4_PageTableIndexBits + sel4_sys::seL4_PageBits)
                    .try_into()
                    .unwrap(),
            ));

        let pt = CPtr::from_bits(first_free_slot.try_into().unwrap()).cast::<sel4::cap_type::PT>();
        pt.pt_map(
            sel4::init_thread::slot::VSPACE.cap(),
            vaddr,
            sel4::VmAttributes::DEFAULT,
        )?;
        first_free_slot += 1;
    }

    let dma = DMAPool::new(
        &mut cspace,
        &mut ut_table,
        bootstrap_data.vspace,
        dma_ut,
        dma_pages,
        dma_paddr,
        dma_vaddr,
    )?;

    ut_table.next_free_vaddr = dma_vaddr + (DMA_RESERVATION_NUM_PAGES + 1) * PAGE_SIZE_4K;

    /* now record all the cptrs we have already used to bootstrap */
    for i in (0..ALIGN_DOWN(first_free_slot, slots_per_cnode)).step_by(slots_per_cnode) {
        let bot_lvl_node = unsafe { cspace.get_bot_lvl_node(NODE_INDEX(i)) };
        bot_lvl_node.n_cnodes += 1;
        for i in 0..CNODE_SLOTS(CNODE_SIZE_BITS) / sel4::WORD_SIZE {
            bot_lvl_node.cnodes[CNODE_INDEX(i)].bf[i] = u64::MAX;
        }
        /* this cnode is full */
        bf_set_bit(&mut cspace.top_bf, TOP_LVL_INDEX(i));
    }

    let bot_lvl_node =
        unsafe { cspace.get_bot_lvl_node(first_free_slot / slots_per_cnode / BOT_LVL_PER_NODE) };
    bot_lvl_node.n_cnodes += 1;

    for i in ALIGN_DOWN(first_free_slot, slots_per_cnode)..first_free_slot {
        bf_set_bit(
            &mut bot_lvl_node.cnodes[CNODE_INDEX(i)].bf,
            BOT_LVL_INDEX(i),
        );
    }

    for i in (first_free_slot / slots_per_cnode + 1)..(n_cnodes) {
        unsafe {
            cspace.get_bot_lvl_node(i / BOT_LVL_PER_NODE).n_cnodes += 1;
        }
    }

    for i in 0..WATERMARK_SLOTS {
        cspace.watermark[i] = cspace.alloc_slot()?;
    }

    return Ok((cspace, ut_table, dma));
}
