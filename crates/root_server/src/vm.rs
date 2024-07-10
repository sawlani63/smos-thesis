use crate::cspace::{CSpace, CSpaceTrait};
use crate::frame_table::FrameTable;
use crate::handle::RootServerResource;
use crate::mapping::map_frame;
use crate::page::PAGE_SIZE_4K;
use crate::proc::UserProcess;
use crate::ut::UTTable;
use crate::util::dealloc_retyped;
use crate::view::View;
use crate::RSReplyWrapper;
use alloc::rc::Rc;
use core::cell::RefCell;
use smos_common::error::InvocationError;
use smos_common::util::ROUND_DOWN;
use smos_server::handle::HandleAllocater;
use smos_server::ntfn_buffer::{
    enqueue_ntfn_buffer_msg, NotificationType, NtfnBufferData, VMFaultNotification,
};
use smos_server::reply::{FaultReply, SMOSReply};
use smos_server::syscalls::PageMap;

fn forward_vm_fault(
    p: &mut UserProcess,
    view: Rc<RefCell<View>>,
    offset: usize,
    reply: RSReplyWrapper,
    info: sel4::VmFault,
) {
    let mut borrowed_view = view.borrow_mut();
    let server_info = borrowed_view.managing_server_info.as_ref().unwrap();
    let server = server_info.0.clone();

    let msg = NotificationType::VMFaultNotification(VMFaultNotification {
        client_id: server_info.1,
        reference: server_info.2,
        fault_offset: offset,
    });

    // Safety: The notification buffer address was set when the server was registered and
    // is only unset if the server stops being a server, at which point all the objects
    // associated with it will no longer be backed by it. @alwin: Make sure to actually implement this
    unsafe { enqueue_ntfn_buffer_msg(server.borrow().ntfn_buffer_addr, msg) };

    server.borrow().badged_ntfn.signal();
    borrowed_view.pending_fault = Some((reply, info, p.vspace.0))
}

