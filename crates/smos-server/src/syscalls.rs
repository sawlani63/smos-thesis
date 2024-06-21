use sel4_sys::seL4_MessageInfo;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use downcast_rs::{Downcast, impl_downcast};
use smos_common::{error::{*}, args::{*}, invocations::SMOSInvocation, connection::{*}};
use core::marker::PhantomData;
use smos_common::local_handle::{HandleOrHandleCap, WindowHandle, ObjectHandle};
use smos_common::server_connection::ServerConnection;
use crate::handle_arg::{ServerReceivedHandleOrHandleCap, ReceivedHandle, UnwrappedHandleCap};
use core::ffi::CStr;
use sel4_bitfield_ops::Bitfield;
use sel4::AbsoluteCPtr;

// Data structs
#[derive(Debug)]
pub struct WindowCreate {
	pub base_vaddr: u64,
	pub size: usize,
	pub return_cap: bool
}

#[derive(Debug)]
pub struct WindowDestroy {
	pub hndl: ServerReceivedHandleOrHandleCap
}

#[derive(Debug)]
pub struct ObjCreate {
	pub name: Option<String>,
	pub size: usize,
	pub rights: sel4::CapRights,
	pub return_cap: bool
}

#[derive(Debug)]
pub struct ObjStat {
	pub hndl: ServerReceivedHandleOrHandleCap
}

#[derive(Debug)]
pub struct ObjOpen {
	pub name: String,
	pub rights: sel4::CapRights,
	pub return_cap: bool
}

#[derive(Debug)]
pub struct ObjClose {
	pub hndl: ServerReceivedHandleOrHandleCap
}

#[derive(Debug)]
pub struct ObjDestroy {
	pub hndl: ServerReceivedHandleOrHandleCap
}

#[derive(Debug)]
pub struct ConnCreate {
	pub name: String
}

#[derive(Debug)]
pub struct ConnOpen {
	pub shared_buf_obj: Option<(ServerReceivedHandleOrHandleCap, usize)>
}

#[derive(Debug)]
pub struct ConnPublish {
	pub ntfn_buffer: usize,
	pub name: String
}

#[derive(Debug)]
pub struct ProcessSpawn {
	// executable name
	// file server name
	// argv
}

#[derive(Debug)]
pub struct ConnRegister {
	pub publish_hndl: ReceivedHandle,
	pub client_id: usize,
}

#[derive(Debug)]
pub struct ConnDeregister {
	pub hndl: ReceivedHandle,
}

#[derive(Debug)]
pub struct WindowRegister {
	pub publish_hndl: ReceivedHandle,
	pub window_hndl: UnwrappedHandleCap,
	pub reference: usize
}

#[derive(Debug)]
pub struct WindowDeregister {
	pub hndl: ReceivedHandle
}

#[derive(Debug)]
pub struct View {
	pub window: ServerReceivedHandleOrHandleCap,
	pub object: ServerReceivedHandleOrHandleCap,
	pub window_offset: usize,
	pub obj_offset: usize,
	pub size: usize,
	pub rights: sel4::CapRights,
}

#[derive(Debug)]
pub struct Unview {
	pub hndl: ReceivedHandle
}

#[derive(Debug)]
pub struct PageMap {
	pub window_registration_hndl: ReceivedHandle,
	pub view_offset: usize,
	pub content_vaddr: usize
}

#[derive(Debug)]
pub struct ConnDestroy {
	pub hndl: ReceivedHandle
}

#[derive(Debug)]
pub struct LoadComplete {
	pub entry_point: usize
}

// General invocation enum
#[derive(Debug)]
pub enum SMOS_Invocation {
	WindowCreate(WindowCreate),
	WindowDestroy(WindowDestroy),
	ObjCreate(ObjCreate),
	ObjOpen(ObjOpen),
	ObjStat(ObjStat),
	ObjClose(ObjClose),
	ObjDestroy(ObjDestroy),
	View(View),
	Unview(Unview),
	ConnCreate(ConnCreate),
	ConnDestroy(ConnDestroy),
	ConnOpen(ConnOpen),
	ConnClose,
	ConnPublish(ConnPublish),
	ConnRegister(ConnRegister),
	ConnDeregister(ConnDeregister),
	ReplyCreate,
	ProcessSpawn(ProcessSpawn),
	WindowRegister(WindowRegister),
	WindowDeregister(WindowDeregister),
	PageMap(PageMap),
	LoadComplete(LoadComplete)
}


