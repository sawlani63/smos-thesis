use smos_common::local_handle::{HandleOrHandleCap, Handle, HandleCap, WindowHandle, ObjectHandle,
								ConnectionHandle, HandleType, ViewHandle};
use smos_common::error::InvocationErrorLabel;

#[derive(Debug)]
pub enum SMOSReply {
	WindowCreate {
		hndl: HandleOrHandleCap<WindowHandle>
	},
	WindowDestroy,
	ConnCreate {
		hndl: Handle<ConnectionHandle>,
		ep: sel4::cap::Endpoint
	},
	View {
		hndl: Handle<ViewHandle>
	},
	ObjCreate {
		hndl: HandleOrHandleCap<ObjectHandle>
	},
}

pub fn match_hndl_or_hndl_cap<T: HandleType>(hndl: HandleOrHandleCap<T>, ipc_buf: &mut sel4::IpcBuffer,
								 msginfo: sel4::MessageInfoBuilder) -> sel4::MessageInfoBuilder {

	match hndl {
		HandleOrHandleCap::Handle(Handle{idx, ..} ) => {
			ipc_buf.msg_regs_mut()[0] = idx as u64;
			msginfo.length(1)
		},
		HandleOrHandleCap::HandleCap(HandleCap{cptr, ..}) => {
			ipc_buf.caps_or_badges_mut()[0] = cptr.path().bits();
			msginfo.extra_caps(1)
		}
	}
}

pub fn handle_reply(ipc_buf: &mut sel4::IpcBuffer, reply_type: SMOSReply) -> sel4::MessageInfo {
	let mut msginfo = sel4::MessageInfoBuilder::default().label(InvocationErrorLabel::NoError.into());
	match reply_type {
		SMOSReply::WindowCreate{hndl} => {
			msginfo = match_hndl_or_hndl_cap(hndl, ipc_buf, msginfo);
		},
		SMOSReply::ObjCreate{hndl} => {
			msginfo = match_hndl_or_hndl_cap(hndl, ipc_buf, msginfo);
		}
		SMOSReply::ConnCreate{hndl, ep} => {
			msginfo = msginfo.length(1).extra_caps(1);
			ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
			ipc_buf.caps_or_badges_mut()[0] = ep.bits()
		},
		SMOSReply::View{hndl} => {
			msginfo = msginfo.length(1);
			ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
		}
		SMOSReply::WindowDestroy => {},
		_ => panic!("Not handled yet"),
	}

	return msginfo.build();
}