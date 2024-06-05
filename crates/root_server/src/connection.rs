use smos_server::syscalls::{ConnCreate};
use crate::proc::UserProcess;
use smos_server::reply::SMOSReply;
use smos_common::error::InvocationError;
use alloc::string::String;
use crate::cspace::{CSpace, CSpaceTrait};
use alloc::rc::Rc;
use crate::handle::{Handle, SMOSObject};
use crate::alloc::string::ToString;
use smos_common::local_handle;
use crate::util::BFS_EP_BITS;
use core::cell::RefCell;

#[derive(Debug, Clone)]
struct Server {
	name: String,
	unbadged_ep: sel4::cap::Endpoint
}

#[derive(Clone, Debug)]
pub struct Connection {
	server: &'static Option<Server>,
	badged_ep: sel4::cap::Endpoint
}

const MAX_SERVERS: usize = 10;

const ARRAY_REPEAT_VALUE: Option<Server> = None;
// @alwin: This should probably be a hashmap keyed by name instead
static mut servers: [Option<Server>; MAX_SERVERS] = [ARRAY_REPEAT_VALUE; MAX_SERVERS];

fn find_empty_slot() -> Option<&'static mut Option<Server>> {
	unsafe {
		for server in servers.iter_mut() {
			if server.is_none() {
				return Some(server);
			}
		}
	}

	return None;
}

fn find_server_with_name(name: &String) -> Option<&'static mut Option<Server>> {
	unsafe {
		for server in servers.iter_mut() {
			match server {
				Some(server_internal) => if server_internal.name == *name { return Some(server) }
				_ => (),
			}
		}
	}

	return None;
}

pub fn handle_conn_create(cspace: &mut CSpace, p: &mut UserProcess, args: &ConnCreate) -> Result<SMOSReply, InvocationError> {
	log_rs!("In handle_conn_create! Creating connection to {:?}", args.name);
	if (args.name != *"BOOT_FS") {
		// @alwin: Deal with this once publish has been sorted out
		// log_rs!("{} {}", args.name, )
		todo!()
	}

	let pid = p.pid;

	let (idx, handle_ref) = p.allocate_handle()?;

	/* We are the boot file server. We can probably just re-issue the same cap they are
	   already using to talk to us. Actually, this might be a bad idea because then revoking the
	   capability used for the connection would sever both connections, which is kind of bad*/
    // @alwin: Deal with this later

    let server = find_server_with_name(&args.name).ok_or(InvocationError::InvalidArguments)?;
    // @alwin: Ideally we would want to partition the RS cspace to prevent any one process from
    // being able to consume too much of it.

    // @alwin: Should the copy of the cap here be restricted to only have send rights?

    // Make an endpoint cap badged with the client's PID @alwin: Should it be badged with something else?
    // idk, but it should probably be unique (over the lifetime of the process or system)
    // so that servers don't get confused.
    let slot = cspace.alloc_slot().or(Err(InvocationError::InsufficientResources))?;
    cspace.root_cnode().relative_bits_with_depth(slot.try_into().unwrap(), sel4::WORD_SIZE)
    	  .mint(&cspace.root_cnode().relative(server.as_ref().unwrap().unbadged_ep), sel4::CapRightsBuilder::all().build(),
    	  		(pid | BFS_EP_BITS).try_into().unwrap()).expect("Why did this fail?");

   let connection = Rc::new( RefCell::new( Connection {
   								server: server,
   								badged_ep: sel4::CPtr::from_bits(slot.try_into().unwrap()).cast::<sel4::cap_type::Endpoint>()
  	}));

   	// p.connections.push(connection.clone());
   	*handle_ref = Some(Handle::new(SMOSObject::Connection(connection.clone())));

	return Ok(SMOSReply::ConnCreate {hndl: local_handle::Handle::new(idx), ep: connection.borrow_mut().badged_ep});
}


pub fn publish_boot_fs(ep: sel4::cap::Endpoint) {
	let slot = find_empty_slot().expect("Could not publish boot file server for some reason");

	*slot = Some( Server {
		name: "BOOT_FS".to_string(),
		unbadged_ep: ep
	})
}

pub fn handle_conn_publish(p: &mut UserProcess) {}