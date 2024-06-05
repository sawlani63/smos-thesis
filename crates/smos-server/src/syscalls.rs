use sel4_sys::seL4_MessageInfo;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use downcast_rs::{Downcast, impl_downcast};
use smos_common::{error::{*}, args::{*}, invocations::SMOSInvocation, connection::{*}};
use core::marker::PhantomData;
use smos_common::local_handle::{HandleOrHandleCap, WindowHandle, ObjectHandle};
use crate::handle_arg::HandleOrUnwrappedHandleCap;
use core::ffi::CStr;
use sel4_bitfield_ops::Bitfield;

// Data structs
#[derive(Debug)]
pub struct WindowCreate {
	pub base_vaddr: u64,
	pub size: usize,
	pub return_cap: bool
}

#[derive(Debug)]
pub struct WindowDestroy {
	pub hndl: HandleOrUnwrappedHandleCap
}

#[derive(Debug)]
pub struct ObjCreate {
	pub name: Option<String>,
	pub size: usize,
	pub rights: sel4::CapRights,
	pub return_cap: bool
}

#[derive(Debug)]
pub struct ObjOpen {
	// pub path: &str?
	// pub attr: ?
	// pub is_cap: bool
}

#[derive(Debug)]
pub struct ConnCreate {
	pub name: String
}

// @alwin: I think the raw invocation will have one more argument
// to determine whetehr the window or object cap was passed in
// if only one was passed in
#[derive(Debug)]
pub struct View {
	pub window: HandleOrUnwrappedHandleCap,
	pub object: HandleOrUnwrappedHandleCap,
	pub window_offset: usize,
	pub obj_offset: usize,
	pub size: usize,
	pub rights: sel4::CapRights,
}

// General invocation enum
#[derive(Debug)]
pub enum SMOS_Invocation {
	WindowCreate(WindowCreate),
	WindowDestroy(WindowDestroy),
	ObjCreate(ObjCreate),
	ObjOpen(ObjOpen),
	View(View),
	ConnCreate(ConnCreate)
}

/* @alwin: Figure out how to autogenerate these */
const ROOT_SERVER_INVOCATIONS: [SMOSInvocation; 5] = 	[ SMOSInvocation::ConnCreate,
												      	  SMOSInvocation::ConnDestroy,
												      	  SMOSInvocation::TestSimple,
												      	  SMOSInvocation::WindowCreate,
												      	  SMOSInvocation::WindowDestroy];
const OBJECT_SERVER_INVOCATIONS: [SMOSInvocation; 4] = 	[ SMOSInvocation::ConnOpen,
													  	  SMOSInvocation::ConnClose,
													  	  SMOSInvocation::ObjCreate,
													  	  SMOSInvocation::View];
const FILE_SERVER_INVOCATION: [SMOSInvocation; 2] =  	[ SMOSInvocation::ObjOpen,
   												  	  	  SMOSInvocation::ObjClose];
trait ServerConnection {
	fn is_supported(inv: SMOSInvocation) -> bool;
}

impl ServerConnection for RootServerConnection {
	fn is_supported(inv: SMOSInvocation) -> bool {
		return ROOT_SERVER_INVOCATIONS.contains(&inv) ||
		   	   OBJECT_SERVER_INVOCATIONS.contains(&inv) ||
		   	   FILE_SERVER_INVOCATION.contains(&inv);
	}
}

impl ServerConnection for FileServerConnection {
		fn is_supported(inv: SMOSInvocation) -> bool {
		return OBJECT_SERVER_INVOCATIONS.contains(&inv) ||
		   	   FILE_SERVER_INVOCATION.contains(&inv);
	}
}

impl<'a> SMOS_Invocation {
	pub fn new<T: ServerConnection>(ipc_buffer: &sel4::IpcBuffer, info: &sel4::MessageInfo, data_buffer: Option<&[u8]>) -> Result<SMOS_Invocation, InvocationError> {
		return SMOS_Invocation_Raw::get_from_ipc_buffer::<T>(info, ipc_buffer, data_buffer);
	}
}

