use crate::client_connection::*;
use crate::connection::*;
use crate::invocations::SMOSInvocation;

/* @alwin: Figure out how to autogenerate these */
const ROOT_SERVER_INVOCATIONS: [SMOSInvocation; 20] = [
    SMOSInvocation::ConnCreate,
    SMOSInvocation::ConnDestroy,
    SMOSInvocation::ConnPublish,
    SMOSInvocation::ConnRegister,
    SMOSInvocation::ConnDeregister,
    SMOSInvocation::TestSimple,
    SMOSInvocation::WindowCreate,
    SMOSInvocation::WindowDestroy,
    SMOSInvocation::WindowRegister,
    SMOSInvocation::WindowDeregister,
    SMOSInvocation::ReplyCreate,
    SMOSInvocation::ServerHandleCapCreate,
    SMOSInvocation::ProcSpawn,
    SMOSInvocation::ProcWait,
    SMOSInvocation::ProcExit,
    SMOSInvocation::PageMap,
    SMOSInvocation::LoadComplete,
    SMOSInvocation::IRQRegister,
    SMOSInvocation::ServerCreateChannel,
    SMOSInvocation::ChannelOpen,
];
const OBJECT_SERVER_INVOCATIONS: [SMOSInvocation; 7] = [
    SMOSInvocation::ObjCreate,
    SMOSInvocation::View,
    SMOSInvocation::Unview,
    SMOSInvocation::ObjOpen,
    SMOSInvocation::ObjClose,
    SMOSInvocation::ObjDestroy,
    SMOSInvocation::ObjStat,
];
const NON_ROOT_SERVER_INVOCATIONS: [SMOSInvocation; 2] =
    [SMOSInvocation::ConnOpen, SMOSInvocation::ConnClose];

//@alwin: This should not be here
const sDDF_INVOCATIONS: [SMOSInvocation; 5] = [
    SMOSInvocation::sDDFChannelRegisterBidirectional,
    SMOSInvocation::sDDFChannelRegisterRecieveOnly,
    SMOSInvocation::sDDFQueueRegister,
    SMOSInvocation::sDDFGetDataRegion,
    SMOSInvocation::sDDFProvideDataRegion,
];

pub trait ServerConnection: ClientConnection {
    fn is_supported(inv: SMOSInvocation) -> bool;
}

impl ServerConnection for RootServerConnection {
    fn is_supported(inv: SMOSInvocation) -> bool {
        return ROOT_SERVER_INVOCATIONS.contains(&inv) || OBJECT_SERVER_INVOCATIONS.contains(&inv);
    }
}

impl ServerConnection for ObjectServerConnection {
    fn is_supported(inv: SMOSInvocation) -> bool {
        return NON_ROOT_SERVER_INVOCATIONS.contains(&inv)
            || OBJECT_SERVER_INVOCATIONS.contains(&inv);
    }
}

impl ServerConnection for sDDFConnection {
    fn is_supported(inv: SMOSInvocation) -> bool {
        return NON_ROOT_SERVER_INVOCATIONS.contains(&inv) || sDDF_INVOCATIONS.contains(&inv);
    }
}
