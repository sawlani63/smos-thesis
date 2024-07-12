#![no_std]
#![no_main]
#![feature(const_refs_to_static)]

use core::cell::RefCell;
use elf::ElfBytes;
use include_bytes_aligned::include_bytes_aligned;
use smos_common::error::*;
use smos_common::local_handle::{
    ConnRegistrationHandle, ConnectionHandle, HandleOrHandleCap, LocalHandle, ObjectHandle,
    ViewHandle, WindowHandle, WindowRegistrationHandle,
};
use smos_common::syscall::{ObjectServerInterface, ReplyWrapper, RootServerInterface};
use smos_common::util::ROUND_UP;
use smos_common::{
    connection::{ObjectServerConnection, RootServerConnection},
    server_connection::ServerConnection,
};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_runtime::Never;
use smos_server::error::*;
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::handle::{
    generic_allocate_handle, generic_cleanup_handle, generic_get_handle,
    generic_invalid_handle_error, HandleAllocater, HandleInner, ServerHandle,
};
use smos_server::handle_arg::ServerReceivedHandleOrHandleCap;
use smos_server::handle_capability::{HandleCapability, HandleCapabilityTable};
use smos_server::reply::*;
use smos_server::syscalls::*;
extern crate alloc;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::ptr::NonNull;
use offset_allocator::{Allocation, Allocator};
use smos_server::ntfn_buffer::*;

// @alwin: Should really make sure nothing else starts on the same page as this ELF finishes
// to prevent information leakage. Maybe if another page aligned 'guard page' is added? Kinda
// relying on the behaviour of the compiler

const NUM_FILES: usize = 2;
const INIT_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("INIT_ELF"));
const ETH_DRIVER_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("ETH_DRIVER_ELF"));

#[derive(Debug, Copy, Clone)]
struct File {
    name: &'static str,
    data: &'static [u8],
}

static mut files: [Option<File>; NUM_FILES] = [None; NUM_FILES];

const ntfn_buffer: *mut u8 = 0xB0000 as *mut u8;
const SHARED_BUFFER_BASE: *mut u8 = 0xC0000 as *mut u8;

const MAX_HANDLES: usize = 16;

struct Object {
    file: &'static File,
    associated_views: Vec<Rc<RefCell<ViewData>>>,
}

struct ViewData {
    object: Rc<RefCell<Object>>,
    size: usize,
    win_offset: usize,
    obj_offset: usize,
    rights: sel4::CapRights,
    registration_hndl: LocalHandle<WindowRegistrationHandle>,
}

enum BFSResource {
    Object(Rc<RefCell<Object>>),
    View(Rc<RefCell<ViewData>>),
}

impl HandleInner for BFSResource {}

struct Client {
    id: usize,
    shared_buffer: Option<(
        *mut u8,
        usize,
        HandleOrHandleCap<WindowHandle>,
        LocalHandle<ViewHandle>,
        Allocation<u32>,
    )>,
    handles: [Option<ServerHandle<BFSResource>>; MAX_HANDLES],
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>,
}

impl HandleAllocater<BFSResource> for Client {
    fn handle_table_size(&self) -> usize {
        return MAX_HANDLES;
    }

    fn handle_table(&self) -> &[Option<ServerHandle<BFSResource>>] {
        return &self.handles;
    }

    fn handle_table_mut(&mut self) -> &mut [Option<ServerHandle<BFSResource>>] {
        return &mut self.handles;
    }
}

const MAX_CLIENTS: usize = 16;
const ARRAY_REPEAT_VALUE: Option<Client> = None;
static mut clients: [Option<Client>; MAX_CLIENTS] = [ARRAY_REPEAT_VALUE; MAX_CLIENTS];

fn find_client_from_id(id: usize) -> Option<&'static mut Option<Client>> {
    unsafe {
        for client in clients.iter_mut() {
            if client.as_ref().is_some() && client.as_ref().unwrap().id == id {
                return Some(client);
            }
        }
    }

    return None;
}

