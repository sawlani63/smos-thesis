use smos_common::error::InvocationErrorLabel;
use smos_common::local_handle::{
    ConnRegistrationHandle, ConnectionHandle, HandleCap, HandleCapHandle, HandleOrHandleCap,
    HandleType, IRQRegistrationHandle, LocalHandle, ObjectHandle, ProcessHandle, ReplyHandle,
    ViewHandle, WindowHandle, WindowRegistrationHandle,
};
use smos_common::returns::*;

#[derive(Debug)]
pub enum SMOSReply {
    WindowCreate {
        hndl: HandleOrHandleCap<WindowHandle>,
    },
    WindowDestroy,
    PageMap,
    Unview,
    WindowDeregister,
    ObjClose,
    ObjDestroy,
    ConnDestroy,
    ConnDeregister,
    LoadComplete,
    ProcessWait, // @alwin: This should probably have some kind of return code
    ConnRegister {
        hndl: LocalHandle<ConnRegistrationHandle>,
    },
    WindowRegister {
        hndl: LocalHandle<WindowRegistrationHandle>,
    },
    IRQRegister {
        hndl: LocalHandle<IRQRegistrationHandle>,
        badge_bit: u8,
    },
    ConnOpen,
    ConnClose,
    ConnCreate {
        hndl: LocalHandle<ConnectionHandle>,
        ep: sel4::cap::Endpoint,
    },
    ServerHandleCapCreate {
        hndl: LocalHandle<HandleCapHandle>,
        cap: sel4::cap::Endpoint,
    },
    ConnPublish {
        hndl: LocalHandle<ConnectionHandle>,
        ep: sel4::cap::Endpoint,
    },
    ReplyCreate {
        hndl: LocalHandle<ReplyHandle>,
        reply: sel4::cap::Reply,
    },
    View {
        hndl: LocalHandle<ViewHandle>,
    },
    ProcessSpawn {
        hndl: LocalHandle<ProcessHandle>,
    },
    ObjCreate {
        hndl: HandleOrHandleCap<ObjectHandle>,
    },
    ObjOpen {
        hndl: HandleOrHandleCap<ObjectHandle>,
    },
    ObjStat {
        data: ObjStat,
    },
}

#[derive(Debug)]
pub enum FaultReply {
    VMFault { resume: bool },
}

pub fn match_hndl_or_hndl_cap<T: HandleType>(
    hndl: HandleOrHandleCap<T>,
    ipc_buf: &mut sel4::IpcBuffer,
    msginfo: sel4::MessageInfoBuilder,
) -> sel4::MessageInfoBuilder {
    match hndl {
        HandleOrHandleCap::Handle(LocalHandle { idx, .. }) => {
            ipc_buf.msg_regs_mut()[0] = idx as u64;
            msginfo.length(1)
        }
        HandleOrHandleCap::HandleCap(HandleCap { cptr, .. }) => {
            ipc_buf.caps_or_badges_mut()[0] = cptr.path().bits();
            msginfo.extra_caps(1)
        }
    }
}

pub fn handle_reply(ipc_buf: &mut sel4::IpcBuffer, reply_type: SMOSReply) -> sel4::MessageInfo {
    let mut msginfo =
        sel4::MessageInfoBuilder::default().label(InvocationErrorLabel::NoError.into());
    match reply_type {
        SMOSReply::WindowCreate { hndl } => {
            msginfo = match_hndl_or_hndl_cap(hndl, ipc_buf, msginfo);
        }
        SMOSReply::ObjCreate { hndl } => {
            msginfo = match_hndl_or_hndl_cap(hndl, ipc_buf, msginfo);
        }
        SMOSReply::ObjOpen { hndl } => {
            msginfo = match_hndl_or_hndl_cap(hndl, ipc_buf, msginfo);
        }
        SMOSReply::ConnCreate { hndl, ep } => {
            msginfo = msginfo.length(1).extra_caps(1);
            ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
            ipc_buf.caps_or_badges_mut()[0] = ep.bits();
        }
        SMOSReply::ServerHandleCapCreate { hndl, cap } => {
            msginfo = msginfo.length(1).extra_caps(1);
            ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
            ipc_buf.caps_or_badges_mut()[0] = cap.bits();
        }
        SMOSReply::ConnPublish { hndl, ep } => {
            msginfo = msginfo.length(1).extra_caps(1);
            ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
            ipc_buf.caps_or_badges_mut()[0] = ep.bits();
        }
        SMOSReply::ReplyCreate { hndl, reply } => {
            msginfo = msginfo.length(1).extra_caps(1);
            ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
            ipc_buf.caps_or_badges_mut()[0] = reply.bits();
        }
        SMOSReply::View { hndl } => {
            msginfo = msginfo.length(1);
            ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
        }
        SMOSReply::WindowRegister { hndl } => {
            msginfo = msginfo.length(1);
            ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
        }
        SMOSReply::IRQRegister { hndl, badge_bit } => {
            msginfo = msginfo.length(2);
            ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
            ipc_buf.msg_regs_mut()[1] = badge_bit as u64;
        }
        SMOSReply::ConnRegister { hndl } => {
            msginfo = msginfo.length(1);
            ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
        }
        SMOSReply::ProcessSpawn { hndl } => {
            msginfo = msginfo.length(1);
            ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
        }
        SMOSReply::ObjStat { data } => {
            msginfo = msginfo.length(ObjStatReturn::Length as usize);
            ipc_buf.msg_regs_mut()[0] = data.size as u64;
            ipc_buf.msg_regs_mut()[1] = match data.paddr {
                Some(x) => x as u64,
                None => 0 as u64,
            };
            // @alwin: it would be nice to do this with serde or something?
        }
        SMOSReply::WindowDestroy
        | SMOSReply::ConnOpen
        | SMOSReply::PageMap
        | SMOSReply::Unview
        | SMOSReply::WindowDeregister
        | SMOSReply::ConnClose
        | SMOSReply::ObjClose
        | SMOSReply::ObjDestroy
        | SMOSReply::ConnDestroy
        | SMOSReply::LoadComplete
        | SMOSReply::ConnDeregister => {}
        _ => panic!("Not handled yet"),
    }

    return msginfo.build();
}

pub fn handle_fault_reply(
    ipc_buf: &mut sel4::IpcBuffer,
    reply_type: FaultReply,
) -> Option<sel4::MessageInfo> {
    let mut msginfo: Option<sel4::MessageInfo>;
    match reply_type {
        FaultReply::VMFault { resume } => {
            if resume {
                msginfo = Some(
                    sel4::MessageInfoBuilder::default()
                        .label(InvocationErrorLabel::NoError.into())
                        .build(),
                );
            } else {
                msginfo = None;
            }
        }
    }

    return msginfo;
}
