use smos_server::syscalls::{ConnCreate, ConnPublish, ConnRegister, ConnDestroy, ConnDeregister,
							ServerHandleCapCreate};
use crate::object::{AnonymousMemoryObject, OBJ_MAX_FRAMES};
use crate::page::PAGE_SIZE_4K;
use crate::proc::{UserProcess, procs_get};
use smos_server::reply::SMOSReply;
use smos_common::error::InvocationError;
use alloc::string::String;
use crate::cspace::{CSpace, CSpaceTrait};
use alloc::rc::Rc;
use crate::handle::{RootServerResource};
use smos_server::handle::{ServerHandle};
use crate::alloc::string::ToString;
use smos_common::local_handle;
use crate::util::{alloc_retype, dealloc_retyped};
use core::borrow::BorrowMut;
use core::cell::RefCell;
use smos_common::util::BIT;
use crate::ut::{UTTable, UTWrapper};
use crate::window::Window;
use crate::view::View;
use alloc::vec::Vec;
use alloc::collections::btree_map::BTreeMap;
use smos_server::event::{NTFN_SIGNAL_BITS, INVOCATION_EP_BITS};
use smos_server::handle::HandleAllocater;
use smos_server::ntfn_buffer::{NtfnBufferData, NotificationType, ConnDestroyNotification,
							   init_ntfn_buffer, enqueue_ntfn_buffer_msg};
use crate::frame_table::FrameTable;
use smos_common::local_handle::LocalHandle;


#[derive(Debug, Clone)]
pub struct Server {
	pid: usize,
	pub unbadged_ep: (sel4::cap::Endpoint, UTWrapper),
	unbadged_ntfn: (sel4::cap::Notification, UTWrapper),
	pub badged_ntfn: sel4::cap::Notification,
	ntfn_buffer_view: Rc<RefCell<View>>,
	pub ntfn_buffer_addr: *mut u8, //@alwin: I am unsure if this is the best way to appraoch this, but having something with a lifetime in here is extremely painful
	pub connections: Vec<Rc<RefCell<Connection>>>
}

#[derive(Clone, Debug)]
pub struct Connection {
	id: usize,
	client: Rc<RefCell<UserProcess>>, // @alwin: Not 100% sure if this is needed yet
	server: Rc<RefCell<Server>>,
	badged_ep: sel4::cap::Endpoint,
	registered: bool,
}

const MAX_SERVERS: usize = 10;

const ARRAY_REPEAT_VALUE: Option<Rc<RefCell<Server>>> = None;
static mut servers: BTreeMap<String, Rc<RefCell<Server>>> = BTreeMap::new();

fn find_server_with_name(name: &str) -> Option<Rc<RefCell<Server>>> {
	unsafe {
		Some(servers.get(name)?.clone())
	}
}

pub fn handle_conn_register(p: &mut UserProcess, args: &ConnRegister) -> Result<SMOSReply, InvocationError> {
	let server_ref = p.get_handle_mut(args.publish_hndl.idx).or(Err(InvocationError::InvalidHandle {which_arg: 0}))?;
	let server: Rc<RefCell<Server>> = match server_ref.as_ref().unwrap().inner() {
		RootServerResource::Server(sv) => Ok(sv.clone()),
		_ => Err(InvocationError::InvalidHandle{ which_arg: 0})
	}?;

	/* Safety: The server is able to randomly provide a client id, but this only lets them
	   register to connections with a connection that has that ID in its list of connections.
	   The only bad thing that I think could happen is that a server could get info about
	   a client after the call conn_create, but before they call conn_open(), but idk if
	   this is a big deal */

	for connection in &server.borrow().connections {
		if connection.borrow().id == args.client_id {
			// @alwin: why do I need to do an explicit dereference thing?
			(**connection).borrow_mut().registered = true;
    		let (idx, handle_ref) = p.allocate_handle()?;
		    *handle_ref = Some(ServerHandle::new(RootServerResource::ConnRegistration(connection.clone())));
    		return Ok(SMOSReply::ConnRegister {hndl: LocalHandle::new(idx)})
		}
	}

	return Err(InvocationError::InvalidArguments);
}

pub fn handle_conn_deregister(p: &mut UserProcess, args: &ConnDeregister) -> Result<SMOSReply, InvocationError> {
	let conn_reg_ref = p.get_handle_mut(args.hndl.idx).or(Err(InvocationError::InvalidHandle {which_arg: 0}))?;
	let conn: Rc<RefCell<Connection>> = match conn_reg_ref.as_ref().unwrap().inner() {
		RootServerResource::ConnRegistration(cr) => Ok(cr.clone()),
		_ => Err(InvocationError::InvalidHandle {which_arg: 0})
	}?;

	(*conn).borrow_mut().registered = false;
	p.cleanup_handle(args.hndl.idx);

	return Ok(SMOSReply::ConnDeregister);
}