fn delete_client(to_del: &mut Client) -> Result<(), ()> {
    unsafe {
        for client in clients.iter_mut() {
            if client.is_some() && core::ptr::eq(client.as_ref().unwrap(), to_del) {
                *client = None;
                return Ok(());
            }
        }
    }

    return Err(());
}

fn find_client_slot() -> Option<&'static mut Option<Client>> {
    unsafe {
        for client in clients.iter_mut() {
            if client.as_ref().is_none() {
                return Some(client);
            }
        }
    }

    return None;
}

fn handle_obj_stat(
    client: &mut Client,
    handle_cap_table: &mut HandleCapabilityTable<BFSResource>,
    args: &ObjStat,
) -> Result<SMOSReply, InvocationError> {
    let hndl_ref = generic_get_handle(client, handle_cap_table, args.hndl, 0)?;

    let object = match hndl_ref.as_ref().unwrap().inner() {
        BFSResource::Object(obj) => Ok(obj.clone()),
        _ => Err(generic_invalid_handle_error(args.hndl, 0)),
    }?;

    let size = object.borrow().file.data.len();
    Ok(SMOSReply::ObjStat {
        data: smos_common::returns::ObjStat { size: size },
    })
}

fn handle_obj_open(
    client: &mut Client,
    handle_cap_table: &mut HandleCapabilityTable<BFSResource>,
    args: &ObjOpen,
) -> Result<SMOSReply, InvocationError> {
    let mut matched_file = None;

    unsafe {
        for file in files.iter() {
            if file.is_none() {
                continue;
            }

            let file_unwrapped = file.as_ref().unwrap();
            if file_unwrapped.name == args.name {
                matched_file = Some(file_unwrapped)
            }
        }
    }

    /* Couldn't find the file */
    if matched_file.is_none() {
        return Err(InvocationError::InvalidArguments);
    }

    let (idx, handle_ref, cptr) =
        generic_allocate_handle(client, handle_cap_table, args.return_cap)?;

    let object = Rc::new(RefCell::new(Object {
        file: matched_file.unwrap(),
        associated_views: Vec::new(),
    }));

    *handle_ref = Some(ServerHandle::<BFSResource>::new(BFSResource::Object(
        object,
    )));

    let ret = if args.return_cap {
        HandleOrHandleCap::<ObjectHandle>::new_handle_cap(cptr.unwrap())
    } else {
        HandleOrHandleCap::<ObjectHandle>::new_handle(idx)
    };

    Ok(SMOSReply::ObjOpen { hndl: ret })
}

fn handle_obj_close(
    client: &mut Client,
    handle_cap_table: &mut HandleCapabilityTable<BFSResource>,
    args: &ObjClose,
) -> Result<SMOSReply, InvocationError> {
    let obj_ref = generic_get_handle(client, handle_cap_table, args.hndl, 0)?;

    let object = match obj_ref.as_ref().unwrap().inner() {
        BFSResource::Object(obj) => Ok(obj.clone()),
        _ => Err(generic_invalid_handle_error(args.hndl, 0)),
    }?;

    if !object.borrow().associated_views.is_empty() {
        sel4::debug_println!("Some views are still around");
        todo!();
    }

    generic_cleanup_handle(client, handle_cap_table, args.hndl, 0);

    return Ok(SMOSReply::ObjClose);
}

