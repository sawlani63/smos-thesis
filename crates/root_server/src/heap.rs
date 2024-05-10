
// @alwin: For verifiablility, maybe we should write our own heap.
// For now, we don't.

use linked_list_allocator::LockedHeap;
use crate::mapping::map_frame;
use crate::cspace::CSpace;
use crate::page::PAGE_SIZE_4K;
use crate::ut::UTTable;
use crate::util::alloc_retype;
use crate::vmem_layout::{HEAP, HEAP_PAGES};

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();


pub fn initialise_heap(cspace: &mut CSpace, ut_table: &mut UTTable) -> Result<(), sel4::Error> {
    let mut vaddr = HEAP;
    for _ in 0..HEAP_PAGES {
        let (frame, _) = alloc_retype::<sel4::cap_type::SmallPage>(cspace, ut_table,
                                                                  sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage))?;
        map_frame(cspace, ut_table, frame.cast(), sel4::init_thread::slot::VSPACE.cap(), vaddr,
                  sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None)?;
        vaddr += PAGE_SIZE_4K;
    }
    unsafe {
        ALLOCATOR.lock().init(HEAP as *mut u8, HEAP_PAGES * PAGE_SIZE_4K);
    }

    return Ok(());
}