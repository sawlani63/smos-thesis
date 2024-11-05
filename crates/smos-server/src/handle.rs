use crate::handle_arg::ServerReceivedHandleOrHandleCap;
use crate::handle_capability::HandleCapabilityTable;
use smos_common::error::InvocationError;

pub trait HandleInner {}

#[derive(Clone, Debug)]
pub struct ServerHandle<T: HandleInner> {
    inner: T,
}

impl<T: HandleInner> ServerHandle<T> {
    pub fn new(val: T) -> Self {
        ServerHandle { inner: val }
    }

    pub fn inner(&self) -> &T {
        return &self.inner;
    }
}

pub trait HandleAllocater<T: HandleInner> {
    fn allocate_handle(
        &mut self,
    ) -> Result<(usize, &mut Option<ServerHandle<T>>), InvocationError> {
        for (i, handle) in self.handle_table_mut().iter_mut().enumerate() {
            if handle.is_none() {
                return Ok((i, handle));
            }
        }

        return Err(InvocationError::OutOfHandles);
    }

    // @alwin: This is kind of weird. Consider an option type instead
    fn get_handle(&self, idx: usize) -> Result<&Option<ServerHandle<T>>, ()> {
        if idx > self.handle_table_size() {
            return Err(());
        }

        return Ok(&self.handle_table()[idx]);
    }

    fn get_handle_mut(&mut self, idx: usize) -> Result<&mut Option<ServerHandle<T>>, ()> {
        if idx > self.handle_table_size() {
            return Err(());
        }

        return Ok(&mut self.handle_table_mut()[idx]);
    }

    fn cleanup_handle(&mut self, idx: usize) -> Result<(), ()> {
        if idx > self.handle_table_size() {
            return Err(());
        }

        self.handle_table_mut()[idx] = None;
        return Ok(());
    }

    fn handle_table_size(&self) -> usize;
    fn handle_table(&self) -> &[Option<ServerHandle<T>>];
    fn handle_table_mut(&mut self) -> &mut [Option<ServerHandle<T>>];
}

pub fn generic_allocate_handle<'a, 'b: 'a, Y: HandleInner, X: HandleAllocater<Y>>(
    p: &'a mut X,
    handle_cap_table: &'b mut HandleCapabilityTable<Y>,
    return_cap: bool,
) -> Result<
    (
        usize,
        &'a mut Option<ServerHandle<Y>>,
        Option<sel4::AbsoluteCPtr>,
    ),
    InvocationError,
> {
    if return_cap {
        handle_cap_table.allocate_handle_cap()
    } else {
        let tmp = p.allocate_handle()?;
        Ok((tmp.0, tmp.1, None))
    }
}

pub fn generic_get_handle<'a, 'b: 'a, Y: HandleInner, X: HandleAllocater<Y>>(
    p: &'a mut X,
    handle_cap_table: &'b mut HandleCapabilityTable<Y>,
    hndl: ServerReceivedHandleOrHandleCap,
    which_arg: usize,
) -> Result<&'a mut Option<ServerHandle<Y>>, InvocationError> {
    match hndl {
        ServerReceivedHandleOrHandleCap::Handle(x) => {
            p.get_handle_mut(x.idx)
                .map_err(|_| InvocationError::InvalidHandle {
                    which_arg: which_arg,
                })
        }
        ServerReceivedHandleOrHandleCap::UnwrappedHandleCap(x) => handle_cap_table
            .get_handle_cap_mut(x.idx)
            .map_err(|_| InvocationError::InvalidHandleCapability {
                which_arg: which_arg,
            }),
        ServerReceivedHandleOrHandleCap::WrappedHandleCap(_) => {
            panic!("Should never be calling this on an wrapped handle cap")
        }
    }
}

pub fn generic_invalid_handle_error(
    hndl: ServerReceivedHandleOrHandleCap,
    which_arg: usize,
) -> InvocationError {
    match hndl {
        ServerReceivedHandleOrHandleCap::Handle(_) => InvocationError::InvalidHandle {
            which_arg: which_arg,
        },
        ServerReceivedHandleOrHandleCap::UnwrappedHandleCap(_) => {
            InvocationError::InvalidHandleCapability {
                which_arg: which_arg,
            }
        }
        _ => panic!("We should not get an unwrapped handle cap here"),
    }
}

// @alwin: I think it is kinda unnecessary for this to to have error checking because it should only be called after a get
pub fn generic_cleanup_handle<'a, 'b: 'a, Y: HandleInner, X: HandleAllocater<Y>>(
    p: &mut X,
    handle_cap_table: &'b mut HandleCapabilityTable<Y>,
    hndl: ServerReceivedHandleOrHandleCap,
    which_arg: usize,
) -> Result<(), InvocationError> {
    match hndl {
        ServerReceivedHandleOrHandleCap::Handle(x) => {
            p.cleanup_handle(x.idx)
                .map_err(|_| InvocationError::InvalidHandle {
                    which_arg: which_arg,
                })
        }
        ServerReceivedHandleOrHandleCap::UnwrappedHandleCap(x) => handle_cap_table
            .cleanup_handle_cap(x.idx)
            .map_err(|_| InvocationError::InvalidHandleCapability {
                which_arg: which_arg,
            }),
        ServerReceivedHandleOrHandleCap::WrappedHandleCap(_) => {
            panic!("Should never be calling this on an wrapped handle cap")
        }
    }
}