impl<'a> SMOS_Invocation {
	pub fn new<T: ServerConnection>(ipc_buffer: &sel4::IpcBuffer, info: &sel4::MessageInfo, data_buffer: Option<&[u8]>, recv_slot: AbsoluteCPtr) -> Result<SMOS_Invocation, InvocationError> {
		return SMOS_Invocation_Raw::get_from_ipc_buffer::<T>(info, ipc_buffer, data_buffer, recv_slot);
	}
}

mod SMOS_Invocation_Raw {
	use sel4_sys::seL4_MessageInfo;
	use alloc::boxed::Box;
	use crate::syscalls::{*};

	pub fn get_from_ipc_buffer<T: ServerConnection>(info: &sel4::MessageInfo, ipcbuf: &sel4::IpcBuffer, data_buffer: Option<&[u8]>, recv_slot: AbsoluteCPtr) -> Result<SMOS_Invocation, InvocationError> {
		if !T::is_supported(info.label().try_into().or(Err(InvocationError::InvalidInvocation))?) {
			return Err(InvocationError::UnsupportedInvocation {label: info.label().try_into().unwrap() });
		}

		get_with(info, |i| { ipcbuf.msg_regs()[i as usize]}, |i| { ipcbuf.caps_or_badges()[i as usize]}, data_buffer, recv_slot)
	}

