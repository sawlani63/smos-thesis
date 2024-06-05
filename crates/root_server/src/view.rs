use crate::handle::{Handle, generic_get_handle, generic_allocate_handle};
use crate::window::Window;
use crate::object::{AnonymousMemoryObject, OBJ_MAX_FRAMES};
use alloc::rc::Rc;
use core::cell::RefCell;
use crate::proc::UserProcess;
use smos_server::reply::SMOSReply;
use smos_common::error::InvocationError;
use smos_common::args::ViewArgs;
use smos_server::handle_arg::HandleOrUnwrappedHandleCap;
use crate::PAGE_SIZE_4K;
use crate::handle::SMOSObject;
use core::borrow::Borrow;
use smos_common::local_handle;

#[derive(Clone, Debug)]
pub struct View {
	pub caps: [Option<sel4::cap::SmallPage>; OBJ_MAX_FRAMES],
	pub bound_window: Rc<RefCell<Window>>,
	pub bound_object: Rc<RefCell<AnonymousMemoryObject>>,
	pub rights: sel4::CapRights,
}


pub fn handle_view(p: &mut UserProcess, args: &smos_server::syscalls::View) -> Result<SMOSReply, InvocationError> {
    let window_ref = generic_get_handle(p, args.window, ViewArgs::Window as usize)?;
    let window: Rc<RefCell<Window>> = match window_ref.as_ref().unwrap().inner() {
        SMOSObject::Window(win) => Ok(win.clone()),
        _ => {
            match args.window {
                HandleOrUnwrappedHandleCap::Handle(x) => Err(InvocationError::InvalidHandle {which_arg: ViewArgs::Window as usize}),
                HandleOrUnwrappedHandleCap::UnwrappedHandleCap(x) => Err(InvocationError::InvalidHandleCapability {which_arg: ViewArgs::Window as usize}),
            }
        }
    }?;

    let object_ref = generic_get_handle(p, args.object, ViewArgs::Object as usize)?;
    let object: Rc<RefCell<AnonymousMemoryObject>> = match object_ref.as_ref().unwrap().inner() {
        SMOSObject::Object(obj) => Ok(obj.clone()),
        _ => {
            match args.object {
                HandleOrUnwrappedHandleCap::Handle(x) => Err(InvocationError::InvalidHandle {which_arg: ViewArgs::Object as usize}),
                HandleOrUnwrappedHandleCap::UnwrappedHandleCap(x) => Err(InvocationError::InvalidHandleCapability {which_arg: ViewArgs::Object as usize}),
            }
        }
    }?;

	/* Ensure the size is non-zero */
	if (args.size == 0) {
		return Err(InvocationError::InvalidArguments);
	}

	/* Ensure offsets into the window and object are page aligned */
	if (args.window_offset & PAGE_SIZE_4K != 0) {
		return Err(InvocationError::AlignmentError{which_arg: ViewArgs::WinOffset as usize});
	}

	if (args.obj_offset & PAGE_SIZE_4K != 0) {
		return Err(InvocationError::AlignmentError{which_arg: ViewArgs::ObjOffset as usize});
	}

	/* Ensure that the object is big enough */
	if args.obj_offset + args.size > object.borrow_mut().size {
		return Err(InvocationError::InvalidArguments);
	}

	/* Ensure everything stays inside the window */
	if args.window_offset + args.size > window.borrow_mut().size {
		return Err(InvocationError::InvalidArguments);
	}

	/* Ensure that the window isn't already being used for another view */
	if window.borrow_mut().bound_view.is_some() { // @alwin: Why do I need to borrow_mut() here?
		return Err(InvocationError::InvalidArguments);
	}

    let view = Rc::new( RefCell::new( View {
    	caps: [None; OBJ_MAX_FRAMES],
    	bound_window: window.clone(),
    	bound_object: object.clone(),
    	rights: args.rights.clone()
    }));

	window.borrow_mut().bound_view = Some(view.clone());
	object.borrow_mut().associated_views.push(view.clone());

	// @alwin: Deal with permissions and do appropriate cleanup
    let (idx, handle_ref) = p.allocate_handle()?;

    *handle_ref = Some(Handle::new(SMOSObject::View(view.clone())));
    p.views.push(view);

    return Ok(SMOSReply::View {hndl: local_handle::Handle::new(idx)})
}