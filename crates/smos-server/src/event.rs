use alloc::vec::Vec;
use smos_common::util::BIT;

/* We set the top bit to differentiate between messages from notifications  and EPs */
pub const NTFN_BIT: usize = 0x1 << 63;

/* If we have a notification, we use the remaining 63 bits to differentiate between them */
pub const IRQ_IDENT_BADGE_BITS: usize = BIT(63) - 1;

/* If we have an endpoint, we use the 2nd and 3rd top bits to determine if it was the result of a
 * fault, a boot file file server invocation, or root server invocation. */
const EP_BIT: usize = 0x0 << 63;
const EP_TYPE_SHIFT: usize = 62;

const INVOCATION_VALUE: usize = 0x0;
const FAULT_VALUE: usize = 0x1;

pub const INVOCATION_EP_BITS: usize = EP_BIT | INVOCATION_VALUE << EP_TYPE_SHIFT;
pub const FAULT_EP_BITS: usize = EP_BIT | FAULT_VALUE << EP_TYPE_SHIFT;

pub enum EntryType {
    Invocation(usize),
    Fault(usize),
    Notification(NtfnWord),
}

pub struct NtfnWord(usize);

impl NtfnWord {
    pub fn from_inner(inner: usize) -> Self {
        return Self { 0: inner };
    }

    pub fn into_inner(self) -> usize {
        return self.0;
    }

    pub fn inner(&self) -> &usize {
        return &self.0;
    }

    pub fn inner_mut(&mut self) -> &mut usize {
        return &mut self.0;
    }
}

pub struct NtfnWordIterator(usize);

impl Iterator for NtfnWordIterator {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        if self.0 == 0 {
            return None;
        }

        let bit = self.0.trailing_zeros().try_into().unwrap();
        self.0 &= !BIT(bit);
        return Some(bit);
    }
}

impl IntoIterator for NtfnWord {
    type Item = usize;
    type IntoIter = NtfnWordIterator;

    fn into_iter(self) -> Self::IntoIter {
        return NtfnWordIterator { 0: self.0 };
    }
}

pub fn decode_entry_type(badge: usize) -> EntryType {
    if badge & NTFN_BIT != 0 {
        return EntryType::Notification(NtfnWord::from_inner(badge & !NTFN_BIT));
    }

    let pid = badge & !(0x3 << EP_TYPE_SHIFT);
    match ((badge >> EP_TYPE_SHIFT) & 0x1) {
        INVOCATION_VALUE => EntryType::Invocation(pid),
        FAULT_VALUE => EntryType::Fault(pid),
        _ => panic!("An unexpected endpoint capability was invoked!"),
    }
}
