use num_enum::{IntoPrimitive, TryFromPrimitive};

// @alwin: This should probably be in a different crate

#[repr(u64)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum QueueType {
    Active = 0,
    Free,
    None,
}

#[repr(u64)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum VirtType {
    Tx = 0,
    Rx,
}
