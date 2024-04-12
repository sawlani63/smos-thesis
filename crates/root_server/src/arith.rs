use core::mem::size_of;
use crate::limits::CHAR_BIT;

pub const fn ROUND_UP(n: usize, b: usize) -> usize {
	(((n - 1) >> b) + 1) << b
}

pub const fn LOG_BASE_2(n: usize) -> usize {
	size_of::<usize>() * CHAR_BIT - n.leading_zeros() as usize - 1
}