pub fn handle_page_map(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
    frame_table: &mut FrameTable,
    p: &mut UserProcess,
    args: &PageMap,
) -> Result<SMOSReply, InvocationError> {
    if args.view_offset % PAGE_SIZE_4K != 0 {
        return Err(InvocationError::AlignmentError { which_arg: 1 });
    }

    if args.content_vaddr % PAGE_SIZE_4K != 0 {
        return Err(InvocationError::AlignmentError { which_arg: 1 });
    }

    let window_reg_ref = p
        .get_handle(args.window_registration_hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;

    let dst_view = match window_reg_ref.as_ref().unwrap().inner() {
        RootServerResource::WindowRegistration(view) => Ok(view.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    /* Check if there is already a mapping in the view at the offset */
    if dst_view.borrow().lookup_cap(args.view_offset).is_some() {
        /* @alwin: What this the correct behaviour here? Overmap? Fail? */
        todo!();
    }

    /* Find the window that contains the provided window virtual address */
    let src_win = p
        .find_window_containing(args.content_vaddr)
        .ok_or(InvocationError::InvalidArguments)?;
    let src_window_offset = (args.content_vaddr - src_win.borrow_mut().start);

    /* Check that the window has a view associated with it */
    let src_view = src_win
        .borrow_mut()
        .bound_view
        .as_ref()
        .ok_or(InvocationError::InvalidArguments)?
        .clone();

    /* Check that the view has a mapping associated with offset */

    if src_view.borrow().lookup_cap(src_window_offset).is_none() {
        let object_option = src_view.borrow().bound_object.clone();

        if object_option.as_ref().is_none() {
            /* @alwin: This is the case where the server tries to map in a page
              from a window that is backed by an externally managed view that has
              not been populated yet

              I think the right behaviour would be to continue forwarding the fault,
              but this may get complicated and requires a bit of thought so I'll
              deal with it when the situation comes up
            */
            todo!()
        }

        /* Otherwise, the view is backed by an anonymous memory object */

        /* Check if the anonymous memory object has a frame mapped at the right offset */
        let object = object_option.unwrap().clone();
        let obj_offset =
            src_view.borrow().obj_offset + (src_window_offset - src_view.borrow().win_offset);

        let obj_frame_cap = if object.borrow().lookup_frame(obj_offset).is_none() {
            /* Allocate a frame */
            let frame_ref = frame_table
                .alloc_frame(cspace, ut_table)
                .expect("@alwin: Should probs not be an assert");
            let orig_frame_cap = frame_table.frame_from_ref(frame_ref).get_cap();
            object
                .borrow_mut()
                .insert_frame_at(obj_offset, (orig_frame_cap, frame_ref));

            /* Zero-out the frame */
            let frame_data = frame_table.frame_data(frame_ref);
            frame_data[0..4096].fill(0);
            orig_frame_cap
        } else {
            /* The object already has a frame at that location */
            object.borrow().lookup_frame(obj_offset).unwrap().cap
        };

        /* Copy the frame into the view*/
        let view_frame_slot = cspace
            .alloc_slot()
            .expect("@alwin: this should not be an assert");
        let view_frame_cap = sel4::CPtr::from_bits(view_frame_slot.try_into().unwrap())
            .cast::<sel4::cap_type::UnspecifiedFrame>();
        cspace.root_cnode().relative(view_frame_cap).copy(
            &cspace.root_cnode().relative(obj_frame_cap),
            // @alwin: Be careful about rights here
            sel4::CapRightsBuilder::all().build(),
        );

        src_view
            .borrow_mut()
            .insert_cap_at(src_window_offset, view_frame_cap);
    }

    /* If we get to this point, there is guaranteed to be a mapping in the view */

    /* Copy from the src view into the dst view */
    let dst_view_frame_slot = cspace
        .alloc_slot()
        .expect("@alwin: this should not be an assert");
    let dst_view_frame_cap = sel4::CPtr::from_bits(dst_view_frame_slot.try_into().unwrap())
        .cast::<sel4::cap_type::UnspecifiedFrame>();
    cspace.root_cnode.relative(dst_view_frame_cap).copy(
        &cspace
            .root_cnode
            .relative(src_view.borrow().lookup_cap(src_window_offset).unwrap().cap),
        // @alwin: Be careful about rights here
        sel4::CapRightsBuilder::all().build(),
    );

    dst_view
        .borrow_mut()
        .insert_cap_at(args.view_offset, dst_view_frame_cap);

    if dst_view.borrow().pending_fault.is_some() {
        /* Map the page into the faulting process */
        // warn_rs!("Mapping frame");
        map_frame(
            cspace,
            ut_table,
            dst_view.borrow().lookup_cap(args.view_offset).unwrap().cap,
            dst_view.borrow().pending_fault.as_ref().unwrap().2,
            ROUND_DOWN(
                dst_view.borrow().pending_fault.as_ref().unwrap().1.addr() as usize,
                sel4_sys::seL4_PageBits.try_into().unwrap(),
            ),
            dst_view.borrow().rights.clone(),
            sel4::VmAttributes::DEFAULT,
            None,
        );

        /* Respond to the faulting process */
        let msginfo = sel4::MessageInfoBuilder::default().build();

        /* @alwin: I have to cast the reply object into an ep and calls end on it instead
        of doing a normal replu */
        dst_view
            .borrow()
            .pending_fault
            .as_ref()
            .unwrap()
            .0
             .0
            .send(msginfo);

        /* Get rid of the reply cap */
        dealloc_retyped(
            cspace,
            ut_table,
            dst_view.borrow().pending_fault.as_ref().unwrap().0,
        );
        dst_view.borrow_mut().pending_fault = None;
    }

    return Ok(SMOSReply::PageMap);
}

pub fn handle_vm_fault(
    cspace: &mut CSpace,
    frame_table: &mut FrameTable,
    ut_table: &mut UTTable,
    reply: RSReplyWrapper,
    proc: &mut UserProcess,
    fault_info: sel4::VmFault,
) -> Option<FaultReply> {
    let window = proc.find_window_containing(fault_info.addr() as usize);

    /* Maybe should return None here and possibly clean up the process? */
    if window.is_none() {
        warn_rs!(
            "Process faulted at vaddr: 0x{:x}, which is not inside a window",
            fault_info.addr()
        );
        return Some(FaultReply::VMFault { resume: false });
    }

    let window_unwrapped = window.unwrap();

    /* Maybe should return None here and possibly clean up the process? */
    if window_unwrapped.borrow().bound_view.is_none() {
        warn_rs!(
            "Process faulted at vaddr: 0x{:x}, which does not have a bound view",
            fault_info.addr()
        );
        return Some(FaultReply::VMFault { resume: false });
    }

    let window_vaddr = window_unwrapped.borrow().start;
    let fault_offset = fault_info.addr() as usize - window_vaddr;

    let view = window_unwrapped
        .borrow()
        .bound_view
        .as_ref()
        .unwrap()
        .clone();

    if view.borrow().lookup_cap(fault_offset).is_none() {
        let object_option = view.borrow().bound_object.clone();

        if object_option.as_ref().is_none() {
            forward_vm_fault(
                proc,
                view.clone(),
                ROUND_DOWN(fault_offset, sel4_sys::seL4_PageBits.try_into().unwrap()),
                reply,
                fault_info,
            );

            return None;
        }

        let object = object_option.unwrap().clone();

        let obj_offset = view.borrow().obj_offset + (fault_offset - view.borrow().win_offset);

        let obj_frame_cap = if object.borrow().lookup_frame(obj_offset).is_none() {
            /* Allocate a frame */
            let frame_ref = frame_table
                .alloc_frame(cspace, ut_table)
                .expect("@alwin: Should probs not be an assert");
            let orig_frame_cap = frame_table.frame_from_ref(frame_ref).get_cap();
            object
                .borrow_mut()
                .insert_frame_at(obj_offset, (orig_frame_cap, frame_ref));

            /* Zero-out the frame */
            let frame_data = frame_table.frame_data(frame_ref);
            frame_data[0..4096].fill(0);
            orig_frame_cap
        } else {
            object.borrow().lookup_frame(obj_offset).unwrap().cap
        };

        let view_frame_cap = cspace
            .alloc_cap::<sel4::cap_type::UnspecifiedFrame>()
            .expect("@alwin: This should not be an assert");
        cspace.root_cnode().relative(view_frame_cap).copy(
            &cspace.root_cnode().relative(obj_frame_cap),
            // @alwin: Be careful about rights here
            sel4::CapRightsBuilder::all().build(),
        );

        view.borrow_mut()
            .insert_cap_at(fault_offset, view_frame_cap);
    }

    /* Map views[idx] into virtual address space */
    // @alwin: This leaks page table caps!! How can this be resolved? Do we need a proper page
    // table after all?
    map_frame(
        cspace,
        ut_table,
        view.borrow().lookup_cap(fault_offset).unwrap().cap,
        proc.vspace.0,
        ROUND_DOWN(
            fault_info.addr() as usize,
            sel4_sys::seL4_PageBits.try_into().unwrap(),
        ),
        view.borrow().rights.clone(),
        sel4::VmAttributes::DEFAULT,
        None,
    );

    return Some(FaultReply::VMFault { resume: true });
}
