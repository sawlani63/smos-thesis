use crate::connection::Server;
use crate::cspace::{CSpace, CSpaceTrait};
use crate::handle::RootServerResource;
use crate::object::{AnonymousMemoryObject, OBJ_LVL_MAX};
use crate::proc::UserProcess;
use crate::window::Window;
use crate::RSReplyWrapper;
use crate::PAGE_SIZE_4K;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use smos_common::args::ViewArgs;
use smos_common::error::InvocationError;
use smos_common::local_handle::LocalHandle;
use smos_common::util::BIT;
use smos_server::handle::{
    generic_get_handle, generic_invalid_handle_error, HandleAllocater, ServerHandle,
};
use smos_server::handle_capability::HandleCapabilityTable;
use smos_server::reply::SMOSReply;

#[derive(Clone, Debug)]
pub struct ViewCap {
    pub cap: sel4::cap::UnspecifiedFrame,
}

#[derive(Clone, Debug)]
pub struct ViewCapTable {
    pub table: Vec<Option<ViewCapTableEntry>>,
}

#[derive(Clone, Debug)]
pub enum ViewCapTableEntry {
    Cap(ViewCap),
    CapTable(ViewCapTable),
}

#[derive(Clone, Debug)]
pub struct View {
    caps: Vec<Option<ViewCapTableEntry>>,
    pub bound_window: Rc<RefCell<Window>>,
    pub bound_object: Option<Rc<RefCell<AnonymousMemoryObject>>>,
    pub managing_server_info: Option<(Rc<RefCell<Server>>, usize, usize)>, // @alwin: Does this need the window registration handle?
    pub rights: sel4::CapRights,
    pub win_offset: usize,
    pub obj_offset: usize,
    pub pending_fault: Option<(RSReplyWrapper, sel4::VmFault, sel4::cap::VSpace)>,
}

impl View {
    pub fn new(
        bw: Rc<RefCell<Window>>,
        bo: Option<Rc<RefCell<AnonymousMemoryObject>>>,
        msi: Option<(Rc<RefCell<Server>>, usize, usize)>,
        rights: sel4::CapRights,
        win_off: usize,
        obj_off: usize,
    ) -> View {
        View {
            caps: vec![None; OBJ_LVL_MAX],
            bound_window: bw,
            bound_object: bo,
            managing_server_info: msi,
            rights: rights,
            win_offset: win_off,
            obj_offset: obj_off,
            pending_fault: None,
        }
    }

    pub fn lookup_cap<'a>(&'a self, offset: usize) -> Option<&'a ViewCap> {
        let mut shift = 39;
        let mut curr_table_lvl = 0;
        let mut curr_table = &self.caps;
        while curr_table_lvl < 4 {
            let idx = (offset >> shift) & (BIT(9) - 1);

            curr_table = match &curr_table[idx] {
                None => return None,
                Some(x) => match x {
                    ViewCapTableEntry::Cap(ref y) => return Some(&y),
                    ViewCapTableEntry::CapTable(y) => &y.table,
                },
            };

            curr_table_lvl += 1;
            shift -= 9;
        }

        return None;
    }

    pub fn insert_cap_at(
        &mut self,
        offset: usize,
        cap: sel4::cap::UnspecifiedFrame,
    ) -> Result<(), sel4::Error> {
        let mut shift = 39;
        let mut curr_table_lvl = 0;
        let mut curr_table = &mut self.caps;
        while curr_table_lvl < 3 {
            let idx = (offset >> shift) & (BIT(9) - 1);

            if curr_table[idx].is_none() {
                curr_table[idx] = Some(ViewCapTableEntry::CapTable(ViewCapTable {
                    table: vec![None; OBJ_LVL_MAX],
                }));
            }

            curr_table = match &mut curr_table[idx] {
                None => return Err(sel4::Error::InvalidArgument), // @alwin: What to actually return here?
                Some(ref mut x) => match x {
                    ViewCapTableEntry::Cap(_) => return Err(sel4::Error::DeleteFirst),
                    ViewCapTableEntry::CapTable(ref mut y) => &mut y.table,
                },
            };

            curr_table_lvl += 1;
            shift -= 9;
        }

        let idx = (offset >> shift) & (BIT(9) - 1);
        if curr_table[idx].is_none() {
            curr_table[idx] = Some(ViewCapTableEntry::Cap(ViewCap { cap: cap }));
            return Ok(());
        }

        match &curr_table[idx] {
            None => panic!("This should have already been handled above"),
            Some(x) => match x {
                ViewCapTableEntry::Cap(_) => return Err(sel4::Error::DeleteFirst),
                ViewCapTableEntry::CapTable(_) => {
                    panic!("Internal error: FrameTable on bottom level should not occur")
                }
            },
        }
    }

    fn cleanup_cap_table_inner(
        vec: &Vec<Option<ViewCapTableEntry>>,
        cspace: &mut CSpace,
        delete: bool,
    ) {
        for node in vec {
            match node {
                None => continue,
                Some(x) => {
                    match x {
                        ViewCapTableEntry::CapTable(ref y) => {
                            Self::cleanup_cap_table_inner(&y.table, cspace, delete)
                        }
                        ViewCapTableEntry::Cap(ref y) => {
                            if delete {
                                /* @alwin: deleting the cap should result in an unmap, but double check*/
                                cspace
                                    .delete_cap(y.cap)
                                    .expect("Failed to delete capability");
                            }
                            cspace.free_cap(y.cap);
                        }
                    }
                }
            }
        }
    }

    /* Cleans up cap table. Should use delete == false when the object frame table was cleaned
    before with revoke == true, as the caps would have already been deleted by this. */
    pub fn cleanup_cap_table(&mut self, cspace: &mut CSpace, delete: bool) {
        Self::cleanup_cap_table_inner(&self.caps, cspace, delete)
    }
}

