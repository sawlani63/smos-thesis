#![no_std]
#![allow(non_snake_case)]

use core::mem::size_of;

const fn BIT(n : usize) -> usize {
    1 << n
}

const WORD_BITS: usize = size_of::<u64>() * 8;

pub fn bf_set_bit(bf: &mut [u64], idx: usize) {
    bf[WORD_INDEX(idx)] |= BIT(BIT_INDEX(idx)) as u64;
}

pub fn bf_clr_bit(bf: &mut [u64], idx: usize) {
    bf[WORD_INDEX(idx)] &= !(BIT(BIT_INDEX(idx))) as u64;
}

pub fn bf_get_bit(bf: &mut[u64], idx: usize) -> bool {
    if (bf[WORD_INDEX(idx)] & <usize as TryInto<u64>>::try_into(BIT(BIT_INDEX(idx))).unwrap()) != 0 { true } else { false }
}

pub fn bf_first_free(bf: &[u64]) -> Result<usize, ()> {
    /* find the first free word */
    let mut i = 0;
    while i < bf.len() && bf[i] == u64::MAX {
        i += 1;
    }

    if i == bf.len() {
        return Err(());
    }

    let mut bit = i * WORD_BITS;

    if i < bf.len() {
        /* we want to find the first 0 bit, do this by inverting the value */
        let val = !bf[i];
        assert!(val != 0);
        bit += val.trailing_zeros() as usize;
    }

    return Ok(bit);
}
pub const fn BITFIELD_SIZE(x: usize) -> usize {
    x / (size_of::<u64>() * 8)
}

fn WORD_INDEX(bit : usize) -> usize {
    bit / WORD_BITS
}

fn BIT_INDEX(bit : usize) -> usize {
    bit % WORD_BITS
}

// @alwin: I had to do these to convince rust that the size of the bitfield was known at
// compile time. It seems a bit evil? Maybe there is a better way
#[macro_export]
macro_rules! bitfield_type {
    ($size:expr) => {
        [u64; $crate::BITFIELD_SIZE($size)]
    };
}

#[macro_export]
macro_rules! bitfield_init {
    ($size:expr) => {
        [0; $crate::BITFIELD_SIZE($size)]
    };
}
