use num_enum::{TryFromPrimitive, IntoPrimitive};
use crate::invocations::SMOSInvocation;

// @alwin: I don't like how these have to be done seperately,
// is there a cleaner way to do it?
#[derive(Debug, TryFromPrimitive, IntoPrimitive)]
#[repr(u64)]
pub enum InvocationErrorLabel {
	NoError = 0,
	InvalidInvocation,
	NotEnoughArgs,
	NotEnoughCaps,
	InvalidType,
	CSpaceFull,
	UnsupportedInvocation,
}

#[derive(Debug)]
pub enum InvocationError {
	NoError,
	InvalidInvocation,
	NotEnoughArgs {expected: usize, actual: usize},
	NotEnoughCaps {expected: usize, actual: usize},
	InvalidType {which_arg: usize},
	CSpaceFull,
	UnsupportedInvocation {label: SMOSInvocation}
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum NotEnoughArgsMessage {
	Expected = 0,
	Actual = 1,
	Length = 2
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum NotEnoughCapsMessage {
	Expected = 0,
	Actual = 1,
	Length = 2
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum InvalidTypeMessage {
	Which = 0,
	Length = 1
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum UnsupportedInvocationMessage {
	Label = 0,
	Length = 1
}