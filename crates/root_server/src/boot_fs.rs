


fn handle_conn_open(p: &mut UserProcess, args: &ConnOpen) -> Result<SMOSReply, InvocationError> {
	
}


pub fn handle_bfs_invocation(msginfo: sel4::MessageInfo, pid: usize, cspace: &mut CSpace,
							 frame_table: &FrameTable) -> sel4::MessageInfo {

	p = procs_get_mut(pid).as_mut().expect("Was called with invalid badge");

	let shared_buffer = match p.bfs_shared_buffer {
		Some(x) => Some(frame_table.frame_data(x.1)),
		None => None,
	};

	let invocation = sel4::with_ipc_buffer(|buf| SMOSInvocation::new::<ObjectServerConnection>(buf, &msg, shared_buffer));

	if invocation.is_err() {
		return Some(sel4::with_ipc_buffer_mut(|buf| handle_error(buf, invocation.unwrap_err())));
	}

	let ret = match invocation.unwrap() {
		SMOSInvocation::ConnOpen(t) => handle_conn_open(p, &t),
		SMOSInvocation::ConnClose(t) => handle_conn_close(p, &t),
		SMOSInvocation::ObjOpen(t) => handle_obj_open(p, &t),
		SMOSInvocation::ObjClose(t) => handle_obj_close(p, &t),
		SMOSInvocation::View(t) => handle_view(p, &t)
		_ => todo!();
	}

	let msginfo = match ret {
        Ok(x) => sel4::with_ipc_buffer_mut(|buf| handle_reply(buf, x)),
        Err(x) => sel4::with_ipc_buffer_mut(|buf| handle_error(buf, x)),
    }

    return Some(msginfo);
}