use alloc::vec::Vec;
use alloc::rc::Rc;
use crate::frame_table::FrameRef;
use crate::view::View;
use crate::proc::UserProcess;
use crate::handle::{generic_allocate_handle, Handle, SMOSObject};
use core::cell::RefCell;
use smos_common::local_handle::{ObjectHandle, HandleOrHandleCap};
use smos_server::reply::SMOSReply;
use smos_common::error::InvocationError;
use smos_server::syscalls::ObjCreate;
use crate::PAGE_SIZE_4K;

pub const OBJ_MAX_FRAMES: usize = 128;

#[derive(Debug)]
pub struct AnonymousMemoryObject {
	pub size: usize,
	pub rights: sel4::CapRights,
	// sid
	pub frames: [Option<(sel4::cap::SmallPage, FrameRef)>; OBJ_MAX_FRAMES], // @alwin: It'd be good if this was bigger and structured more like a page table
	pub associated_views: Vec<Rc<RefCell<View>>>
}

pub fn handle_obj_create(p: &mut UserProcess, args: &ObjCreate) ->
	Result<SMOSReply, InvocationError> {

	/* The root server only supports the creation of anonymous memory objects */
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

    let (idx, handle_ref, cptr) = generic_allocate_handle(p, args.return_cap)?;

    let mem_obj = Rc::new( RefCell::new( AnonymousMemoryObject {
    	size: args.size,
    	rights: args.rights.clone(),
    	frames: [None; OBJ_MAX_FRAMES],
    	associated_views: Vec::new()
    }));

    *handle_ref = Some(Handle::new(SMOSObject::Object(mem_obj)));
    // @alwin: Do we need a list of memory objects in a process? - probs not

    let ret_value = if args.return_cap {
    	HandleOrHandleCap::<ObjectHandle>::new_handle_cap(cptr.unwrap())
    } else {
    	HandleOrHandleCap::<ObjectHandle>::new_handle(idx)
    };

    return Ok(SMOSReply::ObjCreate{hndl: ret_value});


}