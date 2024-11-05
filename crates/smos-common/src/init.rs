use core::marker::PhantomData;
use sel4::{cap_type, CPtr, CPtrBits, Cap, CapType};

const fn usize_into_word(x: usize) -> sel4::Word {
    x as sel4::Word
}

const fn u32_into_usize(x: u32) -> usize {
    x as usize
}

/* The capabilities that every application starts with */
#[rustfmt::skip]
#[allow(non_snake_case)]
#[allow(non_upper_case_globals)]
pub mod InitCNodeSlots {
    pub const SMOS_CapNull: u32            = 0;
    pub const SMOS_RootServerEP: u32       = 1;
    pub const SMOS_CNodeSelf: u32          = 2; // @alwin: add this
}

/// The index of a slot in the initial thread's root CNode.
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Slot<T: CapType = cap_type::Unspecified> {
    index: usize,
    _phantom: PhantomData<T>,
}

impl<T: CapType> Slot<T> {
    const fn from_sys(slot: u32) -> Self {
        Self::from_index(u32_into_usize(slot))
    }

    pub const fn from_index(index: usize) -> Self {
        Self {
            index,
            _phantom: PhantomData,
        }
    }

    pub const fn index(&self) -> usize {
        self.index
    }

    pub const fn cptr_bits(&self) -> CPtrBits {
        usize_into_word(self.index)
    }

    pub const fn cptr(&self) -> CPtr {
        CPtr::from_bits(self.cptr_bits())
    }

    pub const fn cap(&self) -> Cap<T> {
        self.cptr().cast()
    }

    pub const fn cast<T1: CapType>(&self) -> Slot<T1> {
        Slot::from_index(self.index)
    }

    pub const fn upcast(&self) -> Slot {
        self.cast()
    }
}

impl Slot {
    pub const fn downcast<T: CapType>(&self) -> Slot<T> {
        self.cast()
    }
}

pub mod slot {
    use super::{cap_type, Slot};

    macro_rules! mk {
        [
            $(
                $(#[$outer:meta])*
                ($name:ident, $cap_type:ident, $sys_name:ident),
            )*
        ] => {
            $(
                $(#[$outer])*
                pub const $name: Slot<cap_type::$cap_type> = Slot::from_sys($crate::init::InitCNodeSlots::$sys_name);
            )*
        };
    }

    mk![
        (NULL, Null, SMOS_CapNull),
        (CNODE, CNode, SMOS_CNodeSelf),
        (RS_EP, Endpoint, SMOS_RootServerEP),
    ];
}
