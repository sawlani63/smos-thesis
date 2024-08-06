use smos_common::local_handle::{HandleCap, HandleOrHandleCap, HandleType};

#[derive(Debug, Copy, Clone)]
pub struct ReceivedHandle {
    pub idx: usize,
}

impl ReceivedHandle {
    pub fn new(idx: usize) -> Self {
        return Self { idx: idx };
    }
}

#[derive(Debug, Copy, Clone)]
pub struct UnwrappedHandleCap {
    pub idx: usize,
}

impl UnwrappedHandleCap {
    pub fn new(idx: usize) -> Self {
        return Self { idx: idx };
    }
}

#[derive(Debug, Copy, Clone)]
pub struct WrappedHandleCap {
    pub cptr: sel4::AbsoluteCPtr,
}

impl WrappedHandleCap {
    pub fn new(cptr: sel4::AbsoluteCPtr) -> Self {
        return Self { cptr: cptr };
    }
}

// @alwin: Should this be parameterized by the HandleType things
#[derive(Debug, Copy, Clone)]
pub enum ServerReceivedHandleOrHandleCap {
    Handle(ReceivedHandle),
    UnwrappedHandleCap(UnwrappedHandleCap),
    WrappedHandleCap(WrappedHandleCap),
}

impl ServerReceivedHandleOrHandleCap {
    pub fn new_handle(idx: usize) -> Self {
        return ServerReceivedHandleOrHandleCap::Handle(ReceivedHandle { idx: idx });
    }

    pub fn new_unwrapped_handle_cap(idx: usize) -> Self {
        return ServerReceivedHandleOrHandleCap::UnwrappedHandleCap(UnwrappedHandleCap {
            idx: idx,
        });
    }

    pub fn new_wrapped_handle_cap(cptr: sel4::AbsoluteCPtr) -> Self {
        return ServerReceivedHandleOrHandleCap::WrappedHandleCap(WrappedHandleCap { cptr: cptr });
    }
}

impl<A: HandleType> From<WrappedHandleCap> for HandleCap<A> {
    fn from(val: WrappedHandleCap) -> Self {
        HandleCap::new(val.cptr)
    }
}

impl<A: HandleType> From<WrappedHandleCap> for HandleOrHandleCap<A> {
    fn from(val: WrappedHandleCap) -> Self {
        return HandleOrHandleCap::new_handle_cap(val.cptr);
    }
}

impl<A: HandleType> TryFrom<ServerReceivedHandleOrHandleCap> for HandleCap<A> {
    type Error = ();

    fn try_from(val: ServerReceivedHandleOrHandleCap) -> Result<Self, Self::Error> {
        match val {
            ServerReceivedHandleOrHandleCap::WrappedHandleCap(wrapped_hndl_cap) => {
                Ok(HandleCap::new(wrapped_hndl_cap.cptr))
            }
            _ => Err(()),
        }
    }
}

impl<A: HandleType> TryFrom<ServerReceivedHandleOrHandleCap> for HandleOrHandleCap<A> {
    type Error = ();

    fn try_from(val: ServerReceivedHandleOrHandleCap) -> Result<Self, Self::Error> {
        Ok(HandleOrHandleCap::HandleCap(val.try_into()?))
    }
}
