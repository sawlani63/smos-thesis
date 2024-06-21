#![no_std]
#![no_main]
#![feature(const_refs_to_static)]

use smos_runtime::smos_declare_main;
use smos_common::{connection::{RootServerConnection, ObjectServerConnection}, server_connection::{ServerConnection}};
use smos_common::syscall::{RootServerInterface, ObjectServerInterface, ReplyWrapper};
use include_bytes_aligned::include_bytes_aligned;
use smos_cspace::SMOSUserCSpace;
use smos_runtime::Never;
use elf::ElfBytes;
use smos_server::syscalls::{*};
use smos_server::reply::{*};
use smos_common::error::{*};
use smos_common::util::ROUND_UP;
use smos_server::error::{*};
use smos_server::event::{decode_entry_type, EntryType};
use smos_server::handle_arg::ServerReceivedHandleOrHandleCap;
use smos_server::handle::{HandleInner, ServerHandle, HandleAllocater};
use smos_common::local_handle::{HandleOrHandleCap, WindowHandle, ObjectHandle, ViewHandle,
                                LocalHandle, WindowRegistrationHandle, ConnectionHandle,
                                ConnRegistrationHandle};
use core::cell::RefCell;
extern crate alloc;
use alloc::rc::Rc;
use alloc::vec::Vec;
use smos_server::ntfn_buffer::{*};
use core::ptr::NonNull;


// @alwin: Should really make sure nothing else starts on the same page as this ELF to prevent
// information leakage. Maybe if another page aligned 'guard page' is added? Kinda relying on the
// behaviour of the compiler

const NUM_FILES: usize = 1;
const TEST_ELF_CONTENTS: &[u8] = include_bytes_aligned!(4096, env!("TEST_ELF"));

#[derive(Debug)]
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
    associated_views: Vec<Rc<RefCell<ViewData>>>
}

struct ViewData {
    object: Rc<RefCell<Object>>,
    size: usize,
    win_offset: usize,
    obj_offset: usize,
    rights: sel4::CapRights,
    registration_hndl: LocalHandle<WindowRegistrationHandle>
}

enum BFSResource {
    Object(Rc<RefCell<Object>>),
    View(Rc<RefCell<ViewData>>)
}

impl HandleInner for BFSResource {}

struct Client {
    id: usize,
    shared_buffer: Option<(*mut u8, usize, HandleOrHandleCap<WindowHandle>, LocalHandle<ViewHandle>)>,
    handles: [Option<ServerHandle<BFSResource>>; MAX_HANDLES],
    conn_registration_hndl: LocalHandle<ConnRegistrationHandle>
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
                return Some(client)
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
                return Some(client)
            }
        }
    }

    return None;
}

fn handle_obj_stat(client: &mut Client, args: &ObjStat) -> Result<SMOSReply, InvocationError> {

    let hndl = match args.hndl {
        ServerReceivedHandleOrHandleCap::Handle(x) => {
            client.get_handle(x.idx).or(Err(InvocationError::InvalidArguments))?
        },
        ServerReceivedHandleOrHandleCap::UnwrappedHandleCap(x) => {
            /* @alwin: Figure out how to deal with handle caps */
            todo!()
        }
        _ => panic!("Should never recieve a wrapped handle cap. @alwin: Maybe make a new type ServerRecievedLocalHandleOrUnwrappedHandleCap for this kind of case"),
    };

    let object = match hndl.as_ref().unwrap().inner() {
        BFSResource::Object(obj) => obj.clone(),
        _ => todo!() //@alwin: Deal with when wrong kind of handle is passed in
    };

    let size = object.borrow().file.data.len();
    Ok(SMOSReply::ObjStat {
        data: smos_common::returns::ObjStat {
            size: size,
        }
    })
}

