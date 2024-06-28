use crate::handle::RootServerResource;
use smos_server::handle::{generic_allocate_handle, generic_get_handle, generic_cleanup_handle,
						  generic_invalid_handle_error, HandleAllocater, ServerHandle};
use crate::window::Window;
use crate::object::{AnonymousMemoryObject, OBJ_MAX_FRAMES};
use alloc::rc::Rc;
use core::cell::RefCell;
use crate::proc::UserProcess;
use smos_server::reply::SMOSReply;
use smos_common::error::InvocationError;
use smos_common::args::ViewArgs;
use smos_server::handle_arg::ServerReceivedHandleOrHandleCap;
use crate::PAGE_SIZE_4K;
use core::borrow::Borrow;
use smos_common::local_handle::{LocalHandle, WindowRegistrationHandle};
use smos_server::handle_capability::HandleCapabilityTable;
use crate::connection::Server;
use crate::ReplyWrapper;
use crate::cspace::{CSpace, CSpaceTrait};

#[derive(Clone, Debug)]
pub struct View {
	// @alwin: caps should probs look something like a mini page table
	pub caps: [Option<sel4::cap::UnspecifiedFrame>; OBJ_MAX_FRAMES],
	pub bound_window: Rc<RefCell<Window>>,
	pub bound_object: Option<Rc<RefCell<AnonymousMemoryObject>>>,
	pub managing_server_info:  Option<(Rc<RefCell<Server>>, usize, usize)>, // @alwin: Does this need the window registration handle?
	pub rights: sel4::CapRights,
	pub win_offset: usize,
	pub obj_offset: usize,
	pub pending_fault: Option<(ReplyWrapper, sel4::VmFault, sel4::cap::VSpace)>
}


pub fn handle_view(p: &mut UserProcess, handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
				   args: &smos_server::syscalls::View) -> Result<SMOSReply, InvocationError> {

    let window_ref = generic_get_handle(p, handle_cap_table, args.window, ViewArgs::Window as usize)?;
    let window: Rc<RefCell<Window>> = match window_ref.as_ref().unwrap().inner() {
        RootServerResource::Window(win) => Ok(win.clone()),
        _ => Err(generic_invalid_handle_error(args.window, ViewArgs::Window as usize)),
    }?;

    let object_ref = generic_get_handle(p, handle_cap_table, args.object, ViewArgs::Object as usize)?;
    let object: Rc<RefCell<AnonymousMemoryObject>> = match object_ref.as_ref().unwrap().inner() {
        RootServerResource::Object(obj) => Ok(obj.clone()),
        _ => Err(generic_invalid_handle_error(args.object, ViewArgs::Object as usize)),
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
    	win_offset: args.window_offset,
    	obj_offset: args.obj_offset,
    	bound_window: window.clone(),
    	bound_object: Some(object.clone()),
    	managing_server_info: None,
    	rights: args.rights.clone(), // @alwin: The rights of the view should be &'d with the rights of the object
        pending_fault: None
    }));

	window.borrow_mut().bound_view = Some(view.clone());
	object.borrow_mut().associated_views.push(view.clone());

	// @alwin: Deal with permissions and do appropriate cleanup
    let (idx, handle_ref) = p.allocate_handle()?;

    *handle_ref = Some(ServerHandle::new(RootServerResource::View(view.clone())));
    p.views.push(view);

    return Ok(SMOSReply::View {hndl: LocalHandle::new(idx)})
}

pub fn handle_unview(cspace: &mut CSpace, p: &mut UserProcess, args: &smos_server::syscalls::Unview)
	-> Result<SMOSReply, InvocationError> {

	let view_ref = p.get_handle_mut(args.hndl.idx).or(Err(InvocationError::InvalidHandle {which_arg: 0}))?;
	let view = match view_ref.as_ref().unwrap().inner() {
		RootServerResource::View(view) => Ok(view.clone()),
		_ => Err(InvocationError::InvalidHandle{ which_arg: 0})
	}?;

	for cap in view.borrow_mut().caps {
		if let Some(frame_cap) = cap {
			/* @alwin: deleting the cap should result in an unmap, but double check*/
			cspace.delete_cap(frame_cap);
			cspace.free_cap(frame_cap);
		}
	}

	view.borrow_mut().bound_window.borrow_mut().bound_view = None;
    assert!(view.borrow_mut().bound_object.is_some());
	let pos = view.borrow_mut().bound_object.as_ref().unwrap().borrow_mut().associated_views.iter().position(|x| Rc::ptr_eq(x, &view)).unwrap();
	view.borrow_mut().bound_object.as_ref().unwrap().borrow_mut().associated_views.swap_remove(pos);

	p.cleanup_handle(args.hndl.idx);

	return Ok(SMOSReply::Unview)
}