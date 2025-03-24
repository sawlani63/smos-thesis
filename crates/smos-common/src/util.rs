#![allow(non_snake_case)]

pub const fn ROUND_UP(n: usize, b: usize) -> usize {
    if n == 0 {
        return 0;
    }

    return (((n - 1) >> b) + 1) << b;
}

pub const fn ROUND_DOWN(n: usize, b: usize) -> usize {
    return (n >> b) << b;
}

pub const fn BIT(n: usize) -> usize {
    1 << n
}
