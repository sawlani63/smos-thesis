use core::marker::PhantomData;

pub trait HandleType {}

#[derive(Debug)]
pub struct WindowHandle {}
impl HandleType for WindowHandle {}

#[derive(Debug)]
pub struct ViewHandle {}
impl HandleType for ViewHandle {}

#[derive(Debug)]
pub struct ObjectHandle {}
impl HandleType for ObjectHandle {}

#[derive(Debug)]
pub struct ConnectionHandle {}
impl HandleType for ConnectionHandle {}

#[derive(Debug)]
pub struct Handle<A: HandleType> {
	pub idx: usize,
	marker: PhantomData<A>,
}

impl<A: HandleType> Handle<A> {
	pub fn new(idx: usize) -> Handle<A> {
		return Handle {
			marker: PhantomData,
			idx: idx
		};
	}
}

#[derive(Debug)]
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


#[derive(Debug)]
pub enum HandleOrHandleCap<A: HandleType> {
	Handle(Handle<A>),
	HandleCap(HandleCap<A>)
}

impl<A: HandleType> HandleOrHandleCap<A> {
	pub fn new_handle(val: usize) -> HandleOrHandleCap<A> {
		return HandleOrHandleCap::Handle( Handle::new(val) );
	}

	pub fn new_handle_cap(cptr: sel4::AbsoluteCPtr) -> HandleOrHandleCap<A> {
		return HandleOrHandleCap::HandleCap( HandleCap::new(cptr) );
	}
}