fn handle_obj_open(client: &mut Client, args: &ObjOpen) -> Result<SMOSReply, InvocationError> {
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

    let (idx, handle_ref) = client.allocate_handle()?;

    let object = Rc::new(RefCell::new(
            Object {
                file: matched_file.unwrap(),
                associated_views: Vec::new()
            }
    ));

    *handle_ref = Some(ServerHandle::<BFSResource>::new(BFSResource::Object(object)));

    // @alwin: Deal with handle caps
    Ok(SMOSReply::ObjOpen {
        hndl: HandleOrHandleCap::new_handle(idx)
    })
}

fn handle_obj_close(client: &mut Client, args: &ObjClose) -> Result<SMOSReply, InvocationError> {
    let obj_ref = match args.hndl {
        ServerReceivedHandleOrHandleCap::Handle(x) => {
            client.get_handle_mut(x.idx).or(Err(InvocationError::InvalidArguments))
        },
        ServerReceivedHandleOrHandleCap::UnwrappedHandleCap(x) => {
            /* @alwin: Figure out how to deal with handle caps */
            todo!()
        },
        _ => panic!("Should never recieve a wrapped handle cap. @alwin: Maybe make a new type ServerRecievedLocalHandleOrUnwrappedHandleCap for this kind of case"),
    }?;

    let object = match obj_ref.as_ref().unwrap().inner() {
        BFSResource::Object(obj) => Ok(obj.clone()),
        _ => Err(InvocationError::InvalidArguments)
    }?;

    if !object.borrow().associated_views.is_empty() {
        sel4::debug_println!("Some views are still around");
        todo!();
    }

    *obj_ref = None;

    return Ok(SMOSReply::ObjClose);
}

fn handle_view(rs_conn: &RootServerConnection, publish_hdnl: &LocalHandle<ConnectionHandle>,
               client: &mut Client, args: &View) -> Result<SMOSReply, InvocationError> {

    /* This should always be a wrapped handle cap (for a server that isn't the root-server ) */
    // @alwin: I wonder if it's worth having two versions of the invocation unwrapping stubs,
    // one for the root-server and one for non-root servers?
    let window: smos_common::local_handle::HandleCap<WindowHandle> = args.window.try_into()
                                                                                .or(Err(InvocationError::InvalidArguments))?;

    let obj_handle = match args.object {
        ServerReceivedHandleOrHandleCap::Handle(x) => {
            client.get_handle(x.idx).or(Err(InvocationError::InvalidArguments))?.as_ref().unwrap()
        },
        ServerReceivedHandleOrHandleCap::UnwrappedHandleCap(x) => {
            /* @alwin: Figure out how to deal with handle caps */
            todo!()
        }
        _ => panic!("Should never recieve a wrapped handle cap. @alwin: Maybe make a new type ServerRecievedLocalHandleOrUnwrappedHandleCap for this kind of case"),
    };

    let object = match obj_handle.inner() {
        BFSResource::Object(obj) => Ok(obj.clone()),
        _ => Err(InvocationError::InvalidArguments)
    }?;

    let client_id = client.id;
    let (idx, handle_ref) = client.allocate_handle()?;

    let reg_hndl = rs_conn.window_register(publish_hdnl, &window, client_id, idx)?;

    let view = Rc::new( RefCell::new(
        ViewData {
            object: object.clone(),
            size: args.size,
            win_offset: args.window_offset,
            obj_offset: args.obj_offset,
            rights: args.rights.clone(),
            registration_hndl: reg_hndl
        }
    ));

    // @alwin: Deal with proper error handling and cleanup

    object.borrow_mut().associated_views.push(view.clone());

    *handle_ref = Some(ServerHandle::<BFSResource>::new(BFSResource::View(view)));

    return Ok(SMOSReply::View {hndl: LocalHandle::new(idx)})
}

