const fn BIT(n : usize) -> usize {
    1 << n
}

/* We set the top bit to differentiate between messages from notifications  and EPs */
// pub const IRQ_EP_BIT: usize = BIT(sel4_sys::seL4_BadgeBits as usize - 1);
const NTFN_BIT: usize = 0x1 << 63;
pub const NTFN_IRQ_BITS: usize = NTFN_BIT | 0 << 62;
pub const NTFN_SIGNAL_BITS: usize = NTFN_BIT | 1 << 62;

const IRQ_VALUE: usize = 0x0;
const SIGNAL_VALUE: usize = 0x1;

/* If we have a notification, we use the remaining 62 bits to differentiate between them */
pub const IRQ_IDENT_BADGE_BITS: usize = BIT(62) - 1;

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
    Irq,
    Signal,
}

pub fn decode_entry_type(badge: usize) -> EntryType {
    if badge & NTFN_BIT != 0 {
        return match ((badge >> EP_TYPE_SHIFT) & 0x1) {
            IRQ_VALUE => EntryType::Irq,
            SIGNAL_VALUE => EntryType::Signal,
            _ => panic!("An unexpected notification capability was invoked"),
        }
    }

    let pid = badge & !(0x3 << EP_TYPE_SHIFT);
    match ((badge >> EP_TYPE_SHIFT) & 0x1) {
        INVOCATION_VALUE => EntryType::Invocation(pid),
        FAULT_VALUE => EntryType::Fault(pid),
        _ => panic!("An unexpected endpoint capability was invoked!"),
    }
}