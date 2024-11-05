use crate::cspace::CSpace;
use crate::handle::RootServerResource;
use crate::proc::UserProcess;
use crate::view::View;
use crate::PAGE_SIZE_4K;
use alloc::rc::Rc;
use core::cell::RefCell;
use smos_common::args::{WindowCreateArgs, WindowDestroyArgs};
use smos_common::error::InvocationError;
use smos_common::local_handle::{HandleOrHandleCap, LocalHandle, WindowHandle};
use smos_server::handle::{
    generic_allocate_handle, generic_cleanup_handle, generic_get_handle, HandleAllocater,
    ServerHandle,
};
use smos_server::handle_arg::ServerReceivedHandleOrHandleCap;
use smos_server::handle_capability::HandleCapabilityTable;
use smos_server::ntfn_buffer::{
    enqueue_ntfn_buffer_msg, NotificationType, WindowDestroyNotification,
};
use smos_server::reply::SMOSReply;
use smos_server::syscalls::{WindowCreate, WindowDestroy};
use smos_server::syscalls::{WindowDeregister, WindowRegister};

#[derive(Clone, Debug)]
pub struct Window {
    pub start: usize,
    pub size: usize,
    pub bound_view: Option<Rc<RefCell<View>>>,
}

pub fn handle_window_create(
    p: &mut UserProcess,
    handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
    args: &WindowCreate,
) -> Result<SMOSReply, InvocationError> {
    if args.base_vaddr as usize % PAGE_SIZE_4K != 0 {
        warn_rs!("Window base address should be aligned");
        return Err(InvocationError::AlignmentError {
            which_arg: WindowCreateArgs::BaseVaddr as usize,
        });
    }

    /* Ensure that the window does not overlap with any other windows */
    if let Some(overlapping_window) =
        p.overlapping_window(args.base_vaddr.try_into().unwrap(), args.size)
    {
        let start = overlapping_window.borrow_mut().start;
        let size = overlapping_window.borrow_mut().size;
        warn_rs!(
            "Window ({:#x} -> {:#x}) overlaps with an existing window at: {:#x} -> {:#x}",
            args.base_vaddr as usize,
            args.base_vaddr as usize + args.size,
            start,
            start + size
        );
        return Err(InvocationError::InvalidArguments);
    }

    let (idx, handle_ref, cptr) = generic_allocate_handle(p, handle_cap_table, args.return_cap)?;

    // @alwin: Check that window size is in user addressable vspace?

    // @alwin: Eventually, we should have an allocator per-process and allocate this box from
    // the caller's allocator to have better memory usage bookkeeping
    let window = Rc::new(RefCell::new(Window {
        start: args.base_vaddr.try_into().unwrap(),
        size: args.size,
        bound_view: None,
    }));

    *handle_ref = Some(ServerHandle::new(RootServerResource::Window(
        window.clone(),
    )));
    if args.return_cap {
        p.created_handle_caps.push(idx);
    }
    p.add_window_unchecked(window);

    let ret_value = if args.return_cap {
        HandleOrHandleCap::<WindowHandle>::new_handle_cap(cptr.unwrap())
    } else {
        HandleOrHandleCap::<WindowHandle>::new_handle(idx)
    };

    return Ok(SMOSReply::WindowCreate { hndl: ret_value });
}

