use alloc::vec::Vec;
use alloc::rc::Rc;
use crate::frame_table::FrameRef;
use crate::view::View;
use crate::proc::UserProcess;
use crate::handle::RootServerResource;
use core::cell::RefCell;
use crate::cspace::{CSpace, CSpaceTrait};
use crate::frame_table::FrameTable;
use crate::ut::UTTable;
use smos_common::local_handle::{ObjectHandle, HandleOrHandleCap};
use smos_server::reply::SMOSReply;
use smos_common::error::InvocationError;
use smos_server::syscalls::{ObjCreate, ObjDestroy};
use crate::PAGE_SIZE_4K;
use smos_server::handle::{generic_allocate_handle, generic_get_handle, generic_cleanup_handle, ServerHandle};
use smos_server::handle_capability::HandleCapabilityTable;
use smos_server::handle_arg::ServerReceivedHandleOrHandleCap;

pub const OBJ_MAX_FRAMES: usize = 1024;

#[derive(Debug)]
pub struct AnonymousMemoryObject {
	pub size: usize,
	pub rights: sel4::CapRights,
	// sid
	pub frames: [Option<(sel4::cap::SmallPage, FrameRef)>; OBJ_MAX_FRAMES], // @alwin: It'd be good if this was bigger and structured more like a page table
	pub associated_views: Vec<Rc<RefCell<View>>>
}

pub fn handle_obj_create(p: &mut UserProcess, handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
						 args: &ObjCreate) -> Result<SMOSReply, InvocationError> {

	/* The root server only supports the creation of anonymous memory objects */
	// @alwin: Is this the best way to deal with externally managed objects?
	if args.name.is_some() {
		return Err(InvocationError::InvalidArguments);
	}

	/* We only support non-zero, page-size aligned memory objects */
	if args.size == 0 || args.size % PAGE_SIZE_4K != 0 {
		return Err(InvocationError::InvalidArguments);
	}

	/* Make sure the object is smaller than the max size */
	if args.size / PAGE_SIZE_4K >= OBJ_MAX_FRAMES {
		return Err(InvocationError::InvalidArguments);
	}

    let (idx, handle_ref, cptr) = generic_allocate_handle(p, handle_cap_table, args.return_cap)?;

    let mem_obj = Rc::new( RefCell::new( AnonymousMemoryObject {
		size: args.size,
		rights: args.rights.clone(),
		frames: [None; OBJ_MAX_FRAMES],
		associated_views: Vec::new()
	}));

    *handle_ref = Some(ServerHandle::new(RootServerResource::Object(mem_obj)));
    // @alwin: Do we need a list of memory objects in a process? - probs not

    let ret_value = if args.return_cap {
    	HandleOrHandleCap::<ObjectHandle>::new_handle_cap(cptr.unwrap())
    } else {
    	HandleOrHandleCap::<ObjectHandle>::new_handle(idx)
    };

    return Ok(SMOSReply::ObjCreate{hndl: ret_value});
}

pub fn handle_obj_destroy(frame_table: &mut FrameTable, p: &mut UserProcess,
						  handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
						  args: &ObjDestroy) -> Result<SMOSReply, InvocationError> {

    /* Check that the passed in handle/cap is within bounds */
    let handle_ref = generic_get_handle(p, handle_cap_table, args.hndl, 0)?;

	    /* Check that the handle refers to is an object */
    let object = match handle_ref.as_ref().unwrap().inner() {
        RootServerResource::Object(obj) => Ok(obj.clone()),
        _ => {
            match args.hndl {
                ServerReceivedHandleOrHandleCap::Handle(x) => Err(InvocationError::InvalidHandle {which_arg: 0}),
                ServerReceivedHandleOrHandleCap::UnwrappedHandleCap(x) => Err(InvocationError::InvalidHandleCapability {which_arg: 0}),
                _ => panic!("We should not get an unwrapped handle cap here")
            }
        }
    }?;

    if !object.borrow().associated_views.is_empty() {
    	// @alwin: I think we shouldn't be able to destroy objects that have views,
    	// since not everything that sets up a view with the object will have
    	// necessarily set up a ntfn buffer to tell them this has gone away
    	// under their feet.
    	todo!()
    }

    for frame in object.borrow_mut().frames {
    	if let Some(inner) = frame {
    		frame_table.free_frame(inner.1);
    	}
    }

    generic_cleanup_handle(p, handle_cap_table, args.hndl, 0)?;

    return Ok(SMOSReply::ObjDestroy);
}