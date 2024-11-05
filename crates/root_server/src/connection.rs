use crate::alloc::string::ToString;
use crate::cspace::{CSpace, CSpaceTrait};
use crate::frame_table::FrameTable;
use crate::handle::RootServerResource;
use crate::irq::UserNotificationDispatch;
use crate::object::AnonymousMemoryObject;
use crate::page::PAGE_SIZE_4K;
use crate::proc::{procs_get, ProcessType, UserProcess};
use crate::ut::{UTTable, UTWrapper};
use crate::util::{alloc_retype, dealloc_retyped};
use crate::view::View;
use crate::window::Window;
use alloc::collections::btree_map::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use smos_common::error::InvocationError;
use smos_common::local_handle::{HandleCap, LocalHandle};
use smos_common::obj_attributes::ObjAttributes;
use smos_server::event::{INVOCATION_EP_BITS, IRQ_IDENT_BADGE_BITS, NTFN_BIT};
use smos_server::handle::HandleAllocater;
use smos_server::handle::ServerHandle;
use smos_server::handle_capability::HandleCapabilityTable;
use smos_server::ntfn_buffer::{
    enqueue_ntfn_buffer_msg, init_ntfn_buffer, ConnDestroyNotification, NotificationType,
};
use smos_server::reply::SMOSReply;
use smos_server::syscalls::{
    ChannelOpen, ConnCreate, ConnDeregister, ConnDestroy, ConnPublish, ConnRegister,
    ServerCreateChannel, ServerHandleCapCreate,
};

#[derive(Debug, Clone)]
pub struct Server {
    pid: usize,
    pub unbadged_ep: (sel4::cap::Endpoint, UTWrapper),
    pub ntfn_dispatch: UserNotificationDispatch,
    #[allow(dead_code)] // @alwin: Not used for now
    ntfn_buffer_view: Rc<RefCell<View>>,
    pub ntfn_buffer_addr: *mut u8, //@alwin: I am unsure if this is the best way to appraoch this, but having something with a lifetime in here is extremely painful
    pub connections: Vec<Rc<RefCell<Connection>>>,
}

#[derive(Clone, Debug)]
pub struct Connection {
    id: usize,
    #[allow(dead_code)] // @alwin: Not used for now
    client: Rc<RefCell<ProcessType>>, // @alwin: Not 100% sure if this is needed yet
    server: Rc<RefCell<Server>>,
    badged_ep: sel4::cap::Endpoint,
    registered: bool,
}

static mut SERVERS: BTreeMap<String, Rc<RefCell<Server>>> = BTreeMap::new();

fn find_server_with_name(name: &str) -> Option<Rc<RefCell<Server>>> {
    unsafe { Some(SERVERS.get(name)?.clone()) }
}

