#![allow(non_snake_case)]

use core::mem::size_of;
use crate::limits::CHAR_BIT;

pub const fn LOG_BASE_2(n: usize) -> usize {
    size_of::<usize>() * CHAR_BIT - n.leading_zeros() as usize - 1
}