pub fn handle_conn_create(cspace: &mut CSpace, p: &mut UserProcess, args: &ConnCreate) -> Result<SMOSReply, InvocationError> {
	let pid = p.pid;

    let server = find_server_with_name(args.name).ok_or(InvocationError::InvalidArguments)?;
    // @alwin: Ideally we would want to partition the RS cspace to prevent any one process from
    // being able to consume too much of it.

    // @alwin: Should the copy of the cap here be restricted to only have send rights?

    // Make an endpoint cap badged with the client's PID @alwin: Should it be badged with something else?
    // idk, but it should probably be unique (over the lifetime of the process or system)
    // so that servers don't get confused.
    let slot = cspace.alloc_slot().or(Err(InvocationError::InsufficientResources))?;
    cspace.root_cnode().relative_bits_with_depth(slot.try_into().unwrap(), sel4::WORD_SIZE)
    	  .mint(&cspace.root_cnode().relative(server.borrow().unbadged_ep.0), sel4::CapRightsBuilder::all().build(),
    	  		((pid | INVOCATION_EP_BITS) as u64).try_into().unwrap()).expect("Why did this fail?");

   let connection = Rc::new( RefCell::new( Connection {
   								id: pid,
   								client: procs_get(p.pid).as_ref().unwrap().clone(),
   								server: server.clone(),
   								badged_ep: sel4::CPtr::from_bits(slot.try_into().unwrap()).cast::<sel4::cap_type::Endpoint>(),
   								registered: false
  	}));

   	(*server).borrow_mut().connections.push(connection.clone());

	let (idx, handle_ref) = p.allocate_handle()?;
   	*handle_ref = Some(ServerHandle::new(RootServerResource::Connection(connection.clone())));

	return Ok(SMOSReply::ConnCreate {hndl: LocalHandle::new(idx), ep: connection.borrow().badged_ep});
}

pub fn handle_conn_destroy(cspace: &mut CSpace, p: &mut UserProcess, args: &ConnDestroy) -> Result<SMOSReply, InvocationError> {
	let conn_ref = p.get_handle_mut(args.hndl.idx).or(Err(InvocationError::InvalidHandle {which_arg: 0}))?;
	let conn = match conn_ref.as_ref().unwrap().inner() {
		RootServerResource::Connection(conn) => Ok(conn.clone()),
		_ => Err(InvocationError::InvalidHandle{ which_arg: 0})
	}?;

	cspace.root_cnode().relative(conn.borrow().badged_ep).revoke();

	/* Forward the connection destroyed notification if necessary */
	if conn.borrow().registered {
		let msg = NotificationType::ConnDestroyNotification(ConnDestroyNotification {
			conn_id: conn.borrow().id
		});

		unsafe { enqueue_ntfn_buffer_msg(conn.borrow().server.borrow().ntfn_buffer_addr, msg)};

		conn.borrow().server.borrow().badged_ntfn.signal();
	}

	p.cleanup_handle(args.hndl.idx);

	return Ok(SMOSReply::ConnDestroy);
}

pub fn handle_server_handle_cap_create(cspace: &mut CSpace, p: &mut UserProcess,
								args: &ServerHandleCapCreate) -> Result<SMOSReply, InvocationError> {

	let server_ref = p.get_handle_mut(args.publish_hndl.idx).or(Err(InvocationError::InvalidHandle {which_arg: 0}))?;
	let server: Rc<RefCell<Server>> = match server_ref.as_ref().unwrap().inner() {
		RootServerResource::Server(sv) => Ok(sv.clone()),
		_ => Err(InvocationError::InvalidHandle{ which_arg: 0})
	}?;

	/* Create a badged copy of the cap the server listens on with badge == args.ident  */
	let badged_cap = cspace.alloc_cap::<sel4::cap_type::Endpoint>().or(Err(InvocationError::InsufficientResources))?;
	cspace.root_cnode.relative(badged_cap).mint(&cspace.root_cnode().relative(server.borrow().unbadged_ep.0),
													   sel4::CapRights::none(),
													   args.ident as u64).map_err(|_| {
		cspace.free_cap(badged_cap);
		InvocationError::InsufficientResources
   })?;

 	/* Put a handle to this server-created handle cap in the handle table */
	let (idx, handle_ref) = p.allocate_handle().map_err(|e| {
		cspace.delete_cap(badged_cap);
		cspace.free_cap(badged_cap);
		e
	})?;

	*handle_ref = Some(ServerHandle::new(RootServerResource::HandleCap(badged_cap)));

	return Ok(SMOSReply::ServerHandleCapCreate {hndl: LocalHandle::new(idx), cap: badged_cap});
}

