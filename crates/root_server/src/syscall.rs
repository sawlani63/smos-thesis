use smos_server::{syscalls::{*}, error::handle_error, reply::handle_reply};
use smos_common::connection::RootServerConnection;
use crate::proc::{UserProcess, procs_get_mut};
use crate::handle_capability::allocate_handle_cap;
use crate::handle::Handle;
use smos_common::error::InvocationError;
use crate::window::{*};
use crate::object::{*};
use crate::view::{*};
use crate::connection::{*};
use crate::frame_table::FrameTable;
use crate::cspace::CSpace;
use smos_server::reply::SMOSReply;

fn handle_obj_open(args: ObjOpen) {
    todo!();
}


pub fn handle_syscall(msg: sel4::MessageInfo, pid: usize, cspace: &mut CSpace, frame_table: &mut FrameTable) -> Option<sel4::MessageInfo> {
    let p = procs_get_mut(pid).as_mut().expect("Was called with invalid badge");

    let invocation = sel4::with_ipc_buffer(|buf| SMOS_Invocation::new::<RootServerConnection>(buf, &msg, Some(frame_table.frame_data(p.shared_buffer.1))));

    // The user provided an invalid argument
    if invocation.is_err() {
        warn_rs!("Got an error {:?}", invocation);
        return Some(sel4::with_ipc_buffer_mut(|buf| handle_error(buf, invocation.unwrap_err())));
    }

    let ret = match invocation.unwrap() {
        SMOS_Invocation::WindowCreate(t) => handle_window_create(p, &t),
        SMOS_Invocation::WindowDestroy(t) => handle_window_destroy(p, &t),
        SMOS_Invocation::ConnCreate(t) => handle_conn_create(cspace, p, &t),
        SMOS_Invocation::ObjCreate(t) => handle_obj_create(p, &t),
        SMOS_Invocation::View(t) => handle_view(p, &t),
        _ => todo!()
        // SMOS_Invocation::ObjCreate(t) => handle_obj_create(p, t),
        // SMOS_Invocation::ObjOpen(t) => handle_obj_open(p, t),
        // SMOS_Invocation::ObjView(t) => handle_obj_view(p, t)
    };

    let msginfo = match ret {
        Ok(x) => sel4::with_ipc_buffer_mut(|buf| handle_reply(buf, x)),
        Err(x) => sel4::with_ipc_buffer_mut(|buf| handle_error(buf, x)),
    };

    return Some(msginfo);
}