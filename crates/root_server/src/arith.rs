#![allow(non_snake_case)]

use core::mem::size_of;
use crate::limits::CHAR_BIT;

pub fn ROUND_UP(n: usize, b: usize) -> usize {
	if n == 0 {
		return 0;
	}

	return (((n - 1) >> b) + 1) << b
}

pub fn ROUND_DOWN(n: usize, b: usize) -> usize {
	return (((n) >> (b)) << (b));

}

pub const fn LOG_BASE_2(n: usize) -> usize {
	size_of::<usize>() * CHAR_BIT - n.leading_zeros() as usize - 1
}