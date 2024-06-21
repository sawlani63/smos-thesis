use core::marker::PhantomData;

pub trait HandleType {}

#[derive(Debug, Clone)]
pub struct WindowHandle {}
impl HandleType for WindowHandle {}

#[derive(Debug, Clone)]
pub struct ViewHandle {}
impl HandleType for ViewHandle {}

#[derive(Debug, Clone)]
pub struct ObjectHandle {}
impl HandleType for ObjectHandle {}

#[derive(Debug, Clone)]
pub struct ConnectionHandle {}
impl HandleType for ConnectionHandle {}

#[derive(Debug, Clone)]
pub struct PublishHandle {}
impl HandleType for PublishHandle {}

#[derive(Debug, Clone)]
pub struct ReplyHandle {}
impl HandleType for ReplyHandle {}

#[derive(Debug, Clone)]
pub struct ProcessHandle {}
impl HandleType for ProcessHandle {}

#[derive(Debug, Clone)]
pub struct ConnRegistrationHandle {}
impl HandleType for ConnRegistrationHandle {}

#[derive(Debug, Copy, Clone)]
pub struct WindowRegistrationHandle {}
impl HandleType for WindowRegistrationHandle {}

#[derive(Debug, Copy, Clone)]
pub struct LocalHandle<A: HandleType> {
	pub idx: usize,
	marker: PhantomData<A>,
}

impl<A: HandleType> LocalHandle<A> {
	pub fn new(idx: usize) -> LocalHandle<A> {
		return LocalHandle {
			marker: PhantomData,
			idx: idx
		};
	}
}

#[derive(Debug, Clone)]
pub struct HandleCap<A: HandleType> {
	pub cptr: sel4::AbsoluteCPtr,
	marker: PhantomData<A>,
}

impl<A: HandleType> HandleCap<A> {
	pub fn new(cptr: sel4::AbsoluteCPtr) -> HandleCap<A> {
		return HandleCap {
			marker: PhantomData,
			cptr: cptr
		};
	}
}

#[derive(Debug, Clone)]
pub enum HandleOrHandleCap<A: HandleType> {
	Handle(LocalHandle<A>),
	HandleCap(HandleCap<A>)
}

impl<A: HandleType> HandleOrHandleCap<A> {
	pub fn new_handle(val: usize) -> HandleOrHandleCap<A> {
		return HandleOrHandleCap::Handle( LocalHandle::new(val) );
	}

	pub fn new_handle_cap(cptr: sel4::AbsoluteCPtr) -> HandleOrHandleCap<A> {
		return HandleOrHandleCap::HandleCap( HandleCap::new(cptr) );
	}
}