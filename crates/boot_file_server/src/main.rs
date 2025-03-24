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
use smos_common::{
    connection::{ObjectServerConnection, RootServerConnection},
    server_connection::ServerConnection,
};
use smos_cspace::SMOSUserCSpace;
use smos_runtime::smos_declare_main;
use smos_runtime::Never;
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::handle::{
    generic_allocate_handle, generic_cleanup_handle, generic_get_handle,
    generic_invalid_handle_error, HandleAllocater, HandleInner, ServerHandle,
};
use smos_server::handle_capability::{HandleCapability, HandleCapabilityTable};
use smos_server::reply::*;
use smos_server::syscalls::*;
extern crate alloc;
use alloc::rc::Rc;
use alloc::vec::Vec;
use offset_allocator::{Allocation, Allocator};
use smos_server::event::{smos_serv_cleanup, smos_serv_decode_invocation, smos_serv_replyrecv};
use smos_server::ntfn_buffer::*;

// @alwin: Should really make sure nothing else starts on the same page as each ELF finishes
// to prevent information leakage. Maybe if another page aligned 'guard page' is added? Kinda
// relying on the behaviour of the compiler

const NUM_FILES: usize = 10;
const INIT_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("INIT_ELF"));
const ETH_DRIVER_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("ETH_DRIVER_ELF"));
const ETH_VIRT_RX_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("ETH_VIRT_RX_ELF"));
const ETH_VIRT_TX_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("ETH_VIRT_TX_ELF"));
const ETH_COPIER_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("ETH_COPIER_ELF"));
const ECHO_SERVER_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("ECHO_SERVER_ELF"));
const TIMER_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("TIMER_ELF"));
const SERIAL_DRIVER_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("SERIAL_DRIVER_ELF"));
const SERIAL_VIRT_RX_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("SERIAL_VIRT_RX_ELF"));
const SERIAL_VIRT_TX_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("SERIAL_VIRT_TX_ELF"));

#[derive(Debug, Copy, Clone)]
struct File {
    name: &'static str,
    data: &'static [u8],
}

static mut FILES: [Option<File>; NUM_FILES] = [None; NUM_FILES];

const NTFN_BUFFER: *mut u8 = 0xB0000 as *mut u8;
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
    #[allow(dead_code)] // @alwin: Remove once this is used to tear stuff down
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
static mut CLIENTS: [Option<Client>; MAX_CLIENTS] = [ARRAY_REPEAT_VALUE; MAX_CLIENTS];

fn find_client_from_id(id: usize) -> Option<&'static mut Option<Client>> {
    unsafe {
        for client in CLIENTS.iter_mut() {
            if client.as_ref().is_some() && client.as_ref().unwrap().id == id {
                return Some(client);
            }
        }
    }

    return None;
}

