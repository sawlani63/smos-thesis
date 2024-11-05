use num_enum::{IntoPrimitive, TryFromPrimitive};

// @alwin: This file should be auto-generated
#[derive(TryFromPrimitive, IntoPrimitive, Debug, PartialEq)]
#[repr(u64)]
#[allow(non_camel_case_types)]
pub enum SMOSInvocation {
    WindowCreate = 0,
    WindowDestroy,
    WindowRegister,
    WindowDeregister,
    ObjCreate,
    ObjDestroy,
    ObjOpen,
    ObjClose,
    View,
    Unview,
    ObjStat,
    ConnCreate,
    ConnDestroy,
    ConnOpen,
    ConnClose,
    ConnPublish,
    ConnUnpublish,
    ConnRegister,
    ConnDeregister,
    TestSimple,
    Authorise,
    ProcSpawn,
    ProcWait,
    ProcExit,
    ProcDestroy,
    ProcCreateComplete, // @alwin: needed? You can probably just jump to the application from the loader
    ReplyCreate, // @alwin: This is used for making reply objects, but I think this should be a general function kinda like untyped retype
    ReplyDestroy, // @alwin: as previous
    ServerHandleCapCreate,
    ServerHandleCapDestroy,
    DirOpen,
    DirClose,
    DirRead,
    // @alwin: Do we want compound operations like create/open + view in one invocation?
    PageMap,
    PageUnmap,
    LoadComplete,
    IRQRegister,
    sDDFChannelRegisterBidirectional,
    sDDFChannelRegisterReceiveOnly,
    sDDFQueueRegister,
    sDDFGetDataRegion,
    sDDFProvideDataRegion,
    ServerCreateChannel,
    ChannelOpen,
}

impl SMOSInvocation {
    pub fn can_contain_wrapped_cap(&self) -> bool {
        match self {
            SMOSInvocation::sDDFChannelRegisterBidirectional
            | SMOSInvocation::sDDFChannelRegisterReceiveOnly
            | SMOSInvocation::sDDFQueueRegister
            | SMOSInvocation::sDDFProvideDataRegion
            | SMOSInvocation::View
            | SMOSInvocation::ConnOpen => return true,
            _ => return false,
        }
    }
}
