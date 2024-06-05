use crate::handle::Handle;
use smos_common::error::InvocationError;
use crate::cspace::{CSpace, CSpaceTrait};

const MAX_HANDLE_CAPS: usize = 512;

const ARRAY_REPEAT_VALUE: HandleCapability = HandleCapability {handle: None, root_cap: None};

struct HandleCapability {
	handle: Option<Handle>,
	root_cap: Option<sel4::AbsoluteCPtr>
}

static mut handle_caps: [HandleCapability; MAX_HANDLE_CAPS] = [ARRAY_REPEAT_VALUE; MAX_HANDLE_CAPS];

pub fn initialise_handle_cap_table(cspace: &mut CSpace, ep: sel4::cap::Endpoint) -> Result<(), sel4::Error>{
	unsafe {
		for (i, handle_cap) in handle_caps.iter_mut().enumerate() {
			let tmp = cspace.alloc_slot()?;

			// @alwin: Think more about what badge these get. Maybe OR them with some handle cap bit
			// so they can't be spoofed from normal endpoint caps
			cspace.root_cnode().relative_bits_with_depth(tmp.try_into().unwrap(), sel4::WORD_SIZE)
							   .mint(&cspace.root_cnode().relative(ep),
							   		 sel4::CapRightsBuilder::none().build(), i.try_into().unwrap());

			(*handle_cap).root_cap = Some(cspace.root_cnode().relative(ep));
		}
	}

	Ok(())
}

pub fn allocate_handle_cap() -> Result<(usize, &'static mut Option<Handle>, Option<sel4::AbsoluteCPtr>), InvocationError> {
	unsafe {
		for (i, handle_cap) in handle_caps.iter_mut().enumerate() {
			if (handle_cap.handle.is_none()) {
				return Ok((i, &mut handle_cap.handle, handle_cap.root_cap))
			}
		}

		return Err(InvocationError::OutOfHandleCaps);
	}
}

pub fn get_handle_cap_mut(idx: usize) -> Result<&'static mut Option<Handle>, ()> {
	if idx >= MAX_HANDLE_CAPS {
		return Err(())
	}

	return unsafe { Ok(&mut handle_caps[idx].handle) };
}

pub fn cleanup_handle_cap(idx: usize) -> Result<(), ()> {
	if idx >= MAX_HANDLE_CAPS {
		return Err(())
	}

	unsafe {
		// @alwin: Should probs not be an assert
		assert!(handle_caps[idx].root_cap.is_some());
		handle_caps[idx].root_cap.unwrap().revoke();
		handle_caps[idx].handle = None;
	}
	return Ok(());
}

pub fn deallocate_handle_cap() {
	todo!()
}