pub fn handle_conn_register(
    p: &mut UserProcess,
    args: &ConnRegister,
) -> Result<SMOSReply, InvocationError> {
    let server_ref = p
        .get_handle_mut(args.publish_hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;
    let server: Rc<RefCell<Server>> = match server_ref.as_ref().unwrap().inner() {
        RootServerResource::Server(sv) => Ok(sv.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    /* Safety: The server is able to randomly provide a client id, but this only lets them
    register to connections with a connection that has that ID in its list of connections.
    The only bad thing that I think could happen is that a server could get info about
    a client after the call conn_create, but before they call conn_open(), but idk if
    this is a big deal */

    for connection in &server.borrow().connections {
        if connection.borrow().id == args.client_id {
            connection.borrow_mut().registered = true;
            let (idx, handle_ref) = p.allocate_handle()?;
            *handle_ref = Some(ServerHandle::new(RootServerResource::ConnRegistration(
                connection.clone(),
            )));
            return Ok(SMOSReply::ConnRegister {
                hndl: LocalHandle::new(idx),
            });
        }
    }

    return Err(InvocationError::InvalidArguments);
}

pub fn handle_conn_deregister(
    p: &mut UserProcess,
    args: &ConnDeregister,
) -> Result<SMOSReply, InvocationError> {
    let conn_reg_ref = p
        .get_handle_mut(args.hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;
    let conn: Rc<RefCell<Connection>> = match conn_reg_ref.as_ref().unwrap().inner() {
        RootServerResource::ConnRegistration(cr) => Ok(cr.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    conn.borrow_mut().registered = false;
    p.cleanup_handle(args.hndl.idx)
        .expect("Failed to clean up handle");

    return Ok(SMOSReply::ConnDeregister);
}

pub fn handle_conn_create(
    cspace: &mut CSpace,
    p: &mut UserProcess,
    args: &ConnCreate,
) -> Result<SMOSReply, InvocationError> {
    let pid = p.pid;

    let server = find_server_with_name(args.name).ok_or(InvocationError::InvalidArguments)?;

    //  Don't let a process connect to itself
    if server.borrow_mut().pid == p.pid {
        warn_rs!("Attempting to create a connection with self");
        return Err(InvocationError::InvalidArguments);
    }

    // @alwin: Ideally we would want to partition the RS cspace to prevent any one process from
    // being able to consume too much of it.

    // @alwin: Should the copy of the cap here be restricted to only have send rights?

    // Make an endpoint cap badged with the client's PID @alwin: Should it be badged with something else?
    // idk, but it should probably be unique (over the lifetime of the process or system)
    // so that servers don't get confused.
    let slot = cspace
        .alloc_slot()
        .or(Err(InvocationError::InsufficientResources))?;
    cspace
        .root_cnode()
        .relative_bits_with_depth(slot.try_into().unwrap(), sel4::WORD_SIZE)
        .mint(
            &cspace.root_cnode().relative(server.borrow().unbadged_ep.0),
            sel4::CapRightsBuilder::all().build(),
            ((pid | INVOCATION_EP_BITS) as u64).try_into().unwrap(),
        )
        .expect("Why did this fail?");

    let connection = Rc::new(RefCell::new(Connection {
        id: pid,
        client: procs_get(p.pid).as_ref().unwrap().clone(),
        server: server.clone(),
        badged_ep: sel4::CPtr::from_bits(slot.try_into().unwrap())
            .cast::<sel4::cap_type::Endpoint>(),
        registered: false,
    }));

    server.borrow_mut().connections.push(connection.clone());

    let (idx, handle_ref) = p.allocate_handle()?;
    *handle_ref = Some(ServerHandle::new(RootServerResource::Connection(
        connection.clone(),
    )));

    return Ok(SMOSReply::ConnCreate {
        hndl: LocalHandle::new(idx),
        ep: connection.borrow().badged_ep,
    });
}

pub fn handle_conn_destroy(
    cspace: &mut CSpace,
    p: &mut UserProcess,
    args: &ConnDestroy,
) -> Result<SMOSReply, InvocationError> {
    let conn_ref = p
        .get_handle_mut(args.hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;
    let conn = match conn_ref.as_ref().unwrap().inner() {
        RootServerResource::Connection(conn) => Ok(conn.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    cspace
        .root_cnode()
        .relative(conn.borrow().badged_ep)
        .revoke()
        .expect("Failed to revoke badged endpoint");

    /* Forward the connection destroyed notification if necessary */
    if conn.borrow().registered {
        let msg = NotificationType::ConnDestroyNotification(ConnDestroyNotification {
            conn_id: conn.borrow().id,
        });

        unsafe {
            enqueue_ntfn_buffer_msg(conn.borrow().server.borrow().ntfn_buffer_addr, msg)
                .expect("@alwin: This probably shouldn't be an expect")
        };

        conn.borrow()
            .server
            .borrow()
            .ntfn_dispatch
            .rs_badged_ntfn()
            .signal();
    }

    p.cleanup_handle(args.hndl.idx)
        .expect("Failed to clean up handle");

    return Ok(SMOSReply::ConnDestroy);
}

pub fn handle_server_handle_cap_create(
    cspace: &mut CSpace,
    p: &mut UserProcess,
    args: &ServerHandleCapCreate,
) -> Result<SMOSReply, InvocationError> {
    let server_ref = p
        .get_handle_mut(args.publish_hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;
    let server: Rc<RefCell<Server>> = match server_ref.as_ref().unwrap().inner() {
        RootServerResource::Server(sv) => Ok(sv.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    /* Create a badged copy of the cap the server listens on with badge == args.ident  */
    let badged_cap = cspace
        .alloc_cap::<sel4::cap_type::Endpoint>()
        .or(Err(InvocationError::InsufficientResources))?;
    cspace
        .root_cnode
        .relative(badged_cap)
        .mint(
            &cspace.root_cnode().relative(server.borrow().unbadged_ep.0),
            sel4::CapRights::none(),
            args.ident as u64,
        )
        .map_err(|_| {
            cspace.free_cap(badged_cap);
            InvocationError::InsufficientResources
        })?;

    /* Put a handle to this server-created handle cap in the handle table */
    let (idx, handle_ref) = p.allocate_handle().map_err(|e| {
        cspace
            .delete_cap(badged_cap)
            .expect("Failed to delete badged capability");
        cspace.free_cap(badged_cap);
        e
    })?;

    *handle_ref = Some(ServerHandle::new(RootServerResource::HandleCap(badged_cap)));

    return Ok(SMOSReply::ServerHandleCapCreate {
        hndl: LocalHandle::new(idx),
        cap: badged_cap,
    });
}

pub fn handle_server_create_channel(
    cspace: &mut CSpace,
    handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
    p: &mut UserProcess,
    args: &ServerCreateChannel,
) -> Result<SMOSReply, InvocationError> {
    let server_ref = p
        .get_handle_mut(args.publish_hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;
    let server: Rc<RefCell<Server>> = match server_ref.as_ref().unwrap().inner() {
        RootServerResource::Server(sv) => Ok(sv.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    let (idx, handle_ref, cptr) = handle_cap_table.allocate_handle_cap()?;

    let (bit, ntfn_cap) = server.borrow_mut().ntfn_dispatch.ntfn_register(cspace)?;

    let channel_auth = (ntfn_cap, bit);

    *handle_ref = Some(ServerHandle::new(RootServerResource::ChannelAuthority(
        channel_auth,
    )));

    p.created_handle_caps.push(idx);

    return Ok(SMOSReply::ServerCreateChannel {
        bit: bit,
        hndl_cap: HandleCap::new(cptr.unwrap()),
    });
}

pub fn handle_channel_open(
    _p: &mut UserProcess,
    handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
    args: &ChannelOpen,
) -> Result<SMOSReply, InvocationError> {
    let channel_auth_ref = handle_cap_table
        .get_handle_cap_mut(args.hndl_cap.idx)
        .or(Err(InvocationError::InvalidHandleCapability {
            which_arg: 0,
        }))?;

    let channel_auth = match channel_auth_ref.as_ref().unwrap().inner() {
        RootServerResource::ChannelAuthority(ca) => Ok(ca),
        _ => Err(InvocationError::InvalidHandleCapability { which_arg: 0 }),
    }?;

    // @alwin: Should this return a handle?

    return Ok(SMOSReply::ChannelOpen {
        ntfn: channel_auth.0,
    });
}

pub fn handle_conn_publish(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
    frame_table: &mut FrameTable,
    p: &mut UserProcess,
    args: ConnPublish,
) -> Result<SMOSReply, InvocationError> {
    if find_server_with_name(args.name).is_some() {
        return Err(InvocationError::InvalidArguments);
    }

    /* Check that we can create a window at the specified address */
    if args.ntfn_buffer % PAGE_SIZE_4K != 0 {
        return Err(InvocationError::AlignmentError { which_arg: 0 });
    }

    /* Check notification buffer is in user-addressable memory */
    if args.ntfn_buffer >= sel4_sys::seL4_UserTop.try_into().unwrap() {
        return Err(InvocationError::InvalidArguments);
    }

    /* Check that the notification buffer does not overlap with another window */
    if p.overlapping_window(args.ntfn_buffer, PAGE_SIZE_4K)
        .is_some()
    {
        return Err(InvocationError::InvalidArguments);
    }

    /* Create an EP for the server to listen on */
    let ep =
        alloc_retype::<sel4::cap_type::Endpoint>(cspace, ut_table, sel4::ObjectBlueprint::Endpoint)
            .map_err(|_| InvocationError::InsufficientResources)?;

    /* Create a notification to bind to the TCB */
    let ntfn =
        alloc_retype(cspace, ut_table, sel4::ObjectBlueprint::Notification).map_err(|_| {
            dealloc_retyped(cspace, ut_table, ep);
            InvocationError::InsufficientResources
        })?;

    /* Bind the notification to the TCB */
    p.tcb.0.tcb_bind_notification(ntfn.0).map_err(|_| {
        dealloc_retyped(cspace, ut_table, ntfn);
        dealloc_retyped(cspace, ut_table, ep);
        InvocationError::InsufficientResources
    })?;

    let mut ntfn_dispatch = UserNotificationDispatch::new(
        sel4::init_thread::slot::IRQ_CONTROL.cap(),
        ntfn,
        NTFN_BIT,
        IRQ_IDENT_BADGE_BITS,
    );

    /* Create a badged notification cap that the RS uses to communicate with the server */
    ntfn_dispatch.ntfn_register(cspace).map_err(|_| {
        p.tcb
            .0
            .tcb_unbind_notification()
            .expect("Failed to unbind notification");
        dealloc_retyped(cspace, ut_table, ntfn);
        dealloc_retyped(cspace, ut_table, ep);
        InvocationError::InsufficientResources
    })?;

    /* Pre-allocate the frame used for the notification buffer */
    let frame_ref = frame_table.alloc_frame(cspace, ut_table).ok_or_else(|| {
        ntfn_dispatch.destroy(cspace, ut_table);
        p.tcb
            .0
            .tcb_unbind_notification()
            .expect("Failed to unbind notification");
        dealloc_retyped(cspace, ut_table, ep);
        dealloc_retyped(cspace, ut_table, ntfn);
        InvocationError::InsufficientResources
    })?;
    let orig_frame_cap = frame_table.frame_from_ref(frame_ref).get_cap();

    /* Create the notification buffer */
    let window = Rc::new(RefCell::new(Window {
        start: args.ntfn_buffer,
        size: PAGE_SIZE_4K,
        bound_view: None,
    }));

    let object = Rc::new(RefCell::new(AnonymousMemoryObject::new(
        PAGE_SIZE_4K,
        sel4::CapRights::all(),
        ObjAttributes::DEFAULT,
    )));

    let view = Rc::new(RefCell::new(View::new(
        window.clone(),
        Some(object.clone()),
        None,
        sel4::CapRights::all(),
        0,
        0,
    )));

    window.borrow_mut().bound_view = Some(view.clone());
    object.borrow_mut().associated_views.push(view.clone());
    object
        .borrow_mut()
        .insert_frame_at(0, (orig_frame_cap, frame_ref))
        .expect("Failed to insert frame into object");

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
        ntfn_dispatch: ntfn_dispatch,
        ntfn_buffer_view: view.clone(),
        ntfn_buffer_addr: frame_table.frame_data(frame_ref).as_mut_ptr(),
        connections: Vec::new(),
    }));

    /* Put the server into the handle table and the server hashmap  */
    unsafe { SERVERS.insert(args.name.to_string(), server.clone()) };
    *handle_ref = Some(ServerHandle::new(RootServerResource::Server(server)));

    return Ok(SMOSReply::ConnPublish {
        hndl: LocalHandle::new(idx),
        ep: ep.0,
    });
}
