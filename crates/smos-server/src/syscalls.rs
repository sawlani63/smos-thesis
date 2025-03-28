use crate::handle_arg::{
    ReceivedHandle, ServerReceivedHandleOrHandleCap, UnwrappedHandleCap, WrappedHandleCap,
};
use alloc::vec::Vec;
use sel4::AbsoluteCPtr;
use sel4_bitfield_ops::Bitfield;
use smos_common::obj_attributes::ObjAttributes;
use smos_common::sddf::{QueueType, VirtType};
use smos_common::server_connection::ServerConnection;
use smos_common::string::rust_str_from_buffer;
use smos_common::util::BIT;
use smos_common::{args::*, error::*, invocations::SMOSInvocation};

// Data structs
#[derive(Debug)]
pub struct WindowCreate {
    pub base_vaddr: u64,
    pub size: usize,
    pub return_cap: bool,
}

#[derive(Debug)]
pub struct WindowDestroy {
    pub hndl: ServerReceivedHandleOrHandleCap,
}

#[derive(Debug)]
pub struct ObjCreate<'a> {
    pub name: Option<&'a str>,
    pub size: usize,
    pub rights: sel4::CapRights,
    pub attributes: ObjAttributes,
    pub return_cap: bool,
}

#[derive(Debug)]
pub struct ObjStat {
    pub hndl: ServerReceivedHandleOrHandleCap,
}

#[derive(Debug)]
pub struct ObjOpen<'a> {
    pub name: &'a str,
    pub rights: sel4::CapRights,
    pub return_cap: bool,
}

#[derive(Debug)]
pub struct ObjClose {
    pub hndl: ServerReceivedHandleOrHandleCap,
}

#[derive(Debug)]
pub struct ObjDestroy {
    pub hndl: ServerReceivedHandleOrHandleCap,
}

#[derive(Debug)]
pub struct ConnCreate<'a> {
    pub name: &'a str,
}

#[derive(Debug)]
pub struct ConnOpen {
    pub shared_buf_obj: Option<(ServerReceivedHandleOrHandleCap, usize)>,
}

#[derive(Debug)]
pub struct ConnPublish<'a> {
    pub ntfn_buffer: usize,
    pub name: &'a str,
}

#[derive(Debug)]
pub struct ProcessSpawn<'a> {
    pub exec_name: &'a str,
    pub fs_name: &'a str,
    pub prio: u8,
    pub args: Option<Vec<&'a str>>,
}

#[derive(Debug)]
pub struct ProcessWait {
    pub hndl: ReceivedHandle,
}

// @alwin: error code?
// #[derive(Debug)]
// pub struct ProcessExit {
// pub res: usize
// }

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
    pub client_id: usize,
    pub reference: usize,
}

#[derive(Debug)]
pub struct WindowDeregister {
    pub hndl: ReceivedHandle,
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
    pub hndl: ReceivedHandle,
}

#[derive(Debug)]
pub struct PageMap {
    pub window_registration_hndl: ReceivedHandle,
    pub view_offset: usize,
    pub content_vaddr: usize,
}

#[derive(Debug)]
pub struct ConnDestroy {
    pub hndl: ReceivedHandle,
}

#[derive(Debug)]
pub struct LoadComplete {
    pub entry_point: usize,
    pub sp: usize,
}

#[derive(Debug)]
pub struct ServerHandleCapCreate {
    pub publish_hndl: ReceivedHandle,
    pub ident: usize,
}

