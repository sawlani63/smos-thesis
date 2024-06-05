use sel4::cap::Endpoint;
use sel4::{IpcBuffer, AbsoluteCPtr};
use smos_common::invocations::SMOSInvocation;
use smos_common::args::{*};
use smos_common::returns::{*};
use smos_common::connection::{*};
use smos_common::error::{*};
use smos_common::local_handle::{HandleOrHandleCap, Handle, HandleCap, WindowHandle, ViewHandle, ObjectHandle};
use core::marker::PhantomData;
use smos_cspace::SMOSUserCSpace;
use crate::connection::{*};
use crate::error::{*};

/*
 * This is kind of what I want to do:
 *		smos_conn_create() will give us an endpoint capability
 * 		It will also give us some information about what kind of endpoint capability it is
 *		Based on this, we will cast the endpoint capability (maybe using downcast or whatever) to some type
 *		We will have a bunch of traits which correspond to interfaces that are provided by certain servers
 * 		The particular type we cast it to will implement the traits that correspond to that specicic server
 *
 *		An example: The root server acts as an object server and a file server (suppose file servers have special additional invocations)
 *		There will be two traits, ObjectServerTrait (obj_open, obj_view, obj_create) and FileServerTrait (dir_open, dir_close, dir_read)
 *		The endpoint to the root server will have some ObjectFileServer: ObjectServerTrait + FileServerTrait type
 *		Then, you can invoke root_server_ep.obj_open(...) and root_server_ep.dir_open(...) and stuff
 */

// @alwin: Currently, conn_create is implemented in a way that the client knows what they are
// connecting to. In a real dynamic system, it would be better if the client didn't have to
// know this at compile time but idk if this is really THAT useful or even feasible.

pub trait RootServerInterface: ClientConnection {
	fn conn_create<T: ClientConnection>(&self, slot: &AbsoluteCPtr, server_name: &str) -> Result<T, InvocationError> {
		let (handle, endpoint) = sel4::with_ipc_buffer_mut(|ipc_buf| {
			/* Make sure the whole string fits in the buffer */
			let shared_buf = self.get_buf_mut().ok_or(InvocationError::DataBufferNotSet)?;
			if server_name.as_bytes().len() >= shared_buf.1 {
				return Err(InvocationError::BufferTooLarge);
			}
			unsafe {
				core::ptr::copy(server_name.as_bytes().as_ptr(), shared_buf.0, server_name.as_bytes().len());
			}
			ipc_buf.set_recv_slot(slot);
			let mut msginfo = sel4::MessageInfoBuilder::default()
														.label(SMOSInvocation::ConnCreate as u64)
														.length(1)
														.build();
			msginfo = self.ep().call(msginfo);
			try_unpack_error(msginfo.label(), ipc_buf)?;
			return Ok((
				ipc_buf.msg_regs()[ConnectionCreateReturn::ConnectionHandle as usize],
				sel4::CPtr::from_bits(slot.path().bits()).cast::<sel4::cap_type::Endpoint>()
			));
		})?;

		return Ok(T::new(endpoint, handle.try_into().unwrap(), None));
		// @alwin: Should we have a flag to ensure that a connection is opened prior to being used for anything?
	}

	fn conn_destroy<T: ClientConnection>(&self, conn: T) -> Result<(),  InvocationError> {
		sel4::with_ipc_buffer_mut(|ipc_buf| {
			let mut msginfo = sel4::MessageInfoBuilder::default()
														.label(SMOSInvocation::ConnDestroy as u64)
													    .length(1)
													    .build();
			// @alwin: Idk if this is better than doing msg_bytes and doing a memcpy of the arg
			// struct. It will probably be faster?
			ipc_buf.msg_regs_mut()[ConnectionDestroyArgs::Handle as usize] = conn.hndl().try_into().unwrap();
			msginfo = self.ep().call(msginfo);
			try_unpack_error(msginfo.label(), ipc_buf)?;
			return Ok(());
		})
	}

	fn test_simple(&self, msg: u64) -> Result<(), InvocationError> {
		sel4::with_ipc_buffer_mut(|ipc_buf| {
			ipc_buf.msg_regs_mut()[0] = msg;
			let mut msginfo = sel4::MessageInfoBuilder::default()
														.label(SMOSInvocation::TestSimple as u64)
														.length(1)
														.build();
			msginfo = self.ep().call(msginfo);
			try_unpack_error(msginfo.label(), ipc_buf);
			return Ok(())
		})
	}

