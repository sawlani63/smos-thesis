pub const fn ALIGN_DOWN(x : usize, n : usize) -> usize {
	return x & !(n - 1);
}

pub const fn ALIGN_UP(x: usize, n: usize) -> usize {
	(x + n - 1) & !(n - 1)
}

const fn BIT(n : usize) -> usize {
	1 << n
}

pub const fn MASK(n: usize) -> usize {
	BIT(n) - 1
}