mod SMOS_Invocation_Raw {
	use sel4_sys::seL4_MessageInfo;
	use alloc::boxed::Box;
	use crate::syscalls::{*};

	pub fn get_from_ipc_buffer<T: ServerConnection>(info: &sel4::MessageInfo, ipcbuf: &sel4::IpcBuffer, data_buffer: Option<&[u8]>) -> Result<SMOS_Invocation, InvocationError> {
		if !T::is_supported(info.label().try_into().or(Err(InvocationError::InvalidInvocation))?) {
			return Err(InvocationError::UnsupportedInvocation {label: info.label().try_into().unwrap() });
		}

		get_with(info, |i| { ipcbuf.msg_regs()[i as usize]}, |i| { ipcbuf.caps_or_badges()[i as usize]}, data_buffer)
	}

	// @alwin: This is all kind of very ugly and very manual, but if we want to keep the API minimal, I think this is the only way
	pub fn get_with(info: &sel4::MessageInfo,
					f_msg: impl Fn(core::ffi::c_ulong) -> sel4_sys::seL4_Word,
					f_cap: impl Fn(core::ffi::c_ulong) -> sel4_sys::seL4_Word,
					data_buffer: Option<&[u8]>) -> Result<SMOS_Invocation, InvocationError> {

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
					Ok(HandleOrUnwrappedHandleCap::UnwrappedHandleCap(f_cap(WindowDestroyArgs::Handle as u64) as usize))
				} else if info.length() == 1 {
					Ok(HandleOrUnwrappedHandleCap::Handle(f_msg(WindowDestroyArgs::Handle as u64) as usize))
				} else {
					Err(InvocationError::InvalidArguments)
				}?;

				Ok(SMOS_Invocation::WindowDestroy(
					WindowDestroy {
						hndl: val
				}))
			},
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

			}
			SMOSInvocation::ObjCreate => {
				let name = if f_msg(ObjCreateArgs::HasName as u64) != 0 { // @alwin: this casting is kind of absurd
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
			SMOSInvocation::ObjOpen => todo!(),
			SMOSInvocation::View => {
				let window: HandleOrUnwrappedHandleCap;

				let mut cap_arg_counter: u64 = 0;

				let window_buf = f_msg(ViewArgs::Window as u64);
				if window_buf == u64::MAX {
					if info.extra_caps() < (cap_arg_counter + 1).try_into().unwrap() {
						return Err(InvocationError::InvalidArguments);
					}

					if info.caps_unwrapped() & (1 << cap_arg_counter) != 0 {
						/* Capability was unwrapped */
						window = HandleOrUnwrappedHandleCap::UnwrappedHandleCap(f_cap(cap_arg_counter) as usize);
					} else {
						/* Capability was not unwrapped */
						// @alwin: Need to extend HandleOrUnwrappedHandleCap to deal with this
						todo!()
					}
					cap_arg_counter += 1;
				} else {
					window = HandleOrUnwrappedHandleCap::Handle(window_buf as usize)
				}

				let object: HandleOrUnwrappedHandleCap;

				let object_buf = f_msg(ViewArgs::Object as u64);
				if object_buf == u64::MAX {
					if info.extra_caps() < (cap_arg_counter + 1).try_into().unwrap() {
						return Err(InvocationError::InvalidArguments);
					}

					if info.caps_unwrapped() & (1 << cap_arg_counter) != 0 {
						/* Capability was unwrapped */
						object = HandleOrUnwrappedHandleCap::UnwrappedHandleCap(f_cap(cap_arg_counter) as usize);
					} else {
						/* Capability was not unwrapped */
						// @alwin: I think this should never happen
						todo!()
					}
					cap_arg_counter += 1;
				} else {
					object = HandleOrUnwrappedHandleCap::Handle(object_buf as usize)
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
			SMOSInvocation::TestSimple => {
				panic!("Okay got to test simple");
			}
			_ => {
				panic!("Not handled")
			}
		}
	}
}