pub fn handle_window_register(
    p: &mut UserProcess,
    handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
    args: &WindowRegister,
) -> Result<SMOSReply, InvocationError> {
    let publish_hndl_ref = p
        .get_handle(args.publish_hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;

    let server = match publish_hndl_ref.as_ref().unwrap().inner() {
        RootServerResource::Server(x) => Ok(x.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    /* Check that the passed in handle/cap is within bounds */
    let window_handle_ref = handle_cap_table
        .get_handle_cap_mut(args.window_hndl.idx)
        .or(Err(InvocationError::InvalidHandleCapability {
            which_arg: 01,
        }))?;

    /* Check that the object it refers to is a window */
    let window = match window_handle_ref.as_ref().unwrap().inner() {
        RootServerResource::Window(win) => Ok(win.clone()),
        _ => Err(InvocationError::InvalidHandleCapability { which_arg: 0 }),
    }?;

    let (idx, handle_ref) = p.allocate_handle()?;

    let view = Rc::new(RefCell::new(View::new(
        window.clone(),
        None,
        Some((server.clone(), args.client_id, args.reference)),
        sel4::CapRights::all(), // @alwin: what are these meant to be? The permissions of the view don't really make
        // sense with an externally managed object, which the server might choose to map in
        // with any set of rights, with different ones for each page
        0, // @alwin: These aren't really necessary for externally managed
        0, // '''
    )));

    window.borrow_mut().bound_view = Some(view.clone());

    *handle_ref = Some(ServerHandle::new(RootServerResource::WindowRegistration(
        view.clone(),
    )));

    return Ok(SMOSReply::WindowRegister {
        hndl: LocalHandle::new(idx),
    });
}

pub fn handle_window_deregister(
    cspace: &mut CSpace,
    p: &mut UserProcess,
    args: &WindowDeregister,
) -> Result<SMOSReply, InvocationError> {
    let reg_hndl_ref = p
        .get_handle_mut(args.hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;
    let view = match reg_hndl_ref.as_ref().unwrap().inner() {
        RootServerResource::WindowRegistration(view) => Ok(view.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    // @alwin: This fn is kinda the same as handle_unview, what to do abt this?
    view.borrow_mut().cleanup_cap_table(cspace, true);

    // @alwin: There should probs be some checks about pending fault or something here?
    view.borrow_mut().bound_window.borrow_mut().bound_view = None;
    assert!(view.borrow_mut().bound_object.is_none());

    p.cleanup_handle(args.hndl.idx)
        .expect("Failed to clean up handle");

    return Ok(SMOSReply::WindowDeregister);
}

pub fn handle_window_destroy_internal(
    cspace: &mut CSpace,
    window: Rc<RefCell<Window>>,
    destroy_view: bool,
) {
    /* if there is a view inside the window, destroy that too? */
    if let Some(bv) = &window.borrow_mut().bound_view {
        if let Some((server, client_id, reference)) = &bv.borrow_mut().managing_server_info {
            let msg = NotificationType::WindowDestroyNotification(WindowDestroyNotification {
                client_id: *client_id,
                reference: *reference,
            });

            unsafe {
                enqueue_ntfn_buffer_msg(server.borrow().ntfn_buffer_addr, msg)
                    .expect("@alwin: What to do when buffer is full");
            };
            server.borrow().ntfn_dispatch.rs_badged_ntfn().signal();
        }

        if destroy_view {
            // @alwin: Should we destroy the view here or say that you're not allowed to delete a window
            // while a view is still inside of it? What to do we do with the view handle?
            let pos = bv
                .borrow_mut()
                .bound_object
                .as_ref()
                .unwrap()
                .borrow_mut()
                .associated_views
                .iter()
                .position(|x| Rc::ptr_eq(x, &bv))
                .unwrap();
            bv.borrow_mut()
                .bound_object
                .as_ref()
                .unwrap()
                .borrow_mut()
                .associated_views
                .swap_remove(pos);
            bv.borrow_mut().cleanup_cap_table(cspace, true);
            warn_rs!("deleting view inside window");
        }
    }
}

pub fn handle_window_destroy(
    cspace: &mut CSpace,
    p: &mut UserProcess,
    handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
    args: &WindowDestroy,
) -> Result<SMOSReply, InvocationError> {
    /* Check that the passed in handle/cap is within bounds */
    let handle_ref = generic_get_handle(
        p,
        handle_cap_table,
        args.hndl,
        WindowDestroyArgs::Handle as usize,
    )?;

    /* Check that the object it refers to is a window */
    let window = match handle_ref.as_ref().unwrap().inner() {
        RootServerResource::Window(win) => Ok(win.clone()),
        _ => match args.hndl {
            ServerReceivedHandleOrHandleCap::Handle(_) => Err(InvocationError::InvalidHandle {
                which_arg: WindowDestroyArgs::Handle as usize,
            }),
            ServerReceivedHandleOrHandleCap::UnwrappedHandleCap(_) => {
                Err(InvocationError::InvalidHandleCapability {
                    which_arg: WindowDestroyArgs::Handle as usize,
                })
            }
            _ => panic!("We should not get an unwrapped handle cap here"),
        },
    }?;

    handle_window_destroy_internal(cspace, window.clone(), true);
    p.remove_window(window);

    generic_cleanup_handle(
        p,
        handle_cap_table,
        args.hndl,
        WindowDestroyArgs::Handle as usize,
    )?;

    return Ok(SMOSReply::WindowDestroy {});
}
