use smos_common::connection::RootServerConnection;
use smos_common::error::InvocationError;
use smos_common::local_handle::{
    HandleOrHandleCap, LocalHandle, ObjectHandle, ViewHandle, WindowHandle,
};
use smos_common::obj_attributes::ObjAttributes;
use smos_common::syscall::{ObjectServerInterface, RootServerInterface};
use smos_cspace::SMOSUserCSpace;

#[derive(Debug, Copy, Clone)]
pub struct DMARegion {
    pub vaddr: usize,
    pub paddr: usize,
    pub size: usize,
    pub win_hndl: HandleOrHandleCap<WindowHandle>,
    pub obj_hndl: HandleOrHandleCap<ObjectHandle>,
    pub view_hndl: LocalHandle<ViewHandle>,
}

impl DMARegion {
    pub fn new(
        rs_conn: &RootServerConnection,
        cspace: &mut SMOSUserCSpace,
        vaddr: usize,
        size: usize,
        alloc_cap: bool,
    ) -> Result<Self, InvocationError> {
        let win_hndl = rs_conn.window_create(vaddr, size, None)?;

        let cap_arg = if alloc_cap == true {
            let slot = cspace
                .alloc_slot()
                .or(Err(InvocationError::InsufficientResources))?;
            Some(cspace.to_absolute_cptr(slot))
        } else {
            None
        };

        let obj_hndl = rs_conn.obj_create(
            None,
            size,
            sel4::CapRights::all(),
            ObjAttributes::CONTIGUOUS,
            cap_arg,
        )?;
        let view_hndl = rs_conn.view(&win_hndl, &obj_hndl, 0, 0, size, sel4::CapRights::all())?;
        let stat = rs_conn.obj_stat(&obj_hndl)?;

        return Ok(Self {
            vaddr: vaddr,
            paddr: stat.paddr.expect("Created DMA region should have a paddr"),
            size: size,
            win_hndl: win_hndl,
            obj_hndl: obj_hndl,
            view_hndl: view_hndl,
        });
    }

    pub fn open(
        rs_conn: &RootServerConnection,
        obj_hndl: HandleOrHandleCap<ObjectHandle>,
        vaddr: usize,
        expected_size: usize,
    ) -> Result<Self, InvocationError> {
        let stat = rs_conn.obj_stat(&obj_hndl)?;

        // @alwin: necessary?
        if stat.size != expected_size || stat.paddr.is_none() {
            return Err(InvocationError::InvalidArguments);
        }

        let win_hndl = rs_conn.window_create(vaddr, stat.size, None)?;

        let view_hndl = rs_conn.view(
            &win_hndl,
            &obj_hndl,
            0,
            0,
            stat.size,
            sel4::CapRights::all(),
        )?;

        return Ok(Self {
            vaddr: vaddr,
            paddr: stat.paddr.unwrap(),
            size: stat.size,
            win_hndl: win_hndl,
            obj_hndl: obj_hndl,
            view_hndl: view_hndl,
        });
    }
}
