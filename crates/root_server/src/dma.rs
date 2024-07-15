use crate::log_rs;
use crate::mapping::map_frame;
use crate::page::PAGE_SIZE_4K;
use crate::ut::UTTable;
use crate::{cspace::CSpace, println};
use alloc::vec::Vec;
use offset_allocator::{Allocation, Allocator};
use smos_common::{error::InvocationError, util::BIT};

/* 64MB is reserved for DMA  */
pub const DMA_RESERVATION_SIZE_BITS: u32 = sel4_sys::seL4_LargePageBits + 5;
pub const DMA_RESERVATION_NUM_PAGES: usize = BIT(DMA_RESERVATION_SIZE_BITS as usize) / PAGE_SIZE_4K;

#[derive(Debug)]
pub struct DMAPool {
    vstart: usize,
    pstart: usize,
    pnext: usize,
    pend: usize,
    ut: sel4::cap::Untyped,
    pages: [sel4::cap::UnspecifiedFrame; DMA_RESERVATION_NUM_PAGES],
    vspace: sel4::cap::VSpace,
    pub allocation_table: Option<Allocator>,
}

impl DMAPool {
    pub fn new(
        cspace: &mut CSpace,
        ut_table: &mut UTTable,
        vspace: sel4::cap::VSpace,
        ut: sel4::cap::Untyped,
        pages: [sel4::cap::UnspecifiedFrame; DMA_RESERVATION_NUM_PAGES],
        pstart: usize,
        vstart: usize,
    ) -> Result<Self, sel4::Error> {
        let dma = DMAPool {
            pstart: pstart,
            vstart: vstart,
            pend: pstart + BIT(sel4_sys::seL4_LargePageBits.try_into().unwrap()),
            pnext: pstart,
            vspace: vspace,
            pages: pages,
            ut: ut,
            allocation_table: None,
        };

        for (i, page) in dma.pages.iter().enumerate() {
            let vaddr = dma.phys_to_virt(dma.pstart + PAGE_SIZE_4K * i);
            let _ = map_frame(
                cspace,
                ut_table,
                *page,
                dma.vspace,
                vaddr,
                sel4::CapRights::all(),
                sel4::VmAttributes::DEFAULT,
                None,
            )?;
        }

        return Ok(dma);
    }

    /* Call this after the heap has been set up */
    pub fn init(&mut self) {
        assert!(self.allocation_table.is_none());

        self.allocation_table = Some(Allocator::with_max_allocs(
            DMA_RESERVATION_NUM_PAGES.try_into().unwrap(),
            (DMA_RESERVATION_NUM_PAGES / 2).try_into().unwrap(), // The minimum allocation size we allow is 2 pages
        ));
    }

    fn phys_to_virt(self: &Self, phys: usize) -> usize {
        return self.vstart + (phys - self.pstart);
    }

    pub fn allocation_paddr(&self, alloc: &Allocation) -> usize {
        return self.pstart + (alloc.offset as usize) * PAGE_SIZE_4K;
    }

    pub fn allocate_contig_pages(
        &mut self,
        n_pages: u32,
    ) -> Result<(Allocation, Vec<sel4::cap::UnspecifiedFrame>), InvocationError> {
        assert!(self.allocation_table.is_some());

        let alloc = self
            .allocation_table
            .as_mut()
            .unwrap()
            .allocate(n_pages)
            .ok_or(InvocationError::InsufficientResources)?;

        let mut vec = Vec::new();
        for i in (alloc.offset as usize)..(alloc.offset as usize + n_pages as usize) {
            vec.push(self.pages[i]);
        }

        return Ok((alloc, vec));
    }
}