fn handle_view(
    rs_conn: &RootServerConnection,
    publish_hdnl: &LocalHandle<ConnectionHandle>,
    handle_cap_table: &mut HandleCapabilityTable<BFSResource>,
    client: &mut Client,
    args: &View,
) -> Result<SMOSReply, InvocationError> {
    /* This should always be a wrapped handle cap (for a server that isn't the root-server ) */
    // @alwin: I wonder if it's worth having two versions of the invocation unwrapping stubs,
    // one for the root-server and one for non-root servers?
    let window: smos_common::local_handle::HandleCap<WindowHandle> = args
        .window
        .try_into()
        .or(Err(InvocationError::InvalidArguments))?;

    let obj_handle = generic_get_handle(client, handle_cap_table, args.object, 1)?;

    let object = match obj_handle.as_ref().unwrap().inner() {
        BFSResource::Object(obj) => Ok(obj.clone()),
        _ => Err(InvocationError::InvalidArguments),
    }?;

    let client_id = client.id;
    let (idx, handle_ref) = client.allocate_handle()?;

    let reg_hndl = rs_conn.window_register(publish_hdnl, &window, client_id, idx)?;

    let view = Rc::new(RefCell::new(ViewData {
        object: object.clone(),
        size: args.size,
        win_offset: args.window_offset,
        obj_offset: args.obj_offset,
        rights: args.rights.clone(),
        registration_hndl: reg_hndl,
    }));

    object.borrow_mut().associated_views.push(view.clone());

    *handle_ref = Some(ServerHandle::<BFSResource>::new(BFSResource::View(view)));

    return Ok(SMOSReply::View {
        hndl: LocalHandle::new(idx),
    });
}

