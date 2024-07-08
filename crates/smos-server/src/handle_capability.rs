use crate::handle::{HandleInner, ServerHandle};
use alloc::vec::Vec;
use smos_common::error::InvocationError;

pub struct HandleCapability<T: HandleInner> {
    pub handle: Option<ServerHandle<T>>,
    pub root_cap: Option<sel4::AbsoluteCPtr>,
}

pub struct HandleCapabilityTable<T: HandleInner> {
    slots: Vec<HandleCapability<T>>,
}

impl<T: HandleInner> HandleCapabilityTable<T> {
    // @alwin: Could this be a &[HandleCapability<T>]
    pub fn new(slots: Vec<HandleCapability<T>>) -> Self {
        return HandleCapabilityTable { slots: slots };
    }

    pub fn allocate_handle_cap(
        &mut self,
    ) -> Result<
        (
            usize,
            &mut Option<ServerHandle<T>>,
            Option<sel4::AbsoluteCPtr>,
        ),
        InvocationError,
    > {
        for (i, handle_cap) in self.slots.iter_mut().enumerate() {
            if (handle_cap.handle.is_none()) {
                return Ok((i, &mut handle_cap.handle, handle_cap.root_cap));
            }
        }

        return Err(InvocationError::OutOfHandleCaps);
    }

    pub fn get_handle_cap_mut(&mut self, idx: usize) -> Result<&mut Option<ServerHandle<T>>, ()> {
        if idx >= self.slots.len() {
            return Err(());
        }

        Ok(&mut self.slots[idx].handle)
    }

    pub fn cleanup_handle_cap(&mut self, idx: usize) -> Result<(), ()> {
        if idx >= self.slots.len() {
            return Err(());
        }

        // @alwin: Should probs not be an assert
        assert!(self.slots[idx].root_cap.is_some());
        self.slots[idx].root_cap.unwrap().revoke();
        self.slots[idx].handle = None;
        return Ok(());
    }

    pub fn deallocate_handle_cap() {
        todo!()
    }
}
