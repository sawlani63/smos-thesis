use core::mem::size_of;
use crate::page::BIT;

const WORD_BITS: usize = size_of::<u64>() * 8;

pub fn bf_set_bit(bf: &mut [u64], idx: usize) {
	bf[WORD_INDEX(idx)] |= BIT(BIT_INDEX(idx)) as u64;
}
pub fn bf_clr_bit(bf: &mut [u64], idx: usize) {
    bf[WORD_INDEX(idx)] &= !(BIT(BIT_INDEX(idx))) as u64;
}

pub fn bf_first_free(bf: &[u64]) -> usize {
    /* find the first free word */
	let mut i = 0;
	while i < bf.len() && bf[i] == u64::MAX {
		i += 1;
	}

	let mut bit = i * WORD_BITS;

	if (i < bf.len()) {
	    /* we want to find the first 0 bit, do this by inverting the value */
		let val = !bf[i];
		assert!(val != 0);
		bit += val.trailing_zeros() as usize;
	}

	return bit;
}
pub const fn BITFIELD_SIZE(x: usize) -> usize {
	x / sel4::WORD_SIZE
}

fn WORD_INDEX(bit : usize) -> usize {
	bit / WORD_BITS
}

fn BIT_INDEX(bit : usize) -> usize {
	bit % WORD_BITS
}

// @alwin: I had to do these to convince rust that the size of the bitfield was known at
// compile time. It seems a bit evil? Maybe there is a better way
macro_rules! bitfield_type {
	($size:expr) => {
		[u64; BITFIELD_SIZE($size)]
	};
}

macro_rules! bitfield_init {
	($size:expr) => {
		[0; BITFIELD_SIZE($size)]
	};
}

pub(crate) use bitfield_type;
pub(crate) use bitfield_init;
