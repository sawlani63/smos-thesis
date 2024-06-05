#![allow(non_snake_case)]

use sel4::ObjectBlueprint;
use crate::err_rs;
use crate::cspace::{CSpace, CSpaceTrait};
use crate::ut::{UTTable, UTWrapper};

pub const fn ALIGN_DOWN(x : usize, n : usize) -> usize {
    return x & !(n - 1);
}

pub const fn ALIGN_UP(x: usize, n: usize) -> usize {
    (x + n - 1) & !(n - 1)
}

const fn BIT(n : usize) -> usize {
    1 << n
}

pub const fn MASK(n: usize) -> usize {
    BIT(n) - 1
}

pub fn alloc_retype<T: sel4::CapType>(cspace: &mut CSpace, ut_table: &mut UTTable, blueprint: ObjectBlueprint) -> Result<(sel4::Cap<T>, UTWrapper), sel4::Error> {
    let ut = ut_table.alloc(cspace, blueprint.physical_size_bits()).map_err(|_| {
        err_rs!("No memory for object of size {}", blueprint.physical_size_bits());
        sel4::Error::NotEnoughMemory
    })?;

    let cptr = cspace.alloc_slot().map_err(|_| {
        err_rs!("Failed to allocate slot");
        ut_table.free(ut);
        sel4::Error::InvalidCapability
    })?;

    cspace.untyped_retype(&ut.get_cap(), blueprint, cptr).map_err(|_| {
        err_rs!("Failed to retype untyped");
        ut_table.free(ut);
        cspace.free_slot(cptr);
        sel4::Error::IllegalOperation
    })?;

    return Ok((sel4::CPtr::from_bits(cptr.try_into().unwrap()).cast::<T>(), ut));
}

pub fn dealloc_retyped<T: sel4::CapType>(cspace: &mut CSpace, ut_table: &mut UTTable, alloc: (sel4::Cap<T>, UTWrapper)) {
    cspace.delete(alloc.0.bits().try_into().unwrap());
    cspace.free_slot(alloc.0.bits().try_into().unwrap());
    ut_table.free(alloc.1);
}


/* We set the top bit to differentiate between messages from notifications (IRQs) and EPs */
pub const IRQ_EP_BIT: usize = BIT(sel4_sys::seL4_BadgeBits as usize - 1);

/* If we have a notification, we use the remaining 63 bits to differentiate between them */
pub const IRQ_IDENT_BADGE_BITS: usize = IRQ_EP_BIT - 1;

/* If we have an endpoint, we use the 2nd and 3rd top bits to determine if it was the result of a
 * fault, a boot file file server invocation, or root server invocation. */
const INVOCATION_VALUE: usize = 0x0;
const FAULT_VALUE: usize = 0x1;
const BFS_VALUE: usize = 0x2;

pub const INVOCATION_EP_BITS: usize = INVOCATION_VALUE << 1;
pub const FAULT_EP_BITS: usize = FAULT_VALUE << 1;
pub const BFS_EP_BITS: usize = BFS_VALUE << 1;

pub enum EntryType {
    RSInvocation(usize),
    BFSInvocation(usize),
    Fault(usize),
    Irq,
}

pub fn decode_entry_type(badge: usize) -> EntryType {
    if badge & IRQ_EP_BIT != 0 {
        return EntryType::Irq;
    }

    let pid = badge & !(0x7);
    match ((badge >> 0x1) & 0x3) {
        INVOCATION_VALUE => EntryType::RSInvocation(pid),
        FAULT_VALUE => EntryType::Fault(pid),
        BFS_VALUE => EntryType::BFSInvocation(pid),
        _ => panic!("An unexpected endpoint was invoked!"),
    }
}



