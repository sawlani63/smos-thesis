use sel4::cap::Endpoint;
use sel4::IpcBuffer;
use smos_common::error::{InvocationError, InvocationErrorLabel, NotEnoughArgsMessage,
						 NotEnoughCapsMessage, UnsupportedInvocationMessage};
use smos_common::invocations::SMOSInvocation;
use smos_common::args::{*};
use smos_common::returns::{*};
use smos_common::connection::{*};
use core::marker::PhantomData;
use smos_cspace::SMOSUserCSpace;

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

pub trait ClientConnection {
	fn ep(&self) -> Endpoint;
	fn new(ep: Endpoint, conn_hndl: ConnectionHandle) -> Self;
	fn hndl(&self) -> ConnectionHandle;
}

macro_rules! generate_connection_impl {
	($name:ident) => {
		impl ClientConnection for $name {
			fn ep(&self) -> Endpoint {
				self.ep
			}

			fn new(ep: Endpoint, conn_hndl: ConnectionHandle) -> Self {
				Self {
					conn_hndl: conn_hndl,
					ep: ep
				}
			}

			fn hndl(&self) -> ConnectionHandle {
				self.conn_hndl
			}
		}
	};
}

generate_connection_impl!{RootServerConnection}
impl RootServerInterface for RootServerConnection {}
impl ObjectServerInterface for RootServerConnection {}
impl FileServerInterface for RootServerConnection {}

generate_connection_impl!{FileServerConnection}
impl ObjectServerInterface for FileServerConnection {}
impl FileServerInterface for FileServerConnection{}

// @alwin: Currently, conn_create is implemented in a way that the client knows what they are
// connecting to. In a real dynamic system, it would be better if the client didn't have to
// know this at compile time but idk if this is really THAT useful or even feasible.


fn try_unpack_error(label: u64, ipc_buf: &IpcBuffer) -> Result<(), InvocationError> {
	match label.try_into().expect("This probably shouldn't panic") {
		InvocationErrorLabel::NoError => Ok(()),
		InvocationErrorLabel::InvalidInvocation => Err(InvocationError::InvalidInvocation),
		InvocationErrorLabel::NotEnoughArgs => {
			Err(InvocationError::NotEnoughArgs {
				expected: ipc_buf.msg_regs()[NotEnoughArgsMessage::Expected as usize] as usize,
				actual: ipc_buf.msg_regs()[NotEnoughArgsMessage::Actual as usize] as usize
			})
		},
		InvocationErrorLabel::NotEnoughCaps => {
			Err(InvocationError::NotEnoughCaps {
				expected: ipc_buf.msg_regs()[NotEnoughCapsMessage::Expected as usize] as usize,
				actual: ipc_buf.msg_regs()[NotEnoughCapsMessage::Actual as usize] as usize
			})
		},
		InvocationErrorLabel::InvalidType => {
			Err(InvocationError::InvalidType {
				which_arg: todo!()
			})
		},
		InvocationErrorLabel::CSpaceFull => Err(InvocationError::CSpaceFull),
		InvocationErrorLabel::UnsupportedInvocation => {
			Err(InvocationError::UnsupportedInvocation {
				// @alwin: this probably shouldn't unwrap
				label: ipc_buf.msg_regs()[UnsupportedInvocationMessage::Label as usize].try_into().unwrap(),
			})
		}
	}
}

pub trait RootServerInterface: ClientConnection {
	fn conn_create<T: ClientConnection>(&self, cspace: &mut SMOSUserCSpace, server_name: &str) -> Result<T, InvocationError> {
		let slot = cspace.alloc_slot().map_err(|_| InvocationError::CSpaceFull)?;
		let (handle, endpoint) = sel4::with_ipc_buffer_mut(|ipc_buf| {
			// @alwin: How are we dealing with strings??
			ipc_buf.set_recv_slot(&cspace.to_absolute_cptr(slot));
			let mut msginfo = sel4::MessageInfoBuilder::default()
														.label(SMOSInvocation::ConnCreate as u64)
														.length(1)
														.build();
			msginfo = self.ep().call(msginfo);
			try_unpack_error(msginfo.label(), ipc_buf)?;
			return Ok((
				ipc_buf.msg_regs()[ConnectionCreateReturn::ConnectionHandle as usize],
				sel4::CPtr::from_bits(slot.try_into().unwrap()).cast::<sel4::cap_type::Endpoint>()
			));
		})?;

		return Ok(T::new(endpoint, handle.try_into().unwrap()));
	}

	fn conn_destroy<T: ClientConnection>(&self, conn: T) -> Result<(),  InvocationError> {
		sel4::with_ipc_buffer_mut(|ipc_buf| {
			let mut msginfo = sel4::MessageInfoBuilder::default()
														.label(SMOSInvocation::ConnDestroy as u64)
													    .length(1)
													    .build();
			// @alwin: Idk if this is better than doing msg_bytes and doing a memcpy of the arg
			// struct
			ipc_buf.msg_regs_mut()[WindowDestroyArgs::ConnectionHandle as usize] = conn.hndl().try_into().unwrap();
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
}

pub trait ObjectServerInterface: ClientConnection {
	fn conn_open() {
		todo!();
	}
	fn conn_close() {
		todo!();
	}
}

pub trait FileServerInterface: ClientConnection {
	fn file_open() {
		todo!()
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