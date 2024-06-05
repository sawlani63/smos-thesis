use alloc::rc::Rc;
use crate::proc::UserProcess;
use smos_common::error::InvocationError;
use crate::handle_capability::allocate_handle_cap;
use smos_server::handle_arg::HandleOrUnwrappedHandleCap;
use downcast_rs::{Downcast, impl_downcast};
use crate::handle_capability::{get_handle_cap_mut, cleanup_handle_cap};
use core::cell::RefCell;
use crate::window::Window;
use crate::object::AnonymousMemoryObject;
use crate::view::View;
use crate::connection::Connection;

#[derive(Debug, Clone)]
pub enum SMOSObject {
    Window(Rc<RefCell<Window>>),
    Object(Rc<RefCell<AnonymousMemoryObject>>),
    View(Rc<RefCell<View>>),
    Connection(Rc<RefCell<Connection>>) // Does this need a refcell?
}


#[derive(Clone)]
pub struct Handle {
    inner: SMOSObject
}

impl Handle {
    pub fn new(val: SMOSObject) -> Self {
        Handle {
            inner: val
        }
    }

    pub fn inner(&self) -> &SMOSObject {
        return &self.inner;
    }
}

pub fn generic_allocate_handle<'a>(p: &'a mut UserProcess, return_cap: bool) -> Result<(usize, &'a mut Option<Handle>, Option<sel4::AbsoluteCPtr>), InvocationError> {
    if return_cap {
        allocate_handle_cap()
    } else {
        let tmp = p.allocate_handle()?;
        Ok((tmp.0, tmp.1, None))
    }
}


pub fn generic_get_handle<'a>(p: &'a mut UserProcess, hndl: HandleOrUnwrappedHandleCap, which_arg: usize) -> Result<&'a mut Option<Handle>, InvocationError> {
    match hndl {
        HandleOrUnwrappedHandleCap::Handle(x) => p.get_handle_mut(x).map_err(|_| InvocationError::InvalidHandle{ which_arg: which_arg}),
        HandleOrUnwrappedHandleCap::UnwrappedHandleCap(x) => get_handle_cap_mut(x).map_err(|_| InvocationError::InvalidHandleCapability {which_arg: which_arg}),
    }
}

// @alwin: I think it is kinda unnecessary for this to to have error checking because it should only be called after a get
pub fn generic_cleanup_handle(p: &mut UserProcess, hndl: HandleOrUnwrappedHandleCap, which_arg: usize) -> Result<(), InvocationError> {
    match hndl {
        HandleOrUnwrappedHandleCap::Handle(x) => p.cleanup_handle(x).map_err(|_| InvocationError::InvalidHandle{ which_arg: which_arg}),
        HandleOrUnwrappedHandleCap::UnwrappedHandleCap(x) => cleanup_handle_cap(x).map_err(|_| InvocationError::InvalidHandleCapability {which_arg: which_arg})
    }
}