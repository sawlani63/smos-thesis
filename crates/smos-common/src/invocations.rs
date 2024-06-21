use num_enum::{TryFromPrimitive, IntoPrimitive};

// @alwin: This file should be auto-generated
#[derive(TryFromPrimitive, IntoPrimitive, Debug, PartialEq)]
#[repr(u64)]
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
	ProcDestroy,
	ProcCreateComplete, // @alwin: needed? You can probably just jump to the application from the loader
	ReplyCreate, // @alwin: This is used for making reply objects, but I think this should be a general function kinda like untyped retype
	ReplyDestroy, // @alwin: as previous
	DirOpen,
	DirClose,
	DirRead,
	// @alwin: Do we want compound operations like create/open + view in one invocation?
	PageMap,
	PageUnmap,
	LoadComplete
}