fn handle_unview(
    rs_conn: &RootServerConnection,
    client: &mut Client,
    args: &Unview,
) -> Result<SMOSReply, InvocationError> {
    let view_ref = client
        .get_handle_mut(args.hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;

    let view = match view_ref.as_ref().unwrap().inner() {
        BFSResource::View(x) => Ok(x.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    /* Deregister from the window */
    rs_conn.window_deregister(view.borrow().registration_hndl);

    /* Remove reference to view from object*/
    let pos = view
        .borrow_mut()
        .object
        .borrow_mut()
        .associated_views
        .iter()
        .position(|x| Rc::ptr_eq(x, &view))
        .unwrap();
    view.borrow_mut()
        .object
        .borrow_mut()
        .associated_views
        .swap_remove(pos);

    *view_ref = None;

    return Ok(SMOSReply::Unview);
}

fn handle_conn_open(
    rs_conn: &RootServerConnection,
    publish_hdnl: &LocalHandle<ConnectionHandle>,
    window_allocator: &mut Allocator,
    id: usize,
    args: ConnOpen,
) -> Result<SMOSReply, InvocationError> {
    let slot = find_client_slot().ok_or(InvocationError::InsufficientResources)?;

    let shared_buffer: Option<(
        *mut u8,
        usize,
        HandleOrHandleCap<WindowHandle>,
        LocalHandle<ViewHandle>,
        Allocation<u32>,
    )>;
    if let Some(sb) = args.shared_buf_obj {
        // This server arbitrarily only supports windows of size 4K
        if sb.1 != 4096 {
            return Err(InvocationError::AlignmentError { which_arg: 0 });
        }

        /* Create a window of the 4KB size*/
        let window_index = window_allocator
            .allocate(1)
            .ok_or(InvocationError::InsufficientResources)?;
        let window_address = SHARED_BUFFER_BASE as usize + (window_index.offset as usize * 4096);

        /* Create a window for the shared buffer */
        let window_hndl = rs_conn.window_create(window_address, sb.1, None)?;

        /* Create a view for the shared buffer*/
        let view_hndl = rs_conn.view(
            &window_hndl,
            &sb.0.try_into().or(Err(InvocationError::InvalidArguments))?,
            0,
            0,
            4096,
            sel4::CapRights::all(),
        )?;

        shared_buffer = Some((
            window_address as *mut u8,
            sb.1,
            window_hndl,
            view_hndl,
            window_index,
        ));
    } else {
        shared_buffer = None;
    }

    let registration_handle = rs_conn
        .conn_register(publish_hdnl, id)
        .expect("@alwin: can this be an assert?");
    const HANDLE_REPEAT_VALUE: Option<ServerHandle<BFSResource>> = None;
    const VIEW_REPEAT_VALUE: Option<Rc<RefCell<ViewData>>> = None;

    *slot = Some(Client {
        id: id,
        shared_buffer: shared_buffer,
        handles: [HANDLE_REPEAT_VALUE; MAX_HANDLES],
        conn_registration_hndl: registration_handle,
    });

    return Ok(SMOSReply::ConnOpen);
}

fn handle_conn_close(
    rs_conn: &RootServerConnection,
    window_allocator: &mut Allocator,
    client: &mut Client,
) -> Result<SMOSReply, InvocationError> {
    for handle in client.handle_table() {
        if handle.is_some() {
            sel4::debug_println!("Not all handles have been cleaned up!");
            // @alwin: Should probably do some kind of cleanup here
        }
    }

    if client.shared_buffer.is_some() {
        rs_conn
            .unview(client.shared_buffer.as_ref().unwrap().3.clone())
            .expect("Failed to unview");
        rs_conn
            .window_destroy(client.shared_buffer.as_ref().unwrap().2.clone())
            .expect("Failed to destroy window");
        window_allocator.free(client.shared_buffer.as_ref().unwrap().4);
    }

    rs_conn.conn_deregister(&client.conn_registration_hndl);

    delete_client(client);

    return Ok(SMOSReply::ConnClose);
}

fn handle_vm_fault(rs_conn: &RootServerConnection, args: VMFaultNotification) {
    let client = find_client_from_id(args.client_id).expect("BFS corruption: Invalid client ID");

    let view_ref = client
        .as_ref()
        .unwrap()
        .get_handle(args.reference)
        .expect("BFS corruption: Invalid handle from vm fault");
    let view = match view_ref.as_ref().unwrap().inner() {
        BFSResource::View(data) => data.clone(),
        _ => panic!("BFS corruption: Invalid handle type from vm fault"),
    };

    if args.fault_offset < view.borrow().win_offset {
        /* The case where the fault occurs before the view */
        todo!()
    } else if (args.fault_offset > view.borrow().win_offset + view.borrow().size) {
        /* The case where the fault occurs after the view*/
        todo!()
    }

    /* How far into the view the fault occurs */
    let pos = args.fault_offset - view.borrow().win_offset;

    rs_conn.page_map(
        &view.borrow().registration_hndl,
        args.fault_offset,
        view.borrow()
            .object
            .borrow()
            .file
            .data
            .as_ptr()
            .wrapping_add(pos + view.borrow().obj_offset),
    );
}

fn handle_conn_destroy_ntfn(
    rs_conn: &RootServerConnection,
    window_allocator: &mut Allocator,
    args: ConnDestroyNotification,
) {
    let client = find_client_from_id(args.conn_id)
        .expect("BFS corruption: Invalid client ID")
        .as_mut()
        .unwrap();

    for handle in client.handle_table() {
        if handle.is_some() {
            // @alwin: Should probably do some kind of cleanup here
            sel4::debug_println!("Not all handles have been cleaned up!");
            todo!();
        }
    }

    if client.shared_buffer.is_some() {
        rs_conn
            .unview(client.shared_buffer.as_ref().unwrap().3.clone())
            .expect("Failed to unview");
        rs_conn
            .window_destroy(client.shared_buffer.as_ref().unwrap().2.clone())
            .expect("Failed to destroy window");
        window_allocator.free(client.shared_buffer.as_ref().unwrap().4);
    }

    delete_client(client);
}

fn handle_win_destroy_ntfn(rs_conn: &RootServerConnection, args: WindowDestroyNotification) {
    let client = find_client_from_id(args.client_id).expect("BFS corruption: Invalid client ID");

    let view_ref = client
        .as_mut()
        .unwrap()
        .get_handle_mut(args.reference)
        .expect("BFS corruption: Invalid handle from vm fault");
    let view = match view_ref.as_ref().unwrap().inner() {
        BFSResource::View(data) => data.clone(),
        _ => panic!("BFS corruption: Invalid handle type from vm fault"),
    };

    /* Remove reference to the view from object*/
    let pos = view
        .borrow_mut()
        .object
        .borrow_mut()
        .associated_views
        .iter()
        .position(|x| Rc::ptr_eq(x, &view))
        .unwrap();
    view.borrow_mut()
        .object
        .borrow_mut()
        .associated_views
        .swap_remove(pos);

    *view_ref = None;
}

fn handle_notification(rs_conn: &RootServerConnection, window_allocator: &mut Allocator) {
    while let Some(msg) = unsafe { dequeue_ntfn_buffer_msg(ntfn_buffer) } {
        match msg {
            NotificationType::VMFaultNotification(data) => handle_vm_fault(rs_conn, data),
            NotificationType::ConnDestroyNotification(data) => {
                handle_conn_destroy_ntfn(rs_conn, window_allocator, data)
            }
            NotificationType::WindowDestroyNotification(data) => {
                handle_win_destroy_ntfn(rs_conn, data)
            }
        }
    }
}

fn syscall_loop<T: ServerConnection>(
    rs_conn: RootServerConnection,
    mut cspace: SMOSUserCSpace,
    listen_conn: T,
    reply: ReplyWrapper,
) {
    let mut handle_cap_table = init_handle_cap_table(&mut cspace, &rs_conn, &listen_conn);
    let mut reply_msg_info = None;
    let mut recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let mut recv_slot = cspace.to_absolute_cptr(recv_slot_inner);
    sel4::with_ipc_buffer_mut(|ipc_buf| {
        ipc_buf.set_recv_slot(&recv_slot);
    });
    /* Used to allocate regions of the virtual address space for the windows used for mapping
    data buffers with clients  */
    let mut window_allocator = Allocator::with_max_allocs(MAX_CLIENTS as u32, MAX_CLIENTS as u32);

    loop {
        let (msg, mut badge) = if reply_msg_info.is_some() {
            listen_conn
                .ep()
                .reply_recv(reply_msg_info.unwrap(), reply.cap)
        } else {
            listen_conn.ep().recv(reply.cap)
        };

        match decode_entry_type(badge.try_into().unwrap()) {
            EntryType::Notification(bits) => {
            EntryType::Signal => {
                for bit in bits.into_iter() {
                    match bit {
                        0 => handle_notification(&rs_conn, &mut window_allocator),
                        _ => panic!("Don't know how to handle any other notifications {}", badge),
                    }
                }
                reply_msg_info = None;
            }
            EntryType::Fault(_x) => todo!(),
            EntryType::Invocation(id) => {
                let client = find_client_from_id(id);

                /* Extract the shared buffer if the client has already opened the connection and
                provided one */
                let shared_buf = match client {
                    None => None,
                    Some(ref x) => {
                        match &x.as_ref().unwrap().shared_buffer {
                            None => None,
                            Some(raw_buf) => {
                                // Safety: This will only have been set if a conn_open successfully
                                // validated the object passed into conn_open and was able to map
                                // it into a view, which is what this address will reference
                                unsafe { Some(core::slice::from_raw_parts(raw_buf.0, raw_buf.1)) }
                            }
                        }
                    }
                };

                let (invocation, consumed_cap) = sel4::with_ipc_buffer(|buf| {
                    SMOS_Invocation::new::<ObjectServerConnection>(buf, &msg, shared_buf, recv_slot)
                });

                /* Deal with the case where an invalid invocation was done*/
                if invocation.is_err() {
                    if consumed_cap {
                        recv_slot.delete();
                    }
                    reply_msg_info = Some(sel4::with_ipc_buffer_mut(|buf| {
                        handle_error(buf, invocation.unwrap_err())
                    }));
                    continue;
                }

                if client.is_none() && !matches!(invocation, Ok(SMOS_Invocation::ConnOpen(ref t))) {
                    // If the client invokes the server without opening the connection
                    todo!()
                } else if client.is_some()
                    && matches!(invocation, Ok(SMOS_Invocation::ConnOpen(ref t)))
                {
                    // If the client calls conn_open on an already open connection

                    /* @alwin: Is ths benign? Decide what to do here. Maybe it's ok of they opened
                       without a shared buffer but want one without closing and reopening the
                       connection?
                    */
                    todo!()
                }

                let ret = if matches!(invocation, Ok(SMOS_Invocation::ConnOpen(ref t))) {
                    match invocation.unwrap() {
                        SMOS_Invocation::ConnOpen(t) => handle_conn_open(
                            &rs_conn,
                            listen_conn.hndl(),
                            &mut window_allocator,
                            id,
                            t,
                        ),
                        _ => panic!("No invocations besides conn_open should be handled here"),
                    }
                } else {
                    // Safety: Unwrap is safe since if client was none, it would have been handled
                    // by the conn_open case above.
                    let client_unwrapped = client.unwrap().as_mut().unwrap();

                    match invocation.unwrap() {
                        SMOS_Invocation::ConnOpen(_) => {
                            panic!("conn_open should never be handled here")
                        }
                        SMOS_Invocation::ConnClose => {
                            handle_conn_close(&rs_conn, &mut window_allocator, client_unwrapped)
                        }
                        SMOS_Invocation::ObjOpen(t) => {
                            handle_obj_open(client_unwrapped, &mut handle_cap_table, &t)
                        }
                        SMOS_Invocation::ObjClose(t) => {
                            handle_obj_close(client_unwrapped, &mut handle_cap_table, &t)
                        }
                        SMOS_Invocation::ObjStat(t) => {
                            handle_obj_stat(client_unwrapped, &mut handle_cap_table, &t)
                        }
                        SMOS_Invocation::View(t) => handle_view(
                            &rs_conn,
                            listen_conn.hndl(),
                            &mut handle_cap_table,
                            client_unwrapped,
                            &t,
                        ),
                        SMOS_Invocation::Unview(t) => handle_unview(&rs_conn, client_unwrapped, &t),
                        _ => todo!(),
                    }
                };

                /* We delete any cap that was recieved. If a handler wants to hold onto a cap, it
                is their responsibility to copy it somewhere else */
                if consumed_cap {
                    recv_slot.delete();
                }

                reply_msg_info = match ret {
                    Ok(x) => Some(sel4::with_ipc_buffer_mut(|buf| handle_reply(buf, x))),
                    Err(x) => Some(sel4::with_ipc_buffer_mut(|buf| handle_error(buf, x))),
                }
            }
        }
    }
}

fn init_file_table() {
    unsafe {
        files[0] = Some(File {
            name: "init",
            data: INIT_ELF_CONTENTS,
        });
        files[1] = Some(File {
            name: "eth_driver",
            data: ETH_DRIVER_ELF_CONTENTS,
        });
    }
}

fn init_handle_cap_table<T: ServerConnection>(
    cspace: &mut SMOSUserCSpace,
    rs_conn: &RootServerConnection,
    listen_conn: &T,
) -> HandleCapabilityTable<BFSResource> {
    const MAX_HANDLE_CAPS: usize = 16;
    let mut handle_cap_table_inner: Vec<HandleCapability<BFSResource>> = Vec::new();
    for i in 0..MAX_HANDLE_CAPS {
        let slot = cspace.alloc_slot().expect("Could not get a slot");
        rs_conn
            .server_handle_cap_create(&listen_conn.hndl(), i, cspace.to_absolute_cptr(slot))
            .expect("Failed to create handle capability");
        handle_cap_table_inner.push(HandleCapability {
            handle: None,
            root_cap: Some(cspace.to_absolute_cptr(slot)),
        });
    }

    return HandleCapabilityTable::new(handle_cap_table_inner);
}

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) -> sel4::Result<Never> {
    sel4::debug_println!("Entering boot file server...");

    ElfBytes::<elf::endian::AnyEndian>::minimal_parse(INIT_ELF_CONTENTS)
        .expect("Not a valid ELF file");

    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let listen_connection = rs_conn
        .conn_publish::<ObjectServerConnection>(
            ntfn_buffer,
            &cspace.to_absolute_cptr(ep_cptr),
            "BOOT_FS",
        )
        .expect("Could not publish as boot fs");

    init_file_table();

    sel4::debug_println!("Boot file server published...");

    /* Start the other relavant processes */
    rs_conn
        .process_spawn("init", "BOOT_FS", 252, None)
        .expect("Failed to spawn init");

    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    syscall_loop(rs_conn, cspace, listen_connection, reply);

    unreachable!()
}
