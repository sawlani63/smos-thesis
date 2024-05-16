
struct SMOS_Invocation;

// @alwin: Instead of having a different invocation in the API, maybe we determine handle vs handle cap variant
// based on args. This is like it would prevent the api from blowing up as much.

// @alwin: Should this be autogenerated based on interface generator thing?
enum SMOSInvocationLabel {
	Invocation_Window_Create,
	Invocation_Obj_Create,
	Invocation_Obj_Open,
	Invocation_Obj_View
}

impl SMOS_Invocation {
	pub fn get_from_ipc_buffer(info: &seL4_MessageInfo, ipcbuf: &sel4_IPCBuffer) -> Self {
		Self::get_with(info.get_label(), info.get_length(), |i| {
			ipcbuf.msg[i as usize]
		})
	}

	pub fn get_with(label: seL4_Word, length: seL4_Word, f: impl Fn(core::ffi::c_ulong) -> seL4_Word) -> Self {
		match label {
			SMOS_Invocation_Label::WindowCreate => todo!(),
			SMOS_Invocation_Label::Invocation_Obj_Create => todo!(),
			SMOS_Invocation_Label::Invocation_Obj_Open => todo!(),
			SMOS_Invocation_Label::Invocation_Obj_View => todo!(),
			_ => {
				// @alwin: Do some kind of log and error
			}
		}
	}
}