#[derive(Debug)]
pub struct IRQRegister {
    pub publish_hndl: ReceivedHandle,
    pub irq_num: usize,
    pub edge_triggered: bool,
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct sDDFChannelRegisterBidirectional {
    pub channel_hndl_cap: WrappedHandleCap,
    pub virt_type: Option<VirtType>,
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct sDDFChannelRegisterRecvOnly {
    pub channel_hndl_cap: WrappedHandleCap,
}

#[derive(Debug)]
pub struct ChannelOpen {
    pub hndl_cap: UnwrappedHandleCap,
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct sDDFQueueRegister {
    pub hndl_cap: WrappedHandleCap,
    pub size: usize,
    pub queue_type: QueueType,
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct sDDFProvideDataRegion {
    pub hndl_cap: WrappedHandleCap,
    pub size: usize,
}

#[derive(Debug)]
pub struct ServerCreateChannel {
    pub publish_hndl: ReceivedHandle,
}

// General invocation enum
#[derive(Debug)]
#[allow(non_camel_case_types)]
pub enum SMOS_Invocation<'a> {
    WindowCreate(WindowCreate),
    WindowDestroy(WindowDestroy),
    ObjCreate(ObjCreate<'a>),
    ObjOpen(ObjOpen<'a>),
    ObjStat(ObjStat),
    ObjClose(ObjClose),
    ObjDestroy(ObjDestroy),
    View(View),
    Unview(Unview),
    ConnCreate(ConnCreate<'a>),
    ConnDestroy(ConnDestroy),
    ConnOpen(ConnOpen),
    ConnClose,
    ConnPublish(ConnPublish<'a>),
    ConnRegister(ConnRegister),
    ConnDeregister(ConnDeregister),
    ReplyCreate,
    ServerHandleCapCreate(ServerHandleCapCreate),
    ProcessSpawn(ProcessSpawn<'a>),
    ProcessWait(ProcessWait),
    ProcessExit,
    WindowRegister(WindowRegister),
    WindowDeregister(WindowDeregister),
    PageMap(PageMap),
    LoadComplete(LoadComplete),
    IRQRegister(IRQRegister),
    sDDFChannelRegisterBidirectional(sDDFChannelRegisterBidirectional),
    sDDFChannelRegisterRecvOnly(sDDFChannelRegisterRecvOnly),
    sDDFQueueRegister(sDDFQueueRegister),
    sDDFGetDataRegion,
    sDDFProvideDataRegion(sDDFProvideDataRegion),
    ServerCreateChannel(ServerCreateChannel),
    ChannelOpen(ChannelOpen),
}

impl<'a> SMOS_Invocation<'a> {
    pub fn new<T: ServerConnection>(
        ipc_buffer: &sel4::IpcBuffer,
        info: &sel4::MessageInfo,
        data_buffer: Option<&'a [u8]>,
        recv_slot: AbsoluteCPtr,
    ) -> (Result<SMOS_Invocation<'a>, InvocationError>, bool) {
        return SMOS_Invocation_Raw::get_from_ipc_buffer::<T>(
            info,
            ipc_buffer,
            data_buffer,
            recv_slot,
        );
    }

    pub fn contains_cap(&self) -> bool {
        match self {
            SMOS_Invocation::WindowCreate(_)
            | SMOS_Invocation::WindowDestroy(_)
            | SMOS_Invocation::ObjCreate(_)
            | SMOS_Invocation::ObjOpen(_)
            | SMOS_Invocation::ObjStat(_)
            | SMOS_Invocation::ObjClose(_)
            | SMOS_Invocation::ObjDestroy(_)
            | SMOS_Invocation::Unview(_)
            | SMOS_Invocation::ConnCreate(_)
            | SMOS_Invocation::ConnDestroy(_)
            | SMOS_Invocation::ConnClose
            | SMOS_Invocation::ConnPublish(_)
            | SMOS_Invocation::ConnRegister(_)
            | SMOS_Invocation::ConnDeregister(_)
            | SMOS_Invocation::ReplyCreate
            | SMOS_Invocation::ServerHandleCapCreate(_)
            | SMOS_Invocation::ProcessSpawn(_)
            | SMOS_Invocation::ProcessWait(_)
            | SMOS_Invocation::ProcessExit
            | SMOS_Invocation::WindowRegister(_)
            | SMOS_Invocation::WindowDeregister(_)
            | SMOS_Invocation::PageMap(_)
            | SMOS_Invocation::LoadComplete(_)
            | SMOS_Invocation::IRQRegister(_)
            | SMOS_Invocation::sDDFGetDataRegion
            | SMOS_Invocation::ServerCreateChannel(_)
            | SMOS_Invocation::ChannelOpen(_) => {
                return false;
            }
            SMOS_Invocation::sDDFChannelRegisterBidirectional(_)
            | SMOS_Invocation::sDDFChannelRegisterRecvOnly(_)
            | SMOS_Invocation::sDDFQueueRegister(_)
            | SMOS_Invocation::sDDFProvideDataRegion(_) => {
                return true;
            }
            SMOS_Invocation::View(x) => {
                if let ServerReceivedHandleOrHandleCap::WrappedHandleCap(_) = x.window {
                    return true;
                }
                return false;
            }
            SMOS_Invocation::ConnOpen(x) => {
                if x.shared_buf_obj.is_none() {
                    return false;
                } else if let ServerReceivedHandleOrHandleCap::WrappedHandleCap(_) =
                    x.shared_buf_obj.unwrap().0
                {
                    return true;
                }
                return false;
            }
        }
    }
}

#[allow(non_snake_case)]
mod SMOS_Invocation_Raw {
    use crate::syscalls::*;

    pub fn get_from_ipc_buffer<'a, T: ServerConnection>(
        info: &sel4::MessageInfo,
        ipcbuf: &sel4::IpcBuffer,
        data_buffer: Option<&'a [u8]>,
        recv_slot: AbsoluteCPtr,
    ) -> (Result<SMOS_Invocation<'a>, InvocationError>, bool) {
        /* We check if we recieved a capability in the recv slot. We return this to the caller.
        It is up to them to decide what to do with the cap and whether they reuse the same recv
        slot or allocate a new one */

        let mut consumed_recv_slot = false;
        /* Did we recieve a capability? */
        if info.extra_caps() > 0 {
            // @alwin: Double check the correctness of this
            if info.caps_unwrapped() & (BIT(info.extra_caps()) - 1) != BIT(info.extra_caps()) - 1 {
                /* This means there was a capability that was transferred as opposed to being unwrapped */
                consumed_recv_slot = true;
            }
        }

        if SMOSInvocation::try_from(info.label()).is_err() {
            return (Err(InvocationError::InvalidInvocation), consumed_recv_slot);
        }

        if !T::is_supported(info.label().try_into().unwrap()) {
            return (
                Err(InvocationError::UnsupportedInvocation {
                    label: info.label().try_into().unwrap(),
                }),
                consumed_recv_slot,
            );
        }

        let ret = get_with(
            info,
            |i| ipcbuf.msg_regs()[i as usize],
            |i| ipcbuf.caps_or_badges()[i as usize],
            data_buffer,
            recv_slot,
            &mut consumed_recv_slot,
        );

        return (ret, consumed_recv_slot);
    }

    // @alwin: This is all kind of very ugly and very manual, but if we want to keep the API minimal, I think this is the only way
    fn get_with<'a>(
        info: &sel4::MessageInfo,
        f_msg: impl Fn(core::ffi::c_ulong) -> sel4_sys::seL4_Word,
        f_cap: impl Fn(core::ffi::c_ulong) -> sel4_sys::seL4_Word,
        data_buffer: Option<&'a [u8]>,
        recv_slot: AbsoluteCPtr,
        consumed_recv_slot: &mut bool,
    ) -> Result<SMOS_Invocation<'a>, InvocationError> {
        let label: SMOSInvocation = info
            .label()
            .try_into()
            .or(Err(InvocationError::InvalidInvocation))?;

        /* Make sure we only recieve capabilities if we are expected to for a particular invocation*/
        if !label.can_contain_wrapped_cap() && *consumed_recv_slot {
            return Err(InvocationError::InvalidArguments);
        }

        match label {
            SMOSInvocation::WindowCreate => {
                Ok(SMOS_Invocation::WindowCreate(WindowCreate {
                    base_vaddr: f_msg(WindowCreateArgs::BaseVaddr as u64)
                        .try_into()
                        .unwrap(), // @alwin: if there is a type mismatch, it shouldn't panic
                    size: f_msg(WindowCreateArgs::Size as u64).try_into().unwrap(),
                    return_cap: f_msg(WindowCreateArgs::ReturnCap as u64) != 0, // @alwin: hmm?
                }))
            }
            SMOSInvocation::WindowDestroy => {
                let val = if info.extra_caps() == 1 && info.caps_unwrapped() == 1 {
                    Ok(ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(
                        f_cap(WindowDestroyArgs::Handle as u64) as usize,
                    ))
                } else if info.length() == 1 {
                    Ok(ServerReceivedHandleOrHandleCap::new_handle(f_msg(
                        WindowDestroyArgs::Handle as u64,
                    )
                        as usize))
                } else {
                    Err(InvocationError::InvalidArguments)
                }?;

                Ok(SMOS_Invocation::WindowDestroy(WindowDestroy { hndl: val }))
            }
            SMOSInvocation::ObjClose => {
                let val = if info.extra_caps() == 1 && info.caps_unwrapped() == 1 {
                    Ok(ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(
                        f_cap(0) as usize,
                    ))
                } else if info.length() == 1 {
                    Ok(ServerReceivedHandleOrHandleCap::new_handle(
                        f_msg(0) as usize
                    ))
                } else {
                    Err(InvocationError::InvalidArguments)
                }?;

                Ok(SMOS_Invocation::ObjClose(ObjClose { hndl: val }))
            }
            SMOSInvocation::ObjDestroy => {
                let val = if info.extra_caps() == 1 && info.caps_unwrapped() == 1 {
                    Ok(ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(
                        f_cap(0) as usize,
                    ))
                } else if info.length() == 1 {
                    Ok(ServerReceivedHandleOrHandleCap::new_handle(
                        f_msg(0) as usize
                    ))
                } else {
                    Err(InvocationError::InvalidArguments)
                }?;

                Ok(SMOS_Invocation::ObjDestroy(ObjDestroy { hndl: val }))
            }
            SMOSInvocation::sDDFChannelRegisterBidirectional => {
                if info.length() > 1 || info.extra_caps() != 1 || info.caps_unwrapped() != 0 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::sDDFChannelRegisterBidirectional(
                    sDDFChannelRegisterBidirectional {
                        channel_hndl_cap: WrappedHandleCap::new(recv_slot),
                        virt_type: if info.length() == 1 {
                            Some(
                                f_msg(0)
                                    .try_into()
                                    .or(Err(InvocationError::InvalidArguments))?,
                            )
                        } else {
                            None
                        },
                    },
                ))
            }
            SMOSInvocation::sDDFChannelRegisterReceiveOnly => {
                if info.extra_caps() != 1 || info.caps_unwrapped() != 0 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::sDDFChannelRegisterRecvOnly(
                    sDDFChannelRegisterRecvOnly {
                        channel_hndl_cap: WrappedHandleCap::new(recv_slot),
                    },
                ))
            }
            SMOSInvocation::sDDFQueueRegister => {
                if info.length() != 2 || info.extra_caps() != 1 || info.caps_unwrapped() != 0 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::sDDFQueueRegister(sDDFQueueRegister {
                    hndl_cap: WrappedHandleCap::new(recv_slot),
                    size: f_msg(0) as usize,
                    queue_type: f_msg(1)
                        .try_into()
                        .or(Err(InvocationError::InvalidArguments))?,
                }))
            }
            SMOSInvocation::sDDFProvideDataRegion => {
                if info.extra_caps() != 1 || info.caps_unwrapped() != 0 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::sDDFProvideDataRegion(
                    sDDFProvideDataRegion {
                        hndl_cap: WrappedHandleCap::new(recv_slot),
                        size: f_msg(0) as usize,
                    },
                ))
            }
            SMOSInvocation::sDDFGetDataRegion => Ok(SMOS_Invocation::sDDFGetDataRegion),
            SMOSInvocation::WindowRegister => {
                if info.extra_caps() != 1 || info.caps_unwrapped() != 1 || info.length() != 3 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::WindowRegister(WindowRegister {
                    publish_hndl: ReceivedHandle::new(f_msg(0) as usize),
                    window_hndl: UnwrappedHandleCap::new(f_cap(0) as usize),
                    client_id: f_msg(1) as usize,
                    reference: f_msg(2) as usize,
                }))
            }
            SMOSInvocation::ServerCreateChannel => {
                if info.length() != 1 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::ServerCreateChannel(ServerCreateChannel {
                    publish_hndl: ReceivedHandle::new(f_msg(0) as usize),
                }))
            }
            SMOSInvocation::ChannelOpen => {
                if info.extra_caps() != 1 && info.caps_unwrapped() != 1 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::ChannelOpen(ChannelOpen {
                    hndl_cap: UnwrappedHandleCap::new(f_cap(0) as usize),
                }))
            }
            SMOSInvocation::WindowDeregister => {
                if info.length() != 1 {
                    return Err(InvocationError::InvalidArguments)?;
                }

                Ok(SMOS_Invocation::WindowDeregister(WindowDeregister {
                    hndl: ReceivedHandle::new(f_msg(0) as usize),
                }))
            }
            SMOSInvocation::PageMap => {
                if info.length() != 3 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::PageMap(PageMap {
                    window_registration_hndl: ReceivedHandle::new(f_msg(0) as usize),
                    view_offset: f_msg(1) as usize,
                    content_vaddr: f_msg(2) as usize,
                }))
            }
            SMOSInvocation::ConnCreate => {
                if data_buffer.is_none() {
                    return Err(InvocationError::DataBufferNotSet);
                }

                Ok(SMOS_Invocation::ConnCreate(ConnCreate {
                    name: rust_str_from_buffer(data_buffer.unwrap())?.0,
                }))
            }
            SMOSInvocation::ConnPublish => {
                if data_buffer.is_none() {
                    return Err(InvocationError::DataBufferNotSet);
                }

                Ok(SMOS_Invocation::ConnPublish(ConnPublish {
                    ntfn_buffer: f_msg(0) as usize,
                    name: rust_str_from_buffer(data_buffer.unwrap())?.0,
                }))
            }
            SMOSInvocation::ObjCreate => {
                let name = if f_msg(ObjCreateArgs::HasName as u64) != 0 {
                    if data_buffer.is_none() {
                        return Err(InvocationError::DataBufferNotSet);
                    }

                    Some(rust_str_from_buffer(data_buffer.unwrap())?.0)
                } else {
                    None
                };

                Ok(SMOS_Invocation::ObjCreate(ObjCreate {
                    name: name,
                    size: f_msg(ObjCreateArgs::Size as u64) as usize,
                    rights: sel4::CapRights::from_inner(sel4_sys::seL4_CapRights {
                        0: Bitfield::new([f_msg(ObjCreateArgs::Rights as u64)]),
                    }),
                    attributes: ObjAttributes::from_inner(f_msg(ObjCreateArgs::Attributes as u64)),
                    return_cap: f_msg(ObjCreateArgs::ReturnCap as u64) != 0,
                }))
            }
            SMOSInvocation::ObjOpen => {
                if data_buffer.is_none() {
                    return Err(InvocationError::DataBufferNotSet);
                }

                let name = rust_str_from_buffer(data_buffer.unwrap())?.0;

                Ok(SMOS_Invocation::ObjOpen(ObjOpen {
                    name: name,
                    rights: sel4::CapRights::from_inner(sel4_sys::seL4_CapRights {
                        0: Bitfield::new([f_msg(0)]),
                    }),
                    return_cap: f_msg(1) != 0,
                }))
            }
            SMOSInvocation::ObjStat => {
                let hndl = if info.extra_caps() == 1 {
                    if info.caps_unwrapped() != (1 << 0) {
                        /* Obj stat should only be called with objects provided by the server
                        being called into */
                        return Err(InvocationError::InvalidArguments);
                    }

                    Ok(ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(
                        f_cap(0) as usize,
                    ))
                } else if info.length() == 1 {
                    Ok(ServerReceivedHandleOrHandleCap::new_handle(
                        f_msg(0) as usize
                    ))
                } else {
                    Err(InvocationError::InvalidArguments)
                }?;

                Ok(SMOS_Invocation::ObjStat(ObjStat { hndl: hndl }))
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
                        window = ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(f_cap(
                            cap_arg_counter,
                        )
                            as usize);
                    } else {
                        /* Capability was not unwrapped */
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
                        object = ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(f_cap(
                            cap_arg_counter,
                        )
                            as usize);
                    } else {
                        /* Capability was not unwrapped */
                        // @alwin: Double check that this is invalid
                        return Err(InvocationError::InvalidArguments);
                    }
                } else {
                    object = ServerReceivedHandleOrHandleCap::new_handle(object_buf as usize)
                }

                Ok(SMOS_Invocation::View(View {
                    window: window,
                    object: object,
                    window_offset: f_msg(ViewArgs::WinOffset as u64) as usize,
                    obj_offset: f_msg(ViewArgs::ObjOffset as u64) as usize,
                    size: f_msg(ViewArgs::Size as u64) as usize,
                    rights: sel4::CapRights::from_inner(sel4_sys::seL4_CapRights {
                        0: Bitfield::new([f_msg(ViewArgs::Rights as u64)]),
                    }),
                }))
            }
            SMOSInvocation::Unview => {
                if info.length() != 1 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::Unview(Unview {
                    hndl: ReceivedHandle::new(f_msg(0) as usize),
                }))
            }
            SMOSInvocation::ConnDestroy => {
                if info.length() != 1 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::ConnDestroy(ConnDestroy {
                    hndl: ReceivedHandle::new(f_msg(0) as usize),
                }))
            }
            SMOSInvocation::ConnDeregister => {
                if info.length() != 1 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::ConnDeregister(ConnDeregister {
                    hndl: ReceivedHandle::new(f_msg(0) as usize),
                }))
            }
            SMOSInvocation::LoadComplete => {
                if info.length() != 2 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::LoadComplete(LoadComplete {
                    entry_point: f_msg(0) as usize,
                    sp: f_msg(1) as usize,
                }))
            }
            SMOSInvocation::ConnOpen => {
                let object: Option<(ServerReceivedHandleOrHandleCap, usize)>;
                if info.length() == 0 {
                    object = None;
                } else if info.extra_caps() == 1 {
                    if info.caps_unwrapped() & 1 != 0 {
                        object = Some((
                            ServerReceivedHandleOrHandleCap::new_unwrapped_handle_cap(
                                f_cap(0) as usize
                            ),
                            f_msg(1) as usize,
                        ));
                    } else {
                        object = Some((
                            ServerReceivedHandleOrHandleCap::new_wrapped_handle_cap(recv_slot),
                            f_msg(1) as usize,
                        ))
                    }
                } else {
                    object = Some((
                        ServerReceivedHandleOrHandleCap::new_handle(f_msg(0) as usize),
                        f_msg(1) as usize,
                    ));
                }

                Ok(SMOS_Invocation::ConnOpen({
                    ConnOpen {
                        shared_buf_obj: object,
                    }
                }))
            }
            SMOSInvocation::ConnClose => {
                return Ok(SMOS_Invocation::ConnClose);
            }
            SMOSInvocation::ConnRegister => {
                if info.length() != 2 {
                    /* Idk, some kind of error? */
                    todo!()
                }

                Ok(SMOS_Invocation::ConnRegister(ConnRegister {
                    publish_hndl: ReceivedHandle::new(f_msg(0) as usize),
                    client_id: f_msg(1) as usize,
                }))
            }
            SMOSInvocation::IRQRegister => {
                if info.length() != 3 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::IRQRegister(IRQRegister {
                    publish_hndl: ReceivedHandle::new(f_msg(0) as usize),
                    irq_num: f_msg(1) as usize,
                    edge_triggered: f_msg(2) != 0,
                }))
            }
            SMOSInvocation::ReplyCreate => Ok(SMOS_Invocation::ReplyCreate),
            SMOSInvocation::ServerHandleCapCreate => {
                if info.length() != 2 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::ServerHandleCapCreate(
                    ServerHandleCapCreate {
                        publish_hndl: ReceivedHandle::new(f_msg(0) as usize),
                        ident: f_msg(1) as usize,
                    },
                ))
            }
            SMOSInvocation::ProcSpawn => {
                if data_buffer.is_none() {
                    return Err(InvocationError::DataBufferNotSet);
                }

                if info.length() != 2 {
                    return Err(InvocationError::InvalidArguments);
                }

                let data_buffer_ref = data_buffer.unwrap();

                let (exec_name, ref mut data_buffer_ref) = rust_str_from_buffer(data_buffer_ref)?;
                let (fs_name, ref mut data_buffer_ref) = rust_str_from_buffer(data_buffer_ref)?;

                let args = if f_msg(0) == 0 {
                    None
                } else {
                    let mut args_inner = Vec::new();
                    for _ in 0..f_msg(0) {
                        let (arg_tmp, buf_tmp) = rust_str_from_buffer(data_buffer_ref)?;
                        args_inner.push(arg_tmp);
                        *data_buffer_ref = buf_tmp;
                    }
                    Some(args_inner)
                };

                Ok(SMOS_Invocation::ProcessSpawn(ProcessSpawn {
                    exec_name: exec_name,
                    fs_name: fs_name,
                    prio: f_msg(1)
                        .try_into()
                        .expect("@alwin: This should not be an assert"),
                    args: args,
                }))
            }
            SMOSInvocation::ProcWait => {
                if info.length() != 1 {
                    return Err(InvocationError::InvalidArguments);
                }

                Ok(SMOS_Invocation::ProcessWait(ProcessWait {
                    hndl: ReceivedHandle::new(f_msg(0) as usize),
                }))
            }
            SMOSInvocation::ProcExit => Ok(SMOS_Invocation::ProcessExit),
            SMOSInvocation::TestSimple => {
                panic!("Okay got to test simple");
            }

            _ => {
                panic!("Not handled {:?}", SMOSInvocation::try_from(info.label()));
            }
        }
    }
}