fn handle_unview(rs_conn: &RootServerConnection, client: &mut Client, args: &Unview) -> Result<SMOSReply, InvocationError> {
    let view_ref = client.get_handle_mut(args.hndl.idx).or(Err(InvocationError::InvalidHandle {which_arg: 0}))?;

    let view = match view_ref.as_ref().unwrap().inner() {
        BFSResource::View(x) => Ok(x.clone()),
        _ => Err(InvocationError::InvalidHandle {which_arg: 0})
    }?;

    /* Deregister from the window */
    rs_conn.window_deregister(view.borrow().registration_hndl);

    /* Remove reference to view from object*/
    let pos = view.borrow_mut().object.borrow_mut().associated_views.iter().position(|x| Rc::ptr_eq(x, &view)).unwrap();
    view.borrow_mut().object.borrow_mut().associated_views.swap_remove(pos);

    *view_ref = None;

    return Ok(SMOSReply::Unview);
}

fn handle_conn_open(rs_conn: &RootServerConnection, publish_hdnl: &LocalHandle<ConnectionHandle>,
                    id: usize, args: ConnOpen) -> Result<SMOSReply, InvocationError> {
    let slot = find_client_slot().ok_or(InvocationError::InsufficientResources)?;

    let shared_buffer: Option<(*mut u8, usize, HandleOrHandleCap<WindowHandle>, LocalHandle<ViewHandle>)>;
    if let Some(sb) = args.shared_buf_obj {
        // @alwin: This server arbitrarily only supports windows of size 4K
        if sb.1 % 4096 != 0 {
            return Err(InvocationError::AlignmentError{ which_arg: 0});
        }

        if sb.1 > 4096 {
            return Err(InvocationError::InvalidArguments);
        }

        /* Create a window for the shared buffer */
        // @alwin: Figure out a good way to deal with multiple clients
        // I think something that might work cleanly is some kind of heap allocator that doesn't keep
        // metadata inside free blocks and doesn't read/write to the actual memory being allocated.
        // This will essentially be a window allocator inside the virtual address space. Maybe we have
        // one big one for the whole process, or multiple small ones for different kinds of windows
        let window_hndl = rs_conn.window_create(SHARED_BUFFER_BASE as usize, sb.1, None)?;

        /* Create a view for the shared buffer*/
        let view_hndl = rs_conn.view(&window_hndl, &sb.0.try_into().or(Err(InvocationError::InvalidArguments))?, 0, 0, 4096, sel4::CapRights::all())?;

        shared_buffer = Some((SHARED_BUFFER_BASE, sb.1, window_hndl, view_hndl));
    } else {
        shared_buffer = None;
    }

    let registration_handle = rs_conn.conn_register(publish_hdnl, id).expect("@alwin: CAn this be an assert?");
    const HANDLE_REPEAT_VALUE: Option<ServerHandle<BFSResource>> = None;
    const VIEW_REPEAT_VALUE: Option<Rc<RefCell<ViewData>>> = None;

    *slot = Some (
        Client {
            id: id,
            shared_buffer: shared_buffer,
            handles: [HANDLE_REPEAT_VALUE; MAX_HANDLES],
            conn_registration_hndl: registration_handle
        }
    );

    return Ok(SMOSReply::ConnOpen);
}

fn handle_conn_close(rs_conn: &RootServerConnection, client: &mut Client) -> Result<SMOSReply, InvocationError> {
    for handle in client.handle_table() {
        if handle.is_some() {
            sel4::debug_println!("Not all handles have been cleaned up!");
            // @alwin: Should probably do some kind of cleanup here
        }
    }

    if client.shared_buffer.is_some() {
        rs_conn.unview(client.shared_buffer.as_ref().unwrap().3.clone()).expect("Failed to unview");
        rs_conn.window_destroy(client.shared_buffer.as_ref().unwrap().2.clone()).expect("Failed to destroy window");
    }

    rs_conn.conn_deregister(&client.conn_registration_hndl);

    delete_client(client);

    return Ok(SMOSReply::ConnClose);
}

fn handle_vm_fault(rs_conn: &RootServerConnection, args: VMFaultNotification) {
    let client = find_client_from_id(args.client_id).expect("BFS corruption: Invalid client ID");

    let view_ref = client.as_ref().unwrap().get_handle(args.reference).expect("BFS corruption: Invalid handle from vm fault");
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

    rs_conn.page_map(&view.borrow().registration_hndl, args.fault_offset, view.borrow().object.borrow().file.data.as_ptr().wrapping_add(pos + view.borrow().obj_offset));
}