pub fn handle_conn_publish(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable,
						   p: &mut UserProcess, args: ConnPublish) -> Result<SMOSReply, InvocationError> {

	if find_server_with_name(args.name).is_some() {
		return Err(InvocationError::InvalidArguments);
	}

	/* Check that we can create a window at the specified address */
	if (args.ntfn_buffer % PAGE_SIZE_4K != 0) {
		return Err(InvocationError::AlignmentError { which_arg: 0 });
	}

	/* @alwin: Check notification is in user-addressable memory */

	if p.overlapping_window(args.ntfn_buffer, PAGE_SIZE_4K) {
		return Err(InvocationError::InvalidArguments);
	}

	/* Create an EP for the server to listen on */
	let ep = alloc_retype::<sel4::cap_type::Endpoint>(cspace, ut_table, sel4::ObjectBlueprint::Endpoint).map_err(|_| {
		InvocationError::InsufficientResources
	})?;

	/* Create a notification to bind to the TCB */
	let ntfn = alloc_retype(cspace, ut_table, sel4::ObjectBlueprint::Notification).map_err(|_| {
		dealloc_retyped(cspace, ut_table, ep);
		InvocationError::InsufficientResources
	})?;

	/* Bind the notification to the TCB */
	// @alwin: What to return and how to clean up on failure?
	p.tcb.0.tcb_bind_notification(ntfn.0).map_err(|_| {
		dealloc_retyped(cspace, ut_table, ntfn);
		dealloc_retyped(cspace, ut_table, ep);
		InvocationError::InsufficientResources
	})?;

	/* Create a badged notification cap that the RS uses to communicate with the server */
	let badged_ntfn_cap = cspace.alloc_cap::<sel4::cap_type::Notification>().map_err(|_| {
		dealloc_retyped(cspace, ut_table, ntfn);
		dealloc_retyped(cspace, ut_table, ep);
		InvocationError::InsufficientResources
	})?;

	cspace.root_cnode().relative(badged_ntfn_cap).mint(&cspace.root_cnode().relative(ntfn.0),
													   sel4::CapRights::all(),
													   NTFN_SIGNAL_BITS as u64).map_err(|_| {
    	cspace.free_cap(badged_ntfn_cap);
    	dealloc_retyped(cspace, ut_table, ep);
    	dealloc_retyped(cspace, ut_table, ntfn);
		InvocationError::InsufficientResources
   	})?;

    /* Pre-allocate the frame used for the notification buffer */
    let frame_ref = frame_table.alloc_frame(cspace, ut_table).ok_or_else(|| {
    	// cspace.delete(badged_ntfn_cap);
    	cspace.free_cap(badged_ntfn_cap);
    	dealloc_retyped(cspace, ut_table, ep);
    	dealloc_retyped(cspace, ut_table, ntfn);
		InvocationError::InsufficientResources
    })?;
	let orig_frame_cap = frame_table.frame_from_ref(frame_ref).get_cap();

    /* Create the notification buffer */
    let window = Rc::new(RefCell::new (Window {
    	start: args.ntfn_buffer,
    	size: PAGE_SIZE_4K,
    	bound_view: None
    }));


    let object = Rc::new( RefCell::new (AnonymousMemoryObject {
    	size: PAGE_SIZE_4K,
    	rights: sel4::CapRights::all(),
    	frames: [None; OBJ_MAX_FRAMES],
    	associated_views: Vec::new()
    }));

    let view = Rc::new(RefCell::new (View {
    	caps: [None; OBJ_MAX_FRAMES],
    	bound_window: window.clone(),
    	bound_object: Some(object.clone()),
    	managing_server_info: None,
    	rights: sel4::CapRights::all(),
    	win_offset: 0,
    	obj_offset: 0,
        pending_fault: None
    }));

    (*window).borrow_mut().bound_view = Some(view.clone());
    (*object).borrow_mut().associated_views.push(view.clone());
    (*object).borrow_mut().frames[0] = Some((orig_frame_cap, frame_ref));

    p.add_window_unchecked(window);
    p.views.push(view.clone());

    /* Initialize the notification buffer */
    // Safety: The address we use for the notification buffer is that of the root server's mapping
    // of the frame selected for the ntfn buffer. This will never be reused until the process
    // deregisters as a server.
    unsafe { init_ntfn_buffer(frame_table.frame_data(frame_ref).as_mut_ptr()) }

    let pid = p.pid;
    let (idx, handle_ref) = p.allocate_handle()?;

    /* Create the server struct */
    let server = Rc::new(RefCell::new(Server {
    	pid: pid,
    	unbadged_ep: ep,
    	unbadged_ntfn: ntfn,
    	badged_ntfn: badged_ntfn_cap,
    	ntfn_buffer_view: view.clone(),
    	ntfn_buffer_addr: frame_table.frame_data(frame_ref).as_mut_ptr(),
    	connections: Vec::new()
    }));

    /* Put the server into the handle table and the server hashmap  */
    unsafe { servers.insert(args.name.to_string(), server.clone()) };
    *handle_ref = Some(ServerHandle::new(RootServerResource::Server(server)));

    return Ok(SMOSReply::ConnPublish {hndl: local_handle::LocalHandle::new(idx), ep: ep.0});

}