fn delete_client(to_del: &mut Client) -> Result<(), ()> {
    unsafe {
        for client in CLIENTS.iter_mut() {
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
        for client in CLIENTS.iter_mut() {
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
        data: smos_common::returns::ObjStat {
            size: size,
            paddr: None,
        },
    })
}

fn handle_obj_open(
    client: &mut Client,
    handle_cap_table: &mut HandleCapabilityTable<BFSResource>,
    args: &ObjOpen,
) -> Result<SMOSReply, InvocationError> {
    let mut matched_file = None;

    unsafe {
        for file in FILES.iter() {
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

    generic_cleanup_handle(client, handle_cap_table, args.hndl, 0)
        .expect("Failed to clean up handle");

    return Ok(SMOSReply::ObjClose);
}

fn handle_view(
    rs_conn: &RootServerConnection,
    publish_hndl: &LocalHandle<ConnectionHandle>,
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

    let reg_hndl = rs_conn.window_register(publish_hndl, &window, client_id, idx)?;

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
    rs_conn
        .window_deregister(view.borrow().registration_hndl)
        .expect("Failed to deregister from window");

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
    cspace: &mut SMOSUserCSpace,
    publish_hndl: &LocalHandle<ConnectionHandle>,
    window_allocator: &mut Allocator,
    id: usize,
    args: &ConnOpen,
) -> Result<SMOSReply, InvocationError> {
    let slot = find_client_slot().ok_or(InvocationError::InsufficientResources)?;

    let shared_buffer: Option<(
        *mut u8,
        usize,
        HandleOrHandleCap<WindowHandle>,
        LocalHandle<ViewHandle>,
        Allocation<u32>,
    )>;

    let registration_hndl = rs_conn.conn_register(publish_hndl, id)?;

    if let Some(sb) = args.shared_buf_obj {
        // This server arbitrarily only supports windows of size 4K
        if sb.1 != 4096 {
            rs_conn
                .conn_deregister(&registration_hndl)
                .expect("Failed to deregister from connection");
            return Err(InvocationError::AlignmentError { which_arg: 0 });
        }

        /* Create a window of the 4KB size*/
        let window_index = window_allocator.allocate(1).ok_or_else(|| {
            rs_conn
                .conn_deregister(&registration_hndl)
                .expect("Failed to deregister from connection");
            InvocationError::InsufficientResources
        })?;
        let window_address = SHARED_BUFFER_BASE as usize + (window_index.offset as usize * 4096);

        /* Create a window for the shared buffer */
        let window_hndl = rs_conn
            .window_create(window_address, sb.1, None)
            .map_err(|e| {
                window_allocator.free(window_index);
                rs_conn
                    .conn_deregister(&registration_hndl)
                    .expect("Failed to deregister from connection");
                e
            })?;

        /* Create a view for the shared buffer*/
        let view_hndl = rs_conn
            .view(
                &window_hndl,
                &sb.0.try_into().or(Err(InvocationError::InvalidArguments))?,
                0,
                0,
                4096,
                sel4::CapRights::all(),
            )
            .map_err(|e| {
                rs_conn
                    .window_destroy(window_hndl, cspace)
                    .expect("Failed to destroy window");
                window_allocator.free(window_index);
                rs_conn
                    .conn_deregister(&registration_hndl)
                    .expect("Failed to deregister from connection");
                e
            })?;

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

    const HANDLE_REPEAT_VALUE: Option<ServerHandle<BFSResource>> = None;

    *slot = Some(Client {
        id: id,
        shared_buffer: shared_buffer,
        handles: [HANDLE_REPEAT_VALUE; MAX_HANDLES],
        conn_registration_hndl: registration_hndl,
    });

    return Ok(SMOSReply::ConnOpen);
}

fn handle_conn_close(
    rs_conn: &RootServerConnection,
    window_allocator: &mut Allocator,
    client: &mut Client,
    cspace: &mut SMOSUserCSpace,
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
            .window_destroy(client.shared_buffer.as_ref().unwrap().2.clone(), cspace)
            .expect("Failed to destroy window");
        window_allocator.free(client.shared_buffer.as_ref().unwrap().4);
    }

    rs_conn
        .conn_deregister(&client.conn_registration_hndl)
        .expect("Failed to deregister from connection");

    delete_client(client).expect("Failed to delete client");

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
        // @alwin: Should probably add an invocation that tells the RS that this fault couldn't be
        // handled. Not sure what the correct recovery mechanism is here? Maybe something like a
        // SIGBUS?
        todo!()
    } else if args.fault_offset > view.borrow().win_offset + view.borrow().size {
        /* The case where the fault occurs after the view*/
        todo!()
    }

    /* How far into the view the fault occurs */
    let pos = args.fault_offset - view.borrow().win_offset;

    rs_conn
        .page_map(
            &view.borrow().registration_hndl,
            args.fault_offset,
            view.borrow()
                .object
                .borrow()
                .file
                .data
                .as_ptr()
                .wrapping_add(pos + view.borrow().obj_offset),
        )
        .expect("Failed to map page in VM fault handler");
}

fn handle_conn_destroy_ntfn(
    rs_conn: &RootServerConnection,
    window_allocator: &mut Allocator,
    args: ConnDestroyNotification,
    cspace: &mut SMOSUserCSpace,
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
            .window_destroy(client.shared_buffer.as_ref().unwrap().2.clone(), cspace)
            .expect("Failed to destroy window");
        window_allocator.free(client.shared_buffer.as_ref().unwrap().4);
    }

    delete_client(client).expect("Failed to delete client");
}

fn handle_win_destroy_ntfn(args: WindowDestroyNotification) {
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

fn handle_notification(
    rs_conn: &RootServerConnection,
    window_allocator: &mut Allocator,
    cspace: &mut SMOSUserCSpace,
) {
    while let Some(msg) = unsafe { dequeue_ntfn_buffer_msg(NTFN_BUFFER) } {
        match msg {
            NotificationType::VMFaultNotification(data) => handle_vm_fault(rs_conn, data),
            NotificationType::ConnDestroyNotification(data) => {
                handle_conn_destroy_ntfn(rs_conn, window_allocator, data, cspace)
            }
            NotificationType::WindowDestroyNotification(data) => handle_win_destroy_ntfn(data),
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
    let recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let recv_slot = cspace.to_absolute_cptr(recv_slot_inner);
    sel4::with_ipc_buffer_mut(|ipc_buf| {
        ipc_buf.set_recv_slot(&recv_slot);
    });
    /* Used to allocate regions of the virtual address space for the windows used for mapping
    data buffers with clients  */
    let mut window_allocator = Allocator::with_max_allocs(MAX_CLIENTS as u32, MAX_CLIENTS as u32);

    loop {
        let (msg, badge) = smos_serv_replyrecv(&listen_conn, &reply, reply_msg_info);

        match decode_entry_type(badge.try_into().unwrap()) {
            EntryType::Notification(bits) => {
                for bit in bits.into_iter() {
                    match bit {
                        0 => handle_notification(&rs_conn, &mut window_allocator, &mut cspace),
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

                let invocation = smos_serv_decode_invocation::<ObjectServerConnection>(
                    &msg, recv_slot, shared_buf,
                );
                if let Err(e) = invocation {
                    reply_msg_info = e;
                    continue;
                }

                if client.is_none() && !matches!(invocation, Ok(SMOS_Invocation::ConnOpen(_))) {
                    // If the client invokes the server without opening the connection
                    todo!()
                } else if client.is_some() && matches!(invocation, Ok(SMOS_Invocation::ConnOpen(_)))
                {
                    // If the client calls conn_open on an already open connection

                    /* @alwin: Is ths benign? Decide what to do here. Maybe it's ok of they opened
                       without a shared buffer but want one without closing and reopening the
                       connection?
                    */
                    todo!()
                }

                let ret = if matches!(invocation, Ok(SMOS_Invocation::ConnOpen(_))) {
                    match invocation.as_ref().unwrap() {
                        SMOS_Invocation::ConnOpen(t) => handle_conn_open(
                            &rs_conn,
                            &mut cspace,
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

                    match invocation.as_ref().unwrap() {
                        SMOS_Invocation::ConnOpen(_) => {
                            panic!("conn_open should never be handled here")
                        }
                        SMOS_Invocation::ConnClose => handle_conn_close(
                            &rs_conn,
                            &mut window_allocator,
                            client_unwrapped,
                            &mut cspace,
                        ),
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
                reply_msg_info = smos_serv_cleanup(invocation.unwrap(), recv_slot, ret);
            }
        }
    }
}

fn init_file_table() {
    unsafe {
        FILES[0] = Some(File {
            name: "init",
            data: INIT_ELF_CONTENTS,
        });
        FILES[1] = Some(File {
            name: "eth_driver",
            data: ETH_DRIVER_ELF_CONTENTS,
        });
        FILES[2] = Some(File {
            name: "eth_virt_rx",
            data: ETH_VIRT_RX_ELF_CONTENTS,
        });
        FILES[3] = Some(File {
            name: "eth_virt_tx",
            data: ETH_VIRT_TX_ELF_CONTENTS,
        });
        FILES[4] = Some(File {
            name: "eth_copier",
            data: ETH_COPIER_ELF_CONTENTS,
        });
        FILES[5] = Some(File {
            name: "echo_server",
            data: ECHO_SERVER_ELF_CONTENTS,
        });
        FILES[6] = Some(File {
            name: "timer",
            data: TIMER_ELF_CONTENTS,
        });
        FILES[7] = Some(File {
            name: "serial_driver",
            data: SERIAL_DRIVER_ELF_CONTENTS,
        });
        FILES[8] = Some(File {
            name: "serial_virt_rx",
            data: SERIAL_VIRT_RX_ELF_CONTENTS,
        });
        FILES[9] = Some(File {
            name: "serial_virt_tx",
            data: SERIAL_VIRT_TX_ELF_CONTENTS,
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
            NTFN_BUFFER,
            &cspace.to_absolute_cptr(ep_cptr),
            "BOOT_FS",
        )
        .expect("Could not publish as boot fs");

    init_file_table();

    sel4::debug_println!("Boot file server published...");

    /* Start the other relavant processes */
    rs_conn
        .process_spawn("init", "BOOT_FS", 250, None)
        .expect("Failed to spawn init");

    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn
        .reply_create(cspace.to_absolute_cptr(reply_cptr))
        .expect("Could not create reply object");

    syscall_loop(rs_conn, cspace, listen_connection, reply);

    unreachable!()
}