fn handle_conn_destroy_ntfn(rs_conn: &RootServerConnection, args: ConnDestroyNotification) {
    let client = find_client_from_id(args.conn_id).expect("BFS corruption: Invalid client ID").as_mut().unwrap();

    for handle in client.handle_table() {
        if handle.is_some() {
            sel4::debug_println!("Not all handles have been cleaned up!");
            // @alwin: Should probably do some kind of cleanup here
        }
    }

    if client.shared_buffer.is_some() {
        rs_conn.unview(client.shared_buffer.as_ref().unwrap().3.clone()).expect("Failed to unview");
        rs_conn.window_destroy(client.shared_buffer.as_ref().unwrap().2.clone()).expect("Failed to destroy window");
    }

    delete_client(client);
}

fn handle_win_destroy_ntfn(rs_conn: &RootServerConnection, args: WindowDestroyNotification) {
    let client = find_client_from_id(args.client_id).expect("BFS corruption: Invalid client ID");

    let view_ref = client.as_mut().unwrap().get_handle_mut(args.reference).expect("BFS corruption: Invalid handle from vm fault");
    let view = match view_ref.as_ref().unwrap().inner() {
        BFSResource::View(data) => data.clone(),
        _ => panic!("BFS corruption: Invalid handle type from vm fault"),
    };

    /* Remove reference to the view from object*/
    let pos = view.borrow_mut().object.borrow_mut().associated_views.iter().position(|x| Rc::ptr_eq(x, &view)).unwrap();
    view.borrow_mut().object.borrow_mut().associated_views.swap_remove(pos);

    *view_ref = None;
}

fn handle_notification(rs_conn: &RootServerConnection) {
    while let Some(msg) = unsafe { dequeue_ntfn_buffer_msg(ntfn_buffer) } {
        match msg {
            NotificationType::VMFaultNotification(data) => handle_vm_fault(rs_conn, data),
            NotificationType::ConnDestroyNotification(data) => handle_conn_destroy_ntfn(rs_conn, data),
            NotificationType::WindowDestroyNotification(data) => handle_win_destroy_ntfn(rs_conn, data),
        }
    }
}

