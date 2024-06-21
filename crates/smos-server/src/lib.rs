#![no_std]

extern crate alloc;
pub mod syscalls;
pub mod error;
pub mod reply;
pub mod handle_arg;
pub mod event;
pub mod handle;
pub mod handle_capability;
pub mod ntfn_buffer;