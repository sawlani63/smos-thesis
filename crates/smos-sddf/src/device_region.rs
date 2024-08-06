use smos_common::connection::RootServerConnection;
use smos_common::error::InvocationError;
use smos_common::local_handle::{
    HandleOrHandleCap, LocalHandle, ObjectHandle, ViewHandle, WindowHandle,
};
use smos_common::obj_attributes::ObjAttributes;
use smos_common::syscall::{ObjectServerInterface, RootServerInterface};
use smos_cspace::SMOSUserCSpace;
extern crate alloc;
use alloc::string::ToString;

pub struct DeviceRegion {
    pub vaddr: usize,
    pub paddr: usize,
    pub size: usize,
    pub win_hndl: HandleOrHandleCap<WindowHandle>,
    pub obj_hndl: HandleOrHandleCap<ObjectHandle>,
    pub view_hndl: LocalHandle<ViewHandle>,
}

impl DeviceRegion {
    pub fn new(
        rs_conn: &RootServerConnection,
        cspace: &mut SMOSUserCSpace,
        vaddr: usize,
        size: usize,
        paddr: usize,
    ) -> Result<Self, InvocationError> {
        let win_hndl = rs_conn.window_create(vaddr, size, None)?;

        let obj_hndl = rs_conn.obj_create(
            Some(&paddr.to_string()),
            size,
            sel4::CapRights::all(),
            ObjAttributes::DEVICE,
            None,
        )?;

        let view_hndl = rs_conn.view(&win_hndl, &obj_hndl, 0, 0, size, sel4::CapRights::all())?;

        return Ok(Self {
            vaddr: vaddr,
            paddr: paddr,
            size: size,
            win_hndl: win_hndl,
            obj_hndl: obj_hndl,
            view_hndl: view_hndl,
        });
    }
}