pub fn handle_view(
    p: &mut UserProcess,
    handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
    args: &smos_server::syscalls::View,
) -> Result<SMOSReply, InvocationError> {
    let window_ref =
        generic_get_handle(p, handle_cap_table, args.window, ViewArgs::Window as usize)?;
    let window: Rc<RefCell<Window>> = match window_ref.as_ref().unwrap().inner() {
        RootServerResource::Window(win) => Ok(win.clone()),
        _ => Err(generic_invalid_handle_error(
            args.window,
            ViewArgs::Window as usize,
        )),
    }?;

    let object_ref =
        generic_get_handle(p, handle_cap_table, args.object, ViewArgs::Object as usize)?;
    let object: Rc<RefCell<AnonymousMemoryObject>> = match object_ref.as_ref().unwrap().inner() {
        RootServerResource::Object(obj) => Ok(obj.clone()),
        _ => Err(generic_invalid_handle_error(
            args.object,
            ViewArgs::Object as usize,
        )),
    }?;

    /* Ensure the size is non-zero */
    if args.size == 0 {
        return Err(InvocationError::InvalidArguments);
    }

    /* Ensure offsets into the window and object are page aligned */
    if args.window_offset & PAGE_SIZE_4K != 0 {
        return Err(InvocationError::AlignmentError {
            which_arg: ViewArgs::WinOffset as usize,
        });
    }

    if args.obj_offset & PAGE_SIZE_4K != 0 {
        return Err(InvocationError::AlignmentError {
            which_arg: ViewArgs::ObjOffset as usize,
        });
    }

    /* Ensure that the object is big enough */
    if args.obj_offset + args.size > object.borrow_mut().size {
        return Err(InvocationError::InvalidArguments);
    }

    /* Ensure everything stays inside the window */
    if args.window_offset + args.size > window.borrow_mut().size {
        return Err(InvocationError::InvalidArguments);
    }

    /* Ensure that the window isn't already being used for another view */
    if window.borrow_mut().bound_view.is_some() {
        // @alwin: Why do I need to borrow_mut() here?
        return Err(InvocationError::InvalidArguments);
    }

    let view = Rc::new(RefCell::new(View {
        caps: vec![None; OBJ_LVL_MAX],
        win_offset: args.window_offset,
        obj_offset: args.obj_offset,
        bound_window: window.clone(),
        bound_object: Some(object.clone()),
        managing_server_info: None,
        rights: args.rights.clone(), // @alwin: The rights of the view should be &'d with the rights of the object
        pending_fault: None,
    }));

    window.borrow_mut().bound_view = Some(view.clone());
    object.borrow_mut().associated_views.push(view.clone());

    // @alwin: Deal with permissions and do appropriate cleanup
    let (idx, handle_ref) = p.allocate_handle()?;

    *handle_ref = Some(ServerHandle::new(RootServerResource::View(view.clone())));
    p.views.push(view);

    return Ok(SMOSReply::View {
        hndl: LocalHandle::new(idx),
    });
}

pub fn handle_unview_internal(cspace: &mut CSpace, view: Rc<RefCell<View>>) {
    view.borrow_mut().cleanup_cap_table(cspace, true);
    view.borrow_mut().bound_window.borrow_mut().bound_view = None;
    assert!(view.borrow_mut().bound_object.is_some());

    /* Find the view inside the object list and delete it*/
    let pos = view
        .borrow_mut()
        .bound_object
        .as_ref()
        .unwrap()
        .borrow_mut()
        .associated_views
        .iter()
        .position(|x| Rc::ptr_eq(x, &view))
        .unwrap();

    view.borrow_mut()
        .bound_object
        .as_ref()
        .unwrap()
        .borrow_mut()
        .associated_views
        .swap_remove(pos);
}

pub fn handle_unview(
    cspace: &mut CSpace,
    p: &mut UserProcess,
    args: &smos_server::syscalls::Unview,
) -> Result<SMOSReply, InvocationError> {
    let view_ref = p
        .get_handle_mut(args.hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;
    let view = match view_ref.as_ref().unwrap().inner() {
        RootServerResource::View(view) => Ok(view.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    handle_unview_internal(cspace, view);

    p.cleanup_handle(args.hndl.idx)
        .expect("Failed to clean up handle");

    return Ok(SMOSReply::Unview);
}
