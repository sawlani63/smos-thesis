use alloc::vec::Vec;
use alloc::vec;
use smos_common::util::BIT;
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
use smos_server::handle::{generic_allocate_handle, generic_get_handle, generic_cleanup_handle,
						  generic_invalid_handle_error, ServerHandle};
use smos_server::handle_capability::HandleCapabilityTable;
use smos_server::handle_arg::ServerReceivedHandleOrHandleCap;


/* Each level of the page table uses 9 bits, just like the underlying page table structure. Realistically,
   this is kind of unnecessary, because no objects of this size should ever need to be allocated, but
   it lets us more easily use large and huge pages. @alwin: In future, see if this can be redesigned
   to use less bits per level to save space without making it a pain to use

	| 9 | 9 | 9 | 9 | 12
   */

// @alwin: Just increasing this is eventually going to be problematic
pub const OBJ_LVL_MAX: usize = 512;
const NUM_FRAME_TABLE_LEVELS: u32 = 4;

// @alwin: I don't think we actually want the max object size to be this big
pub const MAX_OBJ_SIZE: usize = OBJ_LVL_MAX.pow(NUM_FRAME_TABLE_LEVELS) * PAGE_SIZE_4K;

#[derive(Clone, Debug)]
pub struct ObjectFrame {
	pub cap: sel4::cap::SmallPage,
	pub frame_ref: FrameRef
}

#[derive(Clone, Debug)]
pub struct ObjectFrameTable {
	pub table: Vec<Option<ObjectFrameTableEntry>>
}

#[derive(Clone, Debug)]
pub enum ObjectFrameTableEntry {
	Frame(ObjectFrame),
	FrameTable(ObjectFrameTable)
}

#[derive(Debug)]
pub struct AnonymousMemoryObject {
	pub size: usize,
	pub rights: sel4::CapRights,
	// sid
	frames: Vec<Option<ObjectFrameTableEntry>>,
	pub associated_views: Vec<Rc<RefCell<View>>>
}


impl AnonymousMemoryObject {
	pub fn new(size: usize, rights: sel4::CapRights) -> AnonymousMemoryObject {
		AnonymousMemoryObject {
			size: size,
			rights: rights,
			frames: vec![None; OBJ_LVL_MAX],
			associated_views: Vec::new()
		}
	}

	pub fn lookup_frame<'a>(&'a self, offset: usize) -> Option<&'a ObjectFrame> {
		let mut shift = 39;
		let mut curr_table_lvl = 0;
		let mut curr_table = &self.frames;
		while curr_table_lvl < 4 {
			let idx = (offset >> shift) & (BIT(9) - 1);

			curr_table = match &curr_table[idx] {
				None => return None,
				Some(x) => {
					match x {
						ObjectFrameTableEntry::Frame(ref y) => return Some(&y),
						ObjectFrameTableEntry::FrameTable(y) => &y.table
					}
				}
			};

			curr_table_lvl +=  1;
			shift -= 9;
		}

		return None;
	}

	pub fn insert_frame_at(&mut self, offset: usize, frame: (sel4::cap::SmallPage, FrameRef)) -> Result<(), sel4::Error> {
		let mut shift = 39;
		let mut curr_table_lvl = 0;
		let mut curr_table = &mut self.frames;
		while curr_table_lvl < 3 {
			let idx = (offset >> shift) & (BIT(9) - 1);

			if curr_table[idx].is_none() {
				curr_table[idx] = Some(ObjectFrameTableEntry::FrameTable(ObjectFrameTable { table: vec![None; OBJ_LVL_MAX] }));
			}

			curr_table = match &mut curr_table[idx] {
				None => return Err(sel4::Error::InvalidArgument), // @alwin: What to actually return here?
				Some(ref mut x) => {
					match x {
						ObjectFrameTableEntry::Frame(_) => return Err(sel4::Error::DeleteFirst),
						ObjectFrameTableEntry::FrameTable(ref mut y) => &mut y.table
					}
				}
			};

			curr_table_lvl +=  1;
			shift -= 9;
		}

		let idx = (offset >> shift) & (BIT(9) - 1);
		if curr_table[idx].is_none() {
			curr_table[idx] = Some(ObjectFrameTableEntry::Frame(ObjectFrame {cap: frame.0, frame_ref: frame.1}));
			return Ok(());
		}

		match &curr_table[idx] {
			None => panic!("This should have already been handled above"),
			Some(x) => {
				match x {
					ObjectFrameTableEntry::Frame(_) => return Err(sel4::Error::DeleteFirst),
					ObjectFrameTableEntry::FrameTable(_) => panic!("Internal error: FrameTable on bottom level should not occur")
				}
			}
		}
	}

	pub fn cleanup_obj_table_inner(vec: &Vec<Option<ObjectFrameTableEntry>>, cspace: &mut CSpace,
								  frame_table: &mut FrameTable, revoke: bool) {
		for node in vec {
			match node {
				None => continue,
				Some(x) => {
					match x {
						ObjectFrameTableEntry::FrameTable(ref y) => Self::cleanup_obj_table_inner(&y.table, cspace, frame_table, revoke),
						ObjectFrameTableEntry::Frame(ref y) => {
							if revoke {
								cspace.root_cnode.relative(y.cap).revoke();
							}
							frame_table.free_frame(y.frame_ref);
						}
					}
				}
			}
		}
	}

	pub fn cleanup_frame_table(&mut self, cspace: &mut CSpace, frame_table: &mut FrameTable) {
		// @alwin: Double check the revoke condition
		Self::cleanup_obj_table_inner(&self.frames, cspace, frame_table, self.associated_views.len() != 0)
	}
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
	if args.size / PAGE_SIZE_4K >= MAX_OBJ_SIZE {
		return Err(InvocationError::InvalidArguments);
	}

    let (idx, handle_ref, cptr) = generic_allocate_handle(p, handle_cap_table, args.return_cap)?;

    let mem_obj = Rc::new( RefCell::new( AnonymousMemoryObject::new(args.size, args.rights.clone())));

    *handle_ref = Some(ServerHandle::new(RootServerResource::Object(mem_obj)));

    let ret_value = if args.return_cap {
    	HandleOrHandleCap::<ObjectHandle>::new_handle_cap(cptr.unwrap())
    } else {
    	HandleOrHandleCap::<ObjectHandle>::new_handle(idx)
    };

    return Ok(SMOSReply::ObjCreate{hndl: ret_value});
}

pub fn handle_obj_destroy(cspace: &mut CSpace, frame_table: &mut FrameTable, p: &mut UserProcess,
						  handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
						  args: &ObjDestroy) -> Result<SMOSReply, InvocationError> {

    /* Check that the passed in handle/cap is within bounds */
    let handle_ref = generic_get_handle(p, handle_cap_table, args.hndl, 0)?;

	    /* Check that the handle refers to is an object */
    let object = match handle_ref.as_ref().unwrap().inner() {
        RootServerResource::Object(obj) => Ok(obj.clone()),
        _ => Err(generic_invalid_handle_error(args.hndl, 0)),
    }?;

    if !object.borrow().associated_views.is_empty() {
    	// @alwin: I think we shouldn't be able to destroy objects that have views,
    	// since not everything that sets up a view with the object will have
    	// necessarily set up a ntfn buffer to tell them this has gone away
    	// under their feet.
    	todo!()
    }

    object.borrow_mut().cleanup_frame_table(cspace, frame_table);

    generic_cleanup_handle(p, handle_cap_table, args.hndl, 0)?;

    return Ok(SMOSReply::ObjDestroy);
}