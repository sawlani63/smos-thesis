#![allow(non_snake_case)]

use crate::arith::LOG_BASE_2;

pub const fn BIT(n : usize) -> usize {
	1 << n
}
pub const fn BYTES_TO_SIZE_BITS(bytes: usize) -> usize {
	LOG_BASE_2(bytes)
}

pub const fn BYTES_TO_4K_PAGES(b: usize) -> usize {
	BYTES_TO_SIZE_BITS_PAGES(b, PAGE_BITS_4K)
}

pub const fn SIZE_BITS_TO_BYTES(size_bits: usize) -> usize {
	BIT(size_bits)
}
pub const fn BYTES_TO_SIZE_BITS_PAGES(b : usize, size_bits : usize) -> usize {
	(b / BIT(size_bits)) + if (b % BIT(size_bits)) > 0 { 1 } else { 0 }
}

pub const PAGE_BITS_4K: usize =  12;
pub const PAGE_SIZE_4K: usize = SIZE_BITS_TO_BYTES(PAGE_BITS_4K);
pub const PAGE_MASK_4K: usize = PAGE_SIZE_4K - 1;
pub const fn PAGE_ALIGN_4K(addr : usize) -> usize {
	addr & !PAGE_MASK_4K
}


