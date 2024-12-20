#![no_std]

use bitfield::{bf_clr_bit, bf_first_free, bf_set_bit, bitfield_init, bitfield_type};
use sel4::HasCPtrWithDepth;

const CSPACE_SIZE: usize = 256;

pub struct SMOSUserCSpace {
    root_cnode: sel4::cap::CNode,
    bf: bitfield_type!(CSPACE_SIZE),
}

impl SMOSUserCSpace {
    pub fn new(root_cnode: sel4::cap::CNode) -> Self {
        return Self {
            root_cnode: root_cnode,
            bf: bitfield_init!(CSPACE_SIZE),
        };
    }

    pub fn alloc_slot(&mut self) -> Result<usize, ()> {
        let index = bf_first_free(&self.bf).map_err(|_| ())?;
        bf_set_bit(&mut self.bf, index);
        return Ok(index);
    }

    pub fn free_slot(&mut self, slot: usize) {
        if slot > CSPACE_SIZE {
            // @alwin: How are we doing logging in user processes?
            return;
        }
        bf_clr_bit(&mut self.bf, slot);
    }

    pub fn to_absolute_cptr(&self, slot: usize) -> sel4::AbsoluteCPtr {
        assert!(slot < CSPACE_SIZE);
        return self
            .root_cnode
            .absolute_cptr_from_bits_with_depth(slot.try_into().unwrap(), sel4::WORD_SIZE);
    }

    pub fn delete(&self, cptr: usize) -> Result<(), sel4::Error> {
        self.root_cnode
            .absolute_cptr_from_bits_with_depth(cptr.try_into().unwrap(), sel4::WORD_SIZE)
            .delete()
    }
}