	fn window_create(&self, base_vaddr: usize, size: usize, return_cap: Option<AbsoluteCPtr>) -> Result<HandleOrHandleCap<WindowHandle>, InvocationError> {
		let mut msginfo = sel4::MessageInfoBuilder::default()
													.label(SMOSInvocation::WindowCreate as u64)
													.length(3)
													.build();

		return sel4::with_ipc_buffer_mut(|ipc_buf| {
			// @alwin: use constants
			ipc_buf.msg_regs_mut()[0] = base_vaddr.try_into().unwrap();
			ipc_buf.msg_regs_mut()[1] = size.try_into().unwrap();
			ipc_buf.msg_regs_mut()[2] = return_cap.is_some() as u64;
			if return_cap.is_some() {
				ipc_buf.set_recv_slot(&return_cap.unwrap());
			}
			msginfo = self.ep().call(msginfo);
			try_unpack_error(msginfo.label(), ipc_buf)?;

			if return_cap.is_some() {
				if msginfo.extra_caps() != 1 || msginfo.caps_unwrapped() != 0 {
					return Err(InvocationError::ServerError)
				}
				return Ok(HandleOrHandleCap::new_handle_cap(return_cap.unwrap()));
			} else {
				if msginfo.length() != 1 {
					return Err(InvocationError::ServerError)
				}
				return Ok(HandleOrHandleCap::new_handle(ipc_buf.msg_regs()[0] as usize));
			}
		});
	}

	fn window_destroy(&self, handle: HandleOrHandleCap<WindowHandle>) -> Result<(), InvocationError> {
		let mut msginfo_builder = sel4::MessageInfoBuilder::default()
															.label(SMOSInvocation::WindowDestroy as u64);
		return sel4::with_ipc_buffer_mut(|ipc_buf| {
			msginfo_builder = match handle {
				HandleOrHandleCap::Handle( Handle {idx, ..} ) => {
					ipc_buf.msg_regs_mut()[WindowDestroyArgs::Handle as usize] = idx.try_into().unwrap();
					msginfo_builder.length(1)
				},
				HandleOrHandleCap::HandleCap( HandleCap {cptr, ..} ) => {
					ipc_buf.caps_or_badges_mut()[WindowDestroyArgs::Handle as usize] = cptr.path().bits();
					msginfo_builder.extra_caps(1)
				}
			};

			let msginfo = self.ep().call(msginfo_builder.build());
			try_unpack_error(msginfo.label(), ipc_buf)?;

			Ok(())
		});
	}
}

pub trait ObjectServerInterface: ClientConnection {
	fn conn_open() {
		todo!();
	}
	fn conn_close() {
		todo!();
	}

	fn obj_create(&self, name_opt: Option<&str>, size: usize, rights: sel4::CapRights, return_cap: Option<AbsoluteCPtr>) -> Result<HandleOrHandleCap<ObjectHandle>, InvocationError> {
		let mut msginfo = sel4::MessageInfoBuilder::default()
													.label(SMOSInvocation::ObjCreate as u64)
													.length(4)
													.build();

		return sel4::with_ipc_buffer_mut(|ipc_buf| {
			ipc_buf.msg_regs_mut()[ObjCreateArgs::HasName as usize] = name_opt.is_some() as u64;
			if name_opt.is_some() {
				let name = name_opt.unwrap();
				let shared_buf = self.get_buf_mut().ok_or(InvocationError::DataBufferNotSet)?;
				if name.as_bytes().len() >= shared_buf.1 {
					return Err(InvocationError::BufferTooLarge);
				}
				unsafe {
					core::ptr::copy(name.as_bytes().as_ptr(), shared_buf.0, name.as_bytes().len());
				}
			}
			ipc_buf.msg_regs_mut()[ObjCreateArgs::Size as usize] = size as u64;
			ipc_buf.msg_regs_mut()[ObjCreateArgs::Rights as usize] = rights.into_inner().0.bits()[0];
			ipc_buf.msg_regs_mut()[ObjCreateArgs::ReturnCap as usize] = return_cap.is_some() as u64;
			if return_cap.is_some() {
				ipc_buf.set_recv_slot(&return_cap.unwrap());
			}

			let msginfo = self.ep().call(msginfo);
			try_unpack_error(msginfo.label(), ipc_buf)?;

			if return_cap.is_some() {
				if msginfo.extra_caps() != 1 || msginfo.caps_unwrapped() != 0 {
					return Err(InvocationError::ServerError)
				}
				return Ok(HandleOrHandleCap::new_handle_cap(return_cap.unwrap()));
			} else {
				if msginfo.length() != 1 {
					return Err(InvocationError::ServerError)
				}
				return Ok(HandleOrHandleCap::new_handle(ipc_buf.msg_regs()[0] as usize));
			}
		})
	}

