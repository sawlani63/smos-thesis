use crate::client_connection::*;
use crate::connection::*;
use crate::invocations::SMOSInvocation;

/* @alwin: Figure out how to autogenerate these */
const ROOT_SERVER_INVOCATIONS: [SMOSInvocation; 17] = [
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
];
const OBJECT_SERVER_INVOCATIONS: [SMOSInvocation; 9] = [
    SMOSInvocation::ConnOpen,
    SMOSInvocation::ConnClose,
    SMOSInvocation::ObjCreate,
    SMOSInvocation::View,
    SMOSInvocation::Unview,
    SMOSInvocation::ObjOpen,
    SMOSInvocation::ObjClose,
    SMOSInvocation::ObjDestroy,
    SMOSInvocation::ObjStat,
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
        return OBJECT_SERVER_INVOCATIONS.contains(&inv);
    }
}
