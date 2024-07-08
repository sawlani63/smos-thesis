use crate::connection::*;
use crate::cspace::{CSpace, CSpaceTrait};
use crate::frame_table::FrameTable;
use crate::handle::RootServerResource;
use crate::object::*;
use crate::proc::{handle_load_complete, handle_process_spawn};
use crate::util::alloc_retype;
use crate::view::*;
use crate::vm::handle_page_map;
use crate::window::*;
use crate::{
    proc::{procs_get_mut, UserProcess},
    ut::UTTable,
};
use smos_common::connection::RootServerConnection;
use smos_common::error::InvocationError;
use smos_common::local_handle;
use smos_common::local_handle::LocalHandle;
use smos_server::handle::{HandleAllocater, ServerHandle};
use smos_server::handle_capability::HandleCapabilityTable;
use smos_server::reply::SMOSReply;
use smos_server::{error::handle_error, reply::handle_reply, syscalls::*};

pub fn handle_reply_create(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
    p: &mut UserProcess,
) -> Result<SMOSReply, InvocationError> {
    let reply =
        alloc_retype::<sel4::cap_type::Reply>(cspace, ut_table, sel4::ObjectBlueprint::Reply)
            .map_err(|_| InvocationError::InsufficientResources)?;

    let (idx, handle_ref) = p.allocate_handle()?;

    *handle_ref = Some(ServerHandle::new(RootServerResource::Reply(reply)));

    return Ok(SMOSReply::ReplyCreate {
        hndl: LocalHandle::new(idx),
        reply: reply.0,
    });
}

pub fn handle_syscall(
    msg: sel4::MessageInfo,
    pid: usize,
    cspace: &mut CSpace,
    frame_table: &mut FrameTable,
    ut_table: &mut UTTable,
    handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
    sched_control: sel4::cap::SchedControl,
    ep: sel4::cap::Endpoint,
    recv_slot: sel4::AbsoluteCPtr,
) -> Option<sel4::MessageInfo> {
    let mut p = procs_get_mut(pid)
        .as_mut()
        .expect("Was called with invalid badge")
        .borrow_mut();

    /* Safety: It is necessary to construct this from a raw pointer because otherwise there is
       an issue where frame table is borrowed as mutable and immutable at the same time. This is
       still safe, because frame_data is static after it has been initialized and will never be
       changed by an access to the frame table
    */
    let shared_buf = unsafe { &(*frame_table.frame_data_raw(p.shared_buffer.1)) };

    let (invocation, consumed_cap) = sel4::with_ipc_buffer(|buf| {
        SMOS_Invocation::new::<RootServerConnection>(buf, &msg, Some(shared_buf), recv_slot)
    });

    // The user provided an invalid argument
    if invocation.is_err() {
        if consumed_cap {
            recv_slot.delete();
        }
        return Some(sel4::with_ipc_buffer_mut(|buf| {
            handle_error(buf, invocation.unwrap_err())
        }));
    }

    let ret = match invocation.unwrap() {
        SMOS_Invocation::WindowCreate(t) => handle_window_create(&mut p, handle_cap_table, &t),
        SMOS_Invocation::WindowDestroy(t) => {
            handle_window_destroy(cspace, &mut p, handle_cap_table, &t)
        }
        SMOS_Invocation::WindowRegister(t) => handle_window_register(&mut p, handle_cap_table, &t),
        SMOS_Invocation::ConnCreate(t) => handle_conn_create(cspace, &mut p, &t),
        SMOS_Invocation::ConnDestroy(t) => handle_conn_destroy(cspace, &mut p, &t),
        SMOS_Invocation::ObjCreate(t) => handle_obj_create(&mut p, handle_cap_table, &t),
        SMOS_Invocation::ObjDestroy(t) => {
            handle_obj_destroy(cspace, frame_table, &mut p, handle_cap_table, &t)
        }
        SMOS_Invocation::View(t) => handle_view(&mut p, handle_cap_table, &t),
        SMOS_Invocation::Unview(t) => handle_unview(cspace, &mut p, &t),
        SMOS_Invocation::ConnPublish(t) => {
            handle_conn_publish(cspace, ut_table, frame_table, &mut p, t)
        }
        SMOS_Invocation::ReplyCreate => handle_reply_create(cspace, ut_table, &mut p),
        SMOS_Invocation::ServerHandleCapCreate(t) => {
            handle_server_handle_cap_create(cspace, &mut p, &t)
        }
        SMOS_Invocation::ProcessSpawn(t) => {
            handle_process_spawn(cspace, ut_table, frame_table, sched_control, ep, &mut p, t)
        }
        SMOS_Invocation::LoadComplete(t) => handle_load_complete(cspace, frame_table, &mut p, t),
        SMOS_Invocation::ConnRegister(t) => handle_conn_register(&mut p, &t),
        SMOS_Invocation::PageMap(t) => handle_page_map(cspace, ut_table, frame_table, &mut p, &t),
        SMOS_Invocation::WindowDeregister(t) => handle_window_deregister(cspace, &mut p, &t),
        SMOS_Invocation::ConnDeregister(t) => handle_conn_deregister(&mut p, &t),
        _ => todo!(),
    };

    // Have to be careful here, make sure this is always before the next bit or the ipc buffer of
    // the response will become corrupted
    // @alwin: I'm pretty sure there is never any reason for the root server to recieve capabilities
    if (consumed_cap) {
        recv_slot.delete();
    }

    let msginfo = match ret {
        Ok(x) => sel4::with_ipc_buffer_mut(|buf| handle_reply(buf, x)),
        Err(x) => sel4::with_ipc_buffer_mut(|buf| handle_error(buf, x)),
    };

    return Some(msginfo);
}