	fn view(&self, win: HandleOrHandleCap<WindowHandle>,  obj: HandleOrHandleCap<ObjectHandle>,
			win_offset: usize, obj_offset: usize, size: usize, rights: sel4::CapRights)
			-> Result<HandleOrHandleCap<ViewHandle>, InvocationError> {

		let mut msginfo_builder = sel4::MessageInfoBuilder::default().label(SMOSInvocation::View as u64).length(5);

		return sel4::with_ipc_buffer_mut(|ipc_buf| {
			/* Prevent stale data from sticking around in the IPC buffer, which can be dangerous when
			   skipping args */
			let mut cap_counter = 0;

			match win {
				HandleOrHandleCap::Handle(Handle {idx, ..}) => {
					ipc_buf.msg_regs_mut()[ViewArgs::Window as usize] = idx.try_into().unwrap();
				},
				HandleOrHandleCap::HandleCap(HandleCap {cptr, ..}) => {
					ipc_buf.caps_or_badges_mut()[cap_counter] = cptr.path().bits();
					ipc_buf.msg_regs_mut()[ViewArgs::Window as usize] = u64::MAX; // @alwin: Is this the way to do it?
					cap_counter += 1;
					msginfo_builder = msginfo_builder.extra_caps(cap_counter);
				}
			};

			match obj {
				HandleOrHandleCap::Handle(Handle {idx, ..}) => {
					ipc_buf.msg_regs_mut()[ViewArgs::Object as usize] = idx.try_into().unwrap();
				},
				HandleOrHandleCap::HandleCap(HandleCap {cptr, ..}) => {
					ipc_buf.caps_or_badges_mut()[cap_counter] = cptr.path().bits();
					ipc_buf.msg_regs_mut()[ViewArgs::Object as usize] = u64::MAX;
					cap_counter += 1;
					msginfo_builder = msginfo_builder.extra_caps(cap_counter)
				}
			};

			ipc_buf.msg_regs_mut()[ViewArgs::WinOffset as usize] = win_offset as u64;
			ipc_buf.msg_regs_mut()[ViewArgs::ObjOffset as usize] = obj_offset as u64;
			ipc_buf.msg_regs_mut()[ViewArgs::Size as usize] = size as u64;
			ipc_buf.msg_regs_mut()[ViewArgs::Rights as usize] = rights.into_inner().0.bits()[0];

			let msginfo = self.ep().call(msginfo_builder.build());
			try_unpack_error(msginfo.label(), ipc_buf)?;

			if msginfo.length() != 1 {
				return Err(InvocationError::ServerError)
			}
			return Ok(HandleOrHandleCap::new_handle(ipc_buf.msg_regs()[0] as usize));
		});
	}
}

pub trait FileServerInterface: ClientConnection {
	fn file_open(&self) -> Result<(), InvocationError> {
		sel4::with_ipc_buffer_mut(|ipc_buf| {
			ipc_buf.msg_regs_mut()[0] = 100;
			let mut msginfo = sel4::MessageInfoBuilder::default()
														.label(SMOSInvocation::TestSimple as u64)
														.length(1)
														.build();
			msginfo = self.ep().call(msginfo);
			try_unpack_error(msginfo.label(), ipc_buf);
			return Ok(())
		})
	}
	fn file_read() {
		todo!()
	}
	fn file_write() {
		todo!()
	}
	fn file_close() {
		todo!()
	}
}