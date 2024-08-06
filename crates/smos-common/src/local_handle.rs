use core::marker::PhantomData;

pub trait HandleType {}

#[derive(Debug, Copy, Clone)]
pub struct WindowHandle {}
impl HandleType for WindowHandle {}

#[derive(Debug, Copy, Clone)]
pub struct ViewHandle {}
impl HandleType for ViewHandle {}

#[derive(Debug, Copy, Clone)]
pub struct ObjectHandle {}
impl HandleType for ObjectHandle {}

#[derive(Debug, Copy, Clone)]
pub struct ConnectionHandle {}
impl HandleType for ConnectionHandle {}

#[derive(Debug, Copy, Clone)]
pub struct PublishHandle {}
impl HandleType for PublishHandle {}

#[derive(Debug, Copy, Clone)]
pub struct ReplyHandle {}
impl HandleType for ReplyHandle {}

#[derive(Debug, Copy, Clone)]
pub struct HandleCapHandle {}
impl HandleType for HandleCapHandle {}

#[derive(Debug, Copy, Clone)]
pub struct ProcessHandle {}
impl HandleType for ProcessHandle {}

#[derive(Debug, Copy, Clone)]
pub struct ConnRegistrationHandle {}
impl HandleType for ConnRegistrationHandle {}

#[derive(Debug, Copy, Clone)]
pub struct WindowRegistrationHandle {}
impl HandleType for WindowRegistrationHandle {}

#[derive(Debug, Copy, Clone)]
pub struct IRQRegistrationHandle {}
impl HandleType for IRQRegistrationHandle {}

#[derive(Debug, Copy, Clone)]
pub struct ChannelAuthorityHandle {}
impl HandleType for ChannelAuthorityHandle {}

#[derive(Debug, Copy, Clone)]
pub struct ChannelHandle {}
impl HandleType for ChannelHandle {}

#[derive(Debug, Copy, Clone)]
pub struct LocalHandle<A: HandleType> {
    pub idx: usize,
    marker: PhantomData<A>,
}

impl<A: HandleType> LocalHandle<A> {
    pub fn new(idx: usize) -> LocalHandle<A> {
        return LocalHandle {
            marker: PhantomData,
            idx: idx,
        };
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HandleCap<A: HandleType> {
    pub cptr: sel4::AbsoluteCPtr,
    marker: PhantomData<A>,
}

impl<A: HandleType> HandleCap<A> {
    pub fn new(cptr: sel4::AbsoluteCPtr) -> HandleCap<A> {
        return HandleCap {
            marker: PhantomData,
            cptr: cptr,
        };
    }
}

#[derive(Debug, Clone, Copy)]
pub enum HandleOrHandleCap<A: HandleType> {
    Handle(LocalHandle<A>),
    HandleCap(HandleCap<A>),
}

impl<A: HandleType> HandleOrHandleCap<A> {
    pub fn new_handle(val: usize) -> HandleOrHandleCap<A> {
        return HandleOrHandleCap::Handle(LocalHandle::new(val));
    }

    pub fn new_handle_cap(cptr: sel4::AbsoluteCPtr) -> HandleOrHandleCap<A> {
        return HandleOrHandleCap::HandleCap(HandleCap::new(cptr));
    }
}
