use smos_server::{syscalls::{*}, error::handle_error};
use smos_common::connection::RootServerConnection;

fn handle_window_create(args: WindowCreate) {
    todo!();
}

fn handle_obj_create(args: ObjCreate) {
    todo!();
}

fn handle_obj_open(args: ObjOpen) {
    todo!();
}

fn handle_obj_view(args: ObjView) {
    todo!();
}

pub fn handle_syscall(msg: sel4::MessageInfo, badge: u64) -> Option<sel4::MessageInfo> {
    let invocation = sel4::with_ipc_buffer(|buf| SMOS_Invocation::new::<RootServerConnection>(buf, &msg));
    // The user provided an invalid argument
    if invocation.is_err() {
        return Some(sel4::with_ipc_buffer_mut(|buf| handle_error(invocation.unwrap_err(), buf)));
    }

    match invocation.unwrap() {
        SMOS_Invocation::WindowCreate(t) => handle_window_create(t),
        SMOS_Invocation::ObjCreate(t) => handle_obj_create(t),
        SMOS_Invocation::ObjOpen(t) => handle_obj_open(t),
        SMOS_Invocation::ObjView(t) => handle_obj_view(t)
    }

    return None

}