	// @alwin: This is all kind of very ugly and very manual, but if we want to keep the API minimal, I think this is the only way
	pub fn get_with(info: &sel4::MessageInfo,
					f_msg: impl Fn(core::ffi::c_ulong) -> sel4_sys::seL4_Word,
					f_cap: impl Fn(core::ffi::c_ulong) -> sel4_sys::seL4_Word,
					data_buffer: Option<&[u8]>,
					recv_slot: AbsoluteCPtr) -> Result<SMOS_Invocation, InvocationError> {

		match info.label().try_into().or(Err(InvocationError::InvalidInvocation))? {
			SMOSInvocation::WindowCreate => {
				Ok(SMOS_Invocation::WindowCreate(
					WindowCreate {
						base_vaddr: f_msg(WindowCreateArgs::Base_Vaddr as u64).try_into().unwrap(), // @alwin: if there is a type mismatch, it shouldn't panic
						size: f_msg(WindowCreateArgs::Size as u64).try_into().unwrap(),
						return_cap: f_msg(WindowCreateArgs::ReturnCap as u64) != 0 // @alwin: hmm?
				}))
			},
			SMOSInvocation::WindowDestroy => {
				let val = if info.extra_caps() == 1 && info.caps_unwrapped() == 1 {
					Ok(ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(f_cap(WindowDestroyArgs::Handle as u64) as usize))
				} else if info.length() == 1 {
					Ok(ServerReceivedHandleOrHandleCap::new_handle(f_msg(WindowDestroyArgs::Handle as u64) as usize))
				} else {
					Err(InvocationError::InvalidArguments)
				}?;

				Ok(SMOS_Invocation::WindowDestroy(
					WindowDestroy {
						hndl: val
				}))
			},
			SMOSInvocation::ObjClose => {
				let val = if info.extra_caps() == 1 && info.caps_unwrapped() == 1 {
					Ok(ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(f_cap(0) as usize))
				} else if info.length() == 1 {
					Ok(ServerReceivedHandleOrHandleCap::new_handle(f_msg(0) as usize))
				} else {
					Err(InvocationError::InvalidArguments)
				}?;

				Ok(SMOS_Invocation::ObjClose(
					ObjClose {
						hndl: val
				}))
			},
			SMOSInvocation::ObjDestroy => {
				let val = if info.extra_caps() == 1 && info.caps_unwrapped() == 1 {
					Ok(ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(f_cap(0) as usize))
				} else if info.length() == 1 {
					Ok(ServerReceivedHandleOrHandleCap::new_handle(f_msg(0) as usize))
				} else {
					Err(InvocationError::InvalidArguments)
				}?;

				Ok(SMOS_Invocation::ObjDestroy(
					ObjDestroy {
						hndl: val
				}))
			},
			SMOSInvocation::WindowRegister => {
				if info.extra_caps() != 1 || info.caps_unwrapped() != 1 || info.length() != 2 {
					return Err(InvocationError::InvalidArguments);
				}

				Ok(SMOS_Invocation::WindowRegister(
					WindowRegister {
						publish_hndl: ReceivedHandle::new(f_msg(0) as usize),
						window_hndl: UnwrappedHandleCap::new(f_cap(0) as usize),
						reference: f_msg(1) as usize
				}))
			},
			SMOSInvocation::WindowDeregister => {
				if info.length() != 1 {
					return Err(InvocationError::InvalidArguments)?;
				}

				Ok(SMOS_Invocation::WindowDeregister(
					WindowDeregister {
						hndl: ReceivedHandle::new(f_msg(0) as usize)
				}))
			}
			SMOSInvocation::PageMap => {
				if info.length() != 3 {
					return Err(InvocationError::InvalidArguments);
				}

				Ok(SMOS_Invocation::PageMap(
					PageMap {
						window_registration_hndl: ReceivedHandle::new(f_msg(0) as usize),
						view_offset: f_msg(1) as usize,
						content_vaddr: f_msg(2) as usize
					}
				))
			}
			SMOSInvocation::ConnCreate => {
				if data_buffer.is_none() {
					return Err(InvocationError::DataBufferNotSet);
				}

				// @alwin: This should not do to_string(), because that has an unnecessary copy.
				// Doing it properly involves lifetime wrangling that I do not want to deal with
				// today.

				// @alwin: idk about this, let's see
				Ok(SMOS_Invocation::ConnCreate(
					ConnCreate {
						name: unsafe { CStr::from_ptr(data_buffer.unwrap().as_ptr() as *const i8).to_str().expect("@alwin: This should not be an expect").to_string() },
				}))
			},
			SMOSInvocation::ConnPublish => {
				if data_buffer.is_none() {
					return Err(InvocationError::DataBufferNotSet);
				}

				Ok(SMOS_Invocation::ConnPublish(
					ConnPublish {
						ntfn_buffer: f_msg(0) as usize,
						name: unsafe { CStr::from_ptr(data_buffer.unwrap().as_ptr() as *const i8).to_str().expect("@alwin: This should not be an expect").to_string() },
				}))
			}
			SMOSInvocation::ObjCreate => {
				let name = if f_msg(ObjCreateArgs::HasName as u64) != 0 {
					if data_buffer.is_none() {
						return Err(InvocationError::DataBufferNotSet);
					}

					unsafe { Some(CStr::from_ptr(data_buffer.unwrap().as_ptr() as *const i8).to_str().expect("@alwin: This should not be an expect").to_string()) }
				} else {
					None
				};

				Ok(SMOS_Invocation::ObjCreate(
					ObjCreate {
						name: name,
						size: f_msg(ObjCreateArgs::Size as u64) as usize,
						rights: sel4::CapRights::from_inner(sel4_sys::seL4_CapRights{ 0: Bitfield::new([f_msg(ObjCreateArgs::Rights as u64)]) }),
						return_cap: f_msg(ObjCreateArgs::ReturnCap as u64) != 0,
				}))

			},
			SMOSInvocation::ObjOpen => {
				if data_buffer.is_none() {
					return Err(InvocationError::DataBufferNotSet);
				}

				let name = unsafe { CStr::from_ptr(data_buffer.unwrap().as_ptr() as *const i8).to_str().expect("@alwin: This should not be an expect").to_string() };

				Ok(SMOS_Invocation::ObjOpen(
					ObjOpen {
						name: name,
						rights: sel4::CapRights::from_inner(sel4_sys::seL4_CapRights{0: Bitfield::new([f_msg(0)])}),
						return_cap: f_msg(1) != 0,
					}
				))
			},
			SMOSInvocation::ObjStat => {
				let hndl = if info.extra_caps() == 1 {
					if info.caps_unwrapped() != (1 << 0) {
						/* Obj stat should only be called with objects provided by the server
						   being called into */
						return Err(InvocationError::InvalidArguments);
					}

					Ok(ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(f_cap(0) as usize))
				} else if info.length() == 1 {
					Ok(ServerReceivedHandleOrHandleCap::new_handle(f_msg(0) as usize))
				} else {
					Err(InvocationError::InvalidArguments)
				}?;

				Ok(SMOS_Invocation::ObjStat(
					ObjStat {
						hndl: hndl
				}))
			}
			SMOSInvocation::View => {
				let window: ServerReceivedHandleOrHandleCap;

				let mut cap_arg_counter: u64 = 0;

				let window_buf = f_msg(ViewArgs::Window as u64);
				if window_buf == u64::MAX {
					if info.extra_caps() < (cap_arg_counter + 1).try_into().unwrap() {
						return Err(InvocationError::InvalidArguments);
					}

					if info.caps_unwrapped() & (1 << cap_arg_counter) != 0 {
						/* Capability was unwrapped */
						window = ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(f_cap(cap_arg_counter) as usize);
					} else {
						/* Capability was not unwrapped */
						// @alwin: Need to extend ServerReceivedHandleOrHandleCap to deal with this
						window = ServerReceivedHandleOrHandleCap::new_wrapped_handle_cap(recv_slot)
					}
					cap_arg_counter += 1;
				} else {
					window = ServerReceivedHandleOrHandleCap::new_handle(window_buf as usize)
				}

				let object: ServerReceivedHandleOrHandleCap;

				let object_buf = f_msg(ViewArgs::Object as u64);
				if object_buf == u64::MAX {
					if info.extra_caps() < (cap_arg_counter + 1).try_into().unwrap() {
						return Err(InvocationError::InvalidArguments);
					}

					if info.caps_unwrapped() & (1 << cap_arg_counter) != 0 {
						/* Capability was unwrapped */
						object = ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(f_cap(cap_arg_counter) as usize);
					} else {
						/* Capability was not unwrapped */
						// @alwin: Double check that this is invalid
						return Err(InvocationError::InvalidArguments);
					}
					cap_arg_counter += 1;
				} else {
					object = ServerReceivedHandleOrHandleCap::new_handle(object_buf as usize)
				}


				Ok(SMOS_Invocation::View(
					View {
						window: window,
						object: object,
						window_offset: f_msg(ViewArgs::WinOffset as u64) as usize,
						obj_offset: f_msg(ViewArgs::ObjOffset as u64) as usize,
						size: f_msg(ViewArgs::Size as u64) as usize,
						rights: sel4::CapRights::from_inner(sel4_sys::seL4_CapRights{ 0: Bitfield::new([f_msg(ViewArgs::Rights as u64)]) })
				}))
			},
			SMOSInvocation::Unview => {
				if info.length() != 1 {
					return Err(InvocationError::InvalidArguments);
				}

				Ok(SMOS_Invocation::Unview(
					Unview {
						hndl: ReceivedHandle::new(f_msg(0) as usize)
					}
				))
			},
			SMOSInvocation::ConnDestroy => {
				if info.length() != 1 {
					return Err(InvocationError::InvalidArguments);
				}

				Ok(SMOS_Invocation::ConnDestroy(
					ConnDestroy {
						hndl: ReceivedHandle::new(f_msg(0) as usize)
					}
				))
			},
			SMOSInvocation::ConnDeregister => {
				if info.length() != 1 {
					return Err(InvocationError::InvalidArguments);
				}

				Ok(SMOS_Invocation::ConnDeregister(
					ConnDeregister {
						hndl: ReceivedHandle::new(f_msg(0) as usize)
					}
				))
			},
			SMOSInvocation::LoadComplete => {
				if info.length() != 1 {
					return Err(InvocationError::InvalidArguments);
				}

				Ok(SMOS_Invocation::LoadComplete(
					LoadComplete {
						entry_point: f_msg(0) as usize
					}
				))
			}
			SMOSInvocation::ConnOpen => {
				let object: Option<(ServerReceivedHandleOrHandleCap, usize)>;
				if info.length() == 0 {
					object = None;
				} else if info.extra_caps() == 1 {
					if info.caps_unwrapped() & 1 != 0 {
						object = Some((
								ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(f_cap(0) as usize),
								f_msg(1) as usize
								));

					} else {
						object = Some((
									ServerReceivedHandleOrHandleCap::new_wrapped_handle_cap(recv_slot),
									f_msg(1) as usize
									))
					}
				} else {
					object = Some((
						ServerReceivedHandleOrHandleCap::new_handle(f_msg(0) as usize),
						f_msg(1) as usize
						));
				}

				Ok(SMOS_Invocation::ConnOpen({
					ConnOpen {
						shared_buf_obj: object,
					}
				}))
			},
			SMOSInvocation::ConnClose => {
				return Ok(SMOS_Invocation::ConnClose);
			}
			SMOSInvocation::ConnRegister => {
				if info.length() != 2 {
					/* Idk, some kind of error? */
					todo!()
				}

				Ok(SMOS_Invocation::ConnRegister(
					ConnRegister {
						publish_hndl: ReceivedHandle::new(f_msg(0) as usize),
						client_id: f_msg(1) as usize,
					}
				))
			},
			SMOSInvocation::ReplyCreate => {
				Ok(SMOS_Invocation::ReplyCreate)
			},
			SMOSInvocation::ProcSpawn => {
				// @alwin: Add the argument unmarshalling here
				Ok(SMOS_Invocation::ProcessSpawn(ProcessSpawn {}))
			},
			SMOSInvocation::TestSimple => {
				panic!("Okay got to test simple");
			}
			_ => {
				panic!("Not handled {:?}", SMOSInvocation::try_from(info.label()));
			}
		}
	}
}