fn syscall_loop<T: ServerConnection>(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace, listen_conn: T, reply: ReplyWrapper) {

    let mut reply_msg_info = None;

    /* @alwin: What should be responsible for deciding whether to allocate a new slot? I think it
       is least error-prone if it is done by SMOS_Invocation::new(). If the handlers use the recieved
       cap and decide they don't need it anymore, they can just delete and free it and this can just
       be re-used anyway. */

    let mut recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
    let mut recv_slot = cspace.to_absolute_cptr(recv_slot_inner);
    sel4::with_ipc_buffer_mut(|ipc_buf| {
        ipc_buf.set_recv_slot(&recv_slot);
    });

    loop {
        let (msg, mut badge) = if reply_msg_info.is_some() {
            listen_conn.ep().reply_recv(reply_msg_info.unwrap(), reply.cap)
        } else {
            listen_conn.ep().recv(reply.cap)
        };


        match decode_entry_type(badge.try_into().unwrap()) {
            EntryType::Irq => todo!(),
            EntryType::Signal => {
                handle_notification(&rs_conn);
                reply_msg_info = None;
            },
            // EntryType::Ntfn => todo!(),
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
                                unsafe {
                                    Some(core::slice::from_raw_parts(raw_buf.0, raw_buf.1))
                                }
                            }
                        }
                    }
                };

                let invocation = sel4::with_ipc_buffer(|buf| SMOS_Invocation::new::<ObjectServerConnection>(buf, &msg, shared_buf, recv_slot));

                /* Deal with the case where an invalid invocation was done*/
                if invocation.is_err() {
                    reply_msg_info = Some(sel4::with_ipc_buffer_mut(|buf| handle_error(buf, invocation.unwrap_err())));
                    continue;
                }


                if client.is_none() && !matches!(invocation, Ok(SMOS_Invocation::ConnOpen(ref t))) {
                    // If the client invokes the server without opening the connection
                    todo!()
                } else if client.is_some() && matches!(invocation, Ok(SMOS_Invocation::ConnOpen(ref t))) {
                    // If the client calls conn_open on an already open connection

                    /* @alwin: Is ths benign? Decide what to do here. Maybe it's ok of they opened
                       without a shared buffer but want one without closing and reopening the
                       connection?
                    */
                    todo!()
                }

                // @alwin: I think this is rather ugly
                let ret = if matches!(invocation, Ok(SMOS_Invocation::ConnOpen(ref t))) {
                    // @alwin: this is a HACK! Figure out a better way of dealing with reallocation
                    // of recv cap slot.
                    recv_slot_inner = cspace.alloc_slot().expect("Could not allocate slot");
                    recv_slot = cspace.to_absolute_cptr(recv_slot_inner);
                    sel4::with_ipc_buffer_mut(|ipc_buf| {
                        ipc_buf.set_recv_slot(&recv_slot);
                    });

                    match invocation.unwrap() {
                        SMOS_Invocation::ConnOpen(t) => handle_conn_open(&rs_conn, listen_conn.hndl(), id, t),
                        _ => panic!("No invocations besides conn_open should be handled here")
                    }
                } else {
                    // Safety: Unwrap is safe since if client was none, it would have been handled
                    // by the conn_open case above.
                    let client_unwrapped = client.unwrap().as_mut().unwrap();

                    match invocation.unwrap() {
                        SMOS_Invocation::ConnOpen(_) => panic!("conn_open should never be handled here"),
                        SMOS_Invocation::ConnClose => handle_conn_close(&rs_conn, client_unwrapped),
                        SMOS_Invocation::ObjOpen(t) => handle_obj_open(client_unwrapped, &t),
                        SMOS_Invocation::ObjClose(t) => handle_obj_close(client_unwrapped, &t),
                        SMOS_Invocation::ObjStat(t) => handle_obj_stat(client_unwrapped, &t),
                        SMOS_Invocation::View(t) => handle_view(&rs_conn, listen_conn.hndl(), client_unwrapped, &t),
                        SMOS_Invocation::Unview(t) => handle_unview(&rs_conn, client_unwrapped, &t),
                        _ => todo!()
                    }
                };

                reply_msg_info = match ret {
                    Ok(x) => Some(sel4::with_ipc_buffer_mut(|buf| handle_reply(buf, x))),
                    Err(x) => Some(sel4::with_ipc_buffer_mut(|buf| handle_error(buf, x)))
                }
            }
        }
    }
}

fn init_file_table() {
    unsafe {
        files[0] = Some(File {name: "test_app", data: TEST_ELF_CONTENTS});
    }
}

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) -> sel4::Result<Never> {

    sel4::debug_println!("Entering boot file server...");

    ElfBytes::<elf::endian::AnyEndian>::minimal_parse(TEST_ELF_CONTENTS).expect("Not a valid ELF file");

    let ep_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let listen_connection = rs_conn.conn_publish::<ObjectServerConnection>(ntfn_buffer, &cspace.to_absolute_cptr(ep_cptr), "BOOT_FS").expect("Could not publish as boot fs");

    init_file_table();

    sel4::debug_println!("Boot file server published... Waiting on endpoint {:?}...", listen_connection);

    /* Start the other relavant processes */
    rs_conn.process_spawn("test_app", "BOOT_FS");

    /* @alwin: Idk if this is the best way to do this */
    let reply_cptr = cspace.alloc_slot().expect("Could not get a slot");
    let reply = rs_conn.reply_create(cspace.to_absolute_cptr(reply_cptr)).expect("Could not create reply object");

    syscall_loop(rs_conn, cspace, listen_connection, reply);

    unreachable!()
}
