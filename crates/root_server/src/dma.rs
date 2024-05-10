use crate::mapping::map_frame;
use crate::cspace::CSpace;
use crate::ut::UTTable;
use crate::page::BIT;

pub struct DMA {
    vstart: usize,
    pstart: usize,
    pnext: usize,
    pend: usize,
    page: sel4::cap::LargePage,
    vspace: sel4::cap::VSpace
}

impl DMA {
    pub fn new(cspace: &mut CSpace, ut_table: &mut UTTable, vspace: sel4::cap::VSpace, page: sel4::cap::LargePage,
                pstart: usize, vstart: usize) -> Result<Self, sel4::Error> {
        let dma = DMA {
            pstart: pstart,
            vstart: vstart,
            pend: pstart + BIT(sel4_sys::seL4_LargePageBits.try_into().unwrap()),
            pnext: pstart,
            vspace: vspace,
            page: page
        };

        let vaddr = dma.phys_to_virt(dma.pstart);
        let _ = map_frame(cspace, ut_table, dma.page.cast(), dma.vspace, vaddr,
                          sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None)?;
        return Ok(dma);
    }

    fn phys_to_virt(self: &Self, phys: usize) -> usize {
        return self.vstart + (phys - self.pstart);
    }
}
