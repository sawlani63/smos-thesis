use sel4_sys::seL4_MessageInfo;
use alloc::boxed::Box;
use downcast_rs::{Downcast, impl_downcast};
use smos_common::{error::{*}, args::{*}, invocations::SMOSInvocation, connection::{*}};
use core::marker::PhantomData;

// @alwin: Instead of having a different invocation in the API, maybe we determine handle vs handle cap variant
// based on args. This is like it would prevent the api from blowing up as much.

// Trait definition
// @alwin: I'm not sure what is cleaner, this or a big-ass enum with all possible invocations.
// I *think* this feels easier to write at least, but requires minor heap allocation and a crate
// for downcasting
trait SMOS_Invocation_Data: Downcast + core::fmt::Debug {}
impl_downcast!(SMOS_Invocation_Data);

// Data structs
#[derive(Debug)]
pub struct WindowCreate {
	pub base_vaddr: u64,
	pub size: usize,
	pub return_cap: bool
}

#[derive(Debug)]
pub struct ObjCreate {
	// pub path: &str, I think the raw API should pass in an offset and maybe a size and this
	//				   should maybe be converted to a &str?
	// pub attr: ?
	pub size: usize,
	pub sid: usize,
	pub return_cap: bool
}

#[derive(Debug)]
pub struct ObjOpen {
	// pub path: &str?
	// pub attr: ?
	// pub is_cap: bool
}


// @alwin: I think the raw invocation will have one more argument
// to determine whetehr the window or object cap was passed in
// if only one was passed in
#[derive(Debug)]
pub struct ObjView {
	pub window: HandleOrHandleCap,
	pub object: HandleOrHandleCap,
	pub window_offset: usize,
	pub obj_offset: usize,
	pub size: usize,
	// pub flags: todo!(),
	pub return_cap: bool
}

#[derive(Debug)]
enum HandleOrHandleCap {
	Handle(usize),
	HandleCap(usize)
}

impl SMOS_Invocation_Data for WindowCreate {}
impl SMOS_Invocation_Data for ObjCreate {}
impl SMOS_Invocation_Data for ObjView {}
impl SMOS_Invocation_Data for ObjOpen {}

// General invocation enum
#[derive(Debug)]
pub enum SMOS_Invocation {
	WindowCreate(WindowCreate),
	ObjCreate(ObjCreate),
	ObjOpen(ObjOpen),
	ObjView(ObjView),
}

/* @alwin: Figure out how to autogenerate these */
const ROOT_SERVER_INVOCATIONS: [SMOSInvocation; 3] = 	[ SMOSInvocation::ConnCreate,
												      	  SMOSInvocation::ConnDestroy,
												      	  SMOSInvocation::TestSimple];
const OBJECT_SERVER_INVOCATIONS: [SMOSInvocation; 2] = 	[ SMOSInvocation::ConnOpen,
													  	  SMOSInvocation::ConnClose];
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

impl SMOS_Invocation {
	pub fn new<T: ServerConnection>(ipc_buffer: &sel4::IpcBuffer, info: &sel4::MessageInfo) -> Result<Self, InvocationError> {
		let res = SMOS_Invocation_Raw::get_from_ipc_buffer::<T>(info, ipc_buffer)?;

		Ok(
			match info.label().try_into().or(Err(InvocationError::InvalidInvocation))? {
				SMOSInvocation::WindowCreate => SMOS_Invocation::WindowCreate(*(res.downcast::<WindowCreate>().expect("mismatch"))),
				SMOSInvocation::ObjOpen => SMOS_Invocation::ObjOpen(*(res.downcast::<ObjOpen>().expect("mismatch"))),
				SMOSInvocation::ObjView => SMOS_Invocation::ObjView(*(res.downcast::<ObjView>().expect("mismatch"))),
				SMOSInvocation::ObjCreate => SMOS_Invocation::ObjCreate(*(res.downcast::<ObjCreate>().expect("mismatch"))),
				_ => panic!("Not handled yet!")
			}
		)
	}
}

mod SMOS_Invocation_Raw {
	use sel4_sys::seL4_MessageInfo;
	use alloc::boxed::Box;
	use crate::syscalls::{*};

	pub fn get_from_ipc_buffer<T: ServerConnection>(info: &sel4::MessageInfo, ipcbuf: &sel4::IpcBuffer) -> Result<Box<dyn SMOS_Invocation_Data>, InvocationError> {
		if !T::is_supported(info.label().try_into().or(Err(InvocationError::InvalidInvocation))?) {
			return Err(InvocationError::UnsupportedInvocation {label: info.label().try_into().unwrap() });
		}

		get_with(info.label(), info.length().try_into().unwrap(), |i| {
			ipcbuf.msg_regs()[i as usize]
		})
	}

	pub fn get_with(label: sel4_sys::seL4_Word, length: sel4_sys::seL4_Word,
					f: impl Fn(core::ffi::c_ulong) -> sel4_sys::seL4_Word) -> Result<Box<dyn SMOS_Invocation_Data>, InvocationError> {
		match label.try_into().or(Err(InvocationError::InvalidInvocation))? {
			SMOSInvocation::WindowCreate => {
				Ok(Box::new(WindowCreate {
					base_vaddr: f(WindowCreateArgs::Base_Vaddr as u64).try_into().unwrap(), // @alwin: if there is a type mismatch, it shouldn't panic
					size: f(WindowCreateArgs::Size as u64).try_into().unwrap(),
					return_cap: f(WindowCreateArgs::ReturnCap as u64) != 0 // @alwin: hmm?
				}))
			},
			SMOSInvocation::ObjCreate => todo!(),
			SMOSInvocation::ObjOpen => todo!(),
			SMOSInvocation::ObjView => todo!(),
			SMOSInvocation::ObjView => todo!(),
			SMOSInvocation::TestSimple => {
				panic!("Okay got to test simple");
			}
			_ => {
				panic!("Not handled")
			}
		}
	}
}