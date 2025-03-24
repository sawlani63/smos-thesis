use core::marker::PhantomData;
use smos_common::connection::RootServerConnection;
use smos_common::error::InvocationError;
use smos_common::local_handle::{
    HandleOrHandleCap, LocalHandle, ObjectHandle, ViewHandle, WindowHandle,
};
use smos_common::obj_attributes::ObjAttributes;
use smos_common::syscall::{ObjectServerInterface, RootServerInterface};
use smos_cspace::SMOSUserCSpace;

#[allow(non_camel_case_types)]
pub trait sDDFQueueType {}

// @alwin: Should these be moved somewhere else?
#[derive(Debug, Copy, Clone)]
pub struct ActiveQueue {}
#[derive(Debug, Copy, Clone)]
pub struct FreeQueue {}
#[derive(Debug, Copy, Clone)]
pub struct SerialQueue {}

impl sDDFQueueType for ActiveQueue {}
impl sDDFQueueType for FreeQueue {}
impl sDDFQueueType for SerialQueue {}

pub struct QueuePair {
    pub active: Queue<ActiveQueue>,
    pub free: Queue<FreeQueue>,
}

impl QueuePair {
    pub fn new(
        rs_conn: &RootServerConnection,
        cspace: &mut SMOSUserCSpace,
        active_vaddr: usize,
        free_vaddr: usize,
        size: usize,
    ) -> Result<Self, InvocationError> {
        let active = Queue::new(rs_conn, cspace, active_vaddr, size)?;
        let free = Queue::new(rs_conn, cspace, free_vaddr, size)?;

        return Ok(QueuePair {
            active: active,
            free: free,
        });
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Queue<T: sDDFQueueType> {
    pub vaddr: usize,
    pub size: usize,
    pub win_hndl: HandleOrHandleCap<WindowHandle>,
    pub obj_hndl_cap: Option<HandleOrHandleCap<ObjectHandle>>, // @alwin: Is there any point holding onto the cap in the recipient?
    pub view_hndl: LocalHandle<ViewHandle>,
    pub marker: PhantomData<T>,
}

impl<T: sDDFQueueType> Queue<T> {
    // Called by the one who creates the queue
    pub fn new(
        rs_conn: &RootServerConnection,
        cspace: &mut SMOSUserCSpace,
        vaddr: usize,
        size: usize,
    ) -> Result<Self, InvocationError> {
        let win_hndl = rs_conn.window_create(vaddr, size, None)?;

        let obj_slot = cspace
            .alloc_slot()
            .or(Err(InvocationError::InsufficientResources))?;
        let obj_hndl_cap = rs_conn.obj_create(
            None,
            size,
            sel4::CapRights::all(),
            ObjAttributes::DEFAULT,
            Some(cspace.to_absolute_cptr(obj_slot)),
        )?;

        let view_hndl =
            rs_conn.view(&win_hndl, &obj_hndl_cap, 0, 0, size, sel4::CapRights::all())?;

        return Ok(Self {
            vaddr: vaddr,
            size: size,
            win_hndl: win_hndl,
            obj_hndl_cap: Some(obj_hndl_cap),
            view_hndl: view_hndl,
            marker: PhantomData,
        });
    }

    /// Called by the one who recieves the queue
    pub fn open(
        rs_conn: &RootServerConnection,
        vaddr: usize,
        size: usize,
        obj_hndl_cap: HandleOrHandleCap<ObjectHandle>,
    ) -> Result<Self, InvocationError> {
        let win_hndl = rs_conn.window_create(vaddr, size, None)?;

        let view_hndl =
            rs_conn.view(&win_hndl, &obj_hndl_cap, 0, 0, size, sel4::CapRights::all())?;

        return Ok(Self {
            vaddr: vaddr,
            size: size,
            win_hndl: win_hndl,
            obj_hndl_cap: None,
            view_hndl: view_hndl,
            marker: PhantomData,
        });
    }
}
