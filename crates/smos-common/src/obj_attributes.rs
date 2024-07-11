use core::ops::{BitOr, BitOrAssign, BitAnd, BitAndAssign};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ObjAttributes(u64);

impl ObjAttributes {

	pub const fn from_inner(inner: u64) -> Self {
		Self(inner)
	}

	pub const fn into_inner(self) -> u64 {
		self.0
	}

	pub const fn inner(&self) -> &u64 {
		&self.0
	}

	pub fn inner_mut(&mut self) -> &mut u64 {
		&mut self.0
	}

    pub const fn has(self, rhs: Self) -> bool {
        self.into_inner() & rhs.into_inner() != 0
    }

	pub const DEFAULT: Self = Self::from_inner(0);
	pub const CONTIGUOUS: Self = Self::from_inner(1);
	pub const DEVICE: Self = Self::from_inner(2);
	pub const EAGER: Self = Self::from_inner(4);
}

impl BitOr for ObjAttributes {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self::from_inner(self.into_inner().bitor(rhs.into_inner()))
    }
}

impl BitOrAssign for ObjAttributes {
    fn bitor_assign(&mut self, rhs: Self) {
        self.inner_mut().bitor_assign(rhs.into_inner());
    }
}

impl BitAnd for ObjAttributes {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self::from_inner(self.into_inner().bitand(rhs.into_inner()))
    }
}

impl BitAndAssign for ObjAttributes {
    fn bitand_assign(&mut self, rhs: Self) {
        self.inner_mut().bitand_assign(rhs.into_inner());
    }
}