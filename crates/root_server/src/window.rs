use alloc::rc::Rc;
use crate::view::View;
use crate::handle::{SMOSObject, generic_allocate_handle, generic_get_handle, generic_cleanup_handle, Handle};
use crate::proc::UserProcess;
use smos_server::syscalls::{WindowCreate, WindowDestroy};
use smos_server::reply::SMOSReply;
use smos_common::error::InvocationError;
use crate::PAGE_SIZE_4K;
use smos_common::args::{WindowCreateArgs, WindowDestroyArgs};
use smos_common::local_handle::{HandleOrHandleCap, WindowHandle};
use smos_server::handle_arg::HandleOrUnwrappedHandleCap;
use core::cell::RefCell;

#[derive(Clone, Debug)]
pub struct Window {
    pub start: usize,
    pub size: usize,
    pub bound_view: Option<Rc<RefCell<View>>>
}

pub fn handle_window_create(p: &mut UserProcess, args: &WindowCreate) -> Result<SMOSReply, InvocationError> {
    if args.base_vaddr as usize % PAGE_SIZE_4K != 0 {
        err_rs!("Window base address should be aligned");
        return Err(InvocationError::AlignmentError{ which_arg: WindowCreateArgs::Base_Vaddr as usize})
    }

    /* Ensure that the window does not overlap with any other windows */
    if p.overlapping_window(args.base_vaddr.try_into().unwrap(), args.size) {
        err_rs!("Window overlaps with an existing window");
        return Err(InvocationError::InvalidArguments);
    }

    let (idx, handle_ref, cptr) = generic_allocate_handle(p, args.return_cap)?;

    // @alwin: Check that window size is in user addressable vspace?

    // @alwin: Eventually, we should have an allocator per-process and allocate this box from
    // the caller's allocator to have better memory usage bookkeeping
    let window = Rc::new(RefCell::new( Window {
        start: args.base_vaddr.try_into().unwrap(),
        size: args.size,
        bound_view: None
    }));

    *handle_ref = Some(Handle::new(SMOSObject::Window(window.clone())));
    p.add_window_unchecked(window);

    let ret_value = if args.return_cap {
        HandleOrHandleCap::<WindowHandle>::new_handle_cap(cptr.unwrap())
    } else {
        HandleOrHandleCap::<WindowHandle>::new_handle(idx)
    };

    return Ok(SMOSReply::WindowCreate{hndl: ret_value})
}

pub fn handle_window_destroy(p: &mut UserProcess, args: &WindowDestroy) -> Result<SMOSReply, InvocationError> {
    /* Check that the passed in handle/cap is within bounds */
    let handle_ref = generic_get_handle(p, args.hndl, WindowDestroyArgs::Handle as usize)?;

    /* Check that the object it refers to is a window */
    let window = match handle_ref.as_ref().unwrap().inner() {
        SMOSObject::Window(win) => Ok(win.clone()),
        _ => {
            match args.hndl {
                HandleOrUnwrappedHandleCap::Handle(x) => Err(InvocationError::InvalidHandle {which_arg: WindowDestroyArgs::Handle as usize}),
                HandleOrUnwrappedHandleCap::UnwrappedHandleCap(x) => Err(InvocationError::InvalidHandleCapability {which_arg: WindowDestroyArgs::Handle as usize}),
            }
        }
    }?;


    if let Some(bv) = &window.borrow_mut().bound_view {
        todo!()
    }

    generic_cleanup_handle(p, args.hndl, WindowDestroyArgs::Handle as usize)?;

    return Ok(SMOSReply::WindowDestroy{})
}