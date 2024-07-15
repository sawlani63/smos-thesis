use crate::args::*;
use crate::client_connection::*;
use crate::connection::*;
use crate::error::*;
use crate::invocations::SMOSInvocation;
use crate::local_handle::{
    ConnRegistrationHandle, ConnectionHandle, HandleCap, HandleCapHandle, HandleOrHandleCap,
    IRQRegistrationHandle, LocalHandle, ObjectHandle, ProcessHandle, PublishHandle, ViewHandle,
    WindowHandle, WindowRegistrationHandle,
};
use crate::obj_attributes::ObjAttributes;
use crate::returns::*;
use crate::server_connection::*;
use crate::string::copy_terminated_rust_string_to_buffer;
use core::marker::PhantomData;
use core::slice;
use sel4::cap::Endpoint;
use sel4::{AbsoluteCPtr, IpcBuffer};
use smos_cspace::SMOSUserCSpace;

/*
 * This is kind of what I want to do:
 *		smos_conn_create() will give us an endpoint capability
 * 		It will also give us some information about what kind of endpoint capability it is
 *		Based on this, we will cast the endpoint capability (maybe using downcast or whatever) to some type
 *		We will have a bunch of traits which correspond to interfaces that are provided by certain servers
 * 		The particular type we cast it to will implement the traits that correspond to that specicic server
 *
 *		An example: The root server acts as an object server and a file server (suppose file servers have special additional invocations)
 *		There will be two traits, ObjectServerTrait (obj_open, obj_view, obj_create) and FileServerTrait (dir_open, dir_close, dir_read)
 *		The endpoint to the root server will have some ObjectFileServer: ObjectServerTrait + FileServerTrait type
 *		Then, you can invoke root_server_ep.obj_open(...) and root_server_ep.dir_open(...) and stuff
 */

// @alwin: Currently, conn_create is implemented in a way that the client knows what they are
// connecting to. In a real dynamic system, it would be better if the client didn't have to
// know this at compile time but idk if this is really THAT useful or even feasible.

pub trait RootServerInterface: ClientConnection {
    fn conn_create<T: ClientConnection>(
        &self,
        slot: &AbsoluteCPtr,
        server_name: &str,
    ) -> Result<T, InvocationError> {
        let (handle, endpoint) = sel4::with_ipc_buffer_mut(|ipc_buf| {
            let shared_buf_raw = self
                .get_buf_mut()
                .ok_or(InvocationError::DataBufferNotSet)?;
            let shared_buf =
                unsafe { slice::from_raw_parts_mut(shared_buf_raw.0, shared_buf_raw.1) };
            copy_terminated_rust_string_to_buffer(shared_buf, server_name)?;

            ipc_buf.set_recv_slot(slot);
            let mut msginfo = sel4::MessageInfoBuilder::default()
                .label(SMOSInvocation::ConnCreate as u64)
                .length(0)
                .build();
            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;
            return Ok((
                ipc_buf.msg_regs()[ConnectionCreateReturn::ConnectionHandle as usize],
                sel4::CPtr::from_bits(slot.path().bits()).cast::<sel4::cap_type::Endpoint>(),
            ));
        })?;

        return Ok(T::new(
            endpoint,
            LocalHandle::<ConnectionHandle>::new(handle.try_into().unwrap()),
            None,
        ));
        // @alwin: Should we have a flag to ensure that a connection is opened prior to being used for anything else?
    }

    fn conn_register(
        &self,
        publish_hndl: &LocalHandle<ConnectionHandle>,
        id: usize,
    ) -> Result<(LocalHandle<ConnRegistrationHandle>), InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::ConnRegister as u64)
            .length(2)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = publish_hndl.idx as u64;
            ipc_buf.msg_regs_mut()[1] = id as u64;

            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            Ok(LocalHandle::new(ipc_buf.msg_regs()[0] as usize))
        });
    }

    fn irq_register(
        &self,
        publish_hndl: &LocalHandle<ConnectionHandle>,
        irq_num: usize,
        edge_triggered: bool,
        irq_handler_slot: &AbsoluteCPtr,
    ) -> Result<
        (
            LocalHandle<IRQRegistrationHandle>,
            u8,
            sel4::cap::IrqHandler,
        ),
        InvocationError,
    > {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::IRQRegister as u64)
            .length(3)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = publish_hndl.idx as u64;
            ipc_buf.msg_regs_mut()[1] = irq_num as u64;
            ipc_buf.msg_regs_mut()[2] = edge_triggered.into();
            ipc_buf.set_recv_slot(irq_handler_slot);

            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;
            Ok((
                LocalHandle::new(ipc_buf.msg_regs()[0] as usize),
                ipc_buf.msg_regs()[1].try_into().unwrap(),
                sel4::CPtr::from_bits(irq_handler_slot.path().bits())
                    .cast::<sel4::cap_type::IrqHandler>(),
            ))
        });
    }

    fn conn_deregister(
        &self,
        reg_hndl: &LocalHandle<ConnRegistrationHandle>,
    ) -> Result<(), InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::ConnDeregister as u64)
            .length(1)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = reg_hndl.idx as u64;

            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;
            Ok(())
        });
    }

    fn conn_publish<T: ServerConnection>(
        &self,
        ntfn_buffer: *mut u8,
        slot: &AbsoluteCPtr,
        name: &str,
    ) -> Result<T, InvocationError> {
        let (handle, endpoint) = sel4::with_ipc_buffer_mut(|ipc_buf| {
            let shared_buf_raw = self
                .get_buf_mut()
                .ok_or(InvocationError::DataBufferNotSet)?;
            let shared_buf =
                unsafe { slice::from_raw_parts_mut(shared_buf_raw.0, shared_buf_raw.1) };
            copy_terminated_rust_string_to_buffer(shared_buf, name)?;

            ipc_buf.set_recv_slot(slot);
            ipc_buf.msg_regs_mut()[0] = ntfn_buffer as u64;
            let mut msginfo = sel4::MessageInfoBuilder::default()
                .label(SMOSInvocation::ConnPublish as u64)
                .length(1)
                .build();
            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;
            return Ok((
                ipc_buf.msg_regs()[0],
                sel4::CPtr::from_bits(slot.path().bits()).cast::<sel4::cap_type::Endpoint>(),
            ));
        })?;

        return Ok(T::new(
            endpoint,
            LocalHandle::<ConnectionHandle>::new(handle.try_into().unwrap()),
            None,
        ));
    }

    fn conn_destroy<T: ClientConnection>(&self, conn: T) -> Result<(), InvocationError> {
        sel4::with_ipc_buffer_mut(|ipc_buf| {
            let mut msginfo = sel4::MessageInfoBuilder::default()
                .label(SMOSInvocation::ConnDestroy as u64)
                .length(1)
                .build();
            // @alwin: Idk if this is better than doing msg_bytes and doing a memcpy of the arg
            // struct. It will probably be faster?
            ipc_buf.msg_regs_mut()[ConnectionDestroyArgs::Handle as usize] =
                conn.hndl().idx.try_into().unwrap();
            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;
            return Ok(());
        })
    }

    fn test_simple(&self, msg: u64) -> Result<(), InvocationError> {
        sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = msg;
            let mut msginfo = sel4::MessageInfoBuilder::default()
                .label(SMOSInvocation::TestSimple as u64)
                .length(1)
                .build();
            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs());
            return Ok(());
        })
    }

    fn window_create(
        &self,
        base_vaddr: usize,
        size: usize,
        return_cap: Option<AbsoluteCPtr>,
    ) -> Result<HandleOrHandleCap<WindowHandle>, InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::WindowCreate as u64)
            .length(3)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            // @alwin: use constants
            ipc_buf.msg_regs_mut()[0] = base_vaddr.try_into().unwrap();
            ipc_buf.msg_regs_mut()[1] = size.try_into().unwrap();
            ipc_buf.msg_regs_mut()[2] = return_cap.is_some() as u64;
            if return_cap.is_some() {
                ipc_buf.set_recv_slot(&return_cap.unwrap());
            }
            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            if return_cap.is_some() {
                if msginfo.extra_caps() != 1 || msginfo.caps_unwrapped() != 0 {
                    return Err(InvocationError::ServerError);
                }
                return Ok(HandleOrHandleCap::new_handle_cap(return_cap.unwrap()));
            } else {
                if msginfo.length() != 1 {
                    return Err(InvocationError::ServerError);
                }
                return Ok(HandleOrHandleCap::new_handle(
                    ipc_buf.msg_regs()[0] as usize,
                ));
            }
        });
    }

    fn window_register(
        &self,
        publish_hndl: &LocalHandle<ConnectionHandle>,
        window_hndl_cap: &HandleCap<WindowHandle>,
        client_id: usize,
        reference: usize,
    ) -> Result<LocalHandle<WindowRegistrationHandle>, InvocationError> {
        // reference is just a number that is returned to the server when a fault occurs

        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::WindowRegister as u64)
            .length(3)
            .extra_caps(1)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = publish_hndl.idx.try_into().unwrap();
            ipc_buf.msg_regs_mut()[1] = client_id.try_into().unwrap();
            ipc_buf.msg_regs_mut()[2] = reference.try_into().unwrap();
            ipc_buf.caps_or_badges_mut()[0] = window_hndl_cap.cptr.path().bits();

            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            Ok(LocalHandle::<WindowRegistrationHandle>::new(
                ipc_buf.msg_regs()[0] as usize,
            ))
        });
    }

    fn window_deregister(
        &self,
        win_reg_hndl: LocalHandle<WindowRegistrationHandle>,
    ) -> Result<(), InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::WindowDeregister as u64)
            .length(1)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = win_reg_hndl.idx.try_into().unwrap();

            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            Ok(())
        });
    }

    fn page_map(
        &self,
        win_reg_hndl: &LocalHandle<WindowRegistrationHandle>,
        view_offset: usize,
        content_vaddr: *const u8,
    ) -> Result<(), InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::PageMap as u64)
            .length(3)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = win_reg_hndl.idx.try_into().unwrap();
            ipc_buf.msg_regs_mut()[1] = view_offset as u64;
            ipc_buf.msg_regs_mut()[2] = content_vaddr as u64;

            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            Ok(())
        });
    }

    fn window_destroy(
        &self,
        handle: HandleOrHandleCap<WindowHandle>,
    ) -> Result<(), InvocationError> {
        let mut msginfo_builder =
            sel4::MessageInfoBuilder::default().label(SMOSInvocation::WindowDestroy as u64);
        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            msginfo_builder = match handle {
                HandleOrHandleCap::Handle(LocalHandle { idx, .. }) => {
                    ipc_buf.msg_regs_mut()[WindowDestroyArgs::Handle as usize] =
                        idx.try_into().unwrap();
                    msginfo_builder.length(1)
                }
                HandleOrHandleCap::HandleCap(HandleCap { cptr, .. }) => {
                    ipc_buf.caps_or_badges_mut()[WindowDestroyArgs::Handle as usize] =
                        cptr.path().bits();
                    msginfo_builder.extra_caps(1)
                }
            };

            let msginfo = self.ep().call(msginfo_builder.build());
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            Ok(())
        });
    }

    /* @alwin: I think this should be a generic functon that generalises for all kernel objects
    that can be allocated */
    fn reply_create(&self, return_cap: AbsoluteCPtr) -> Result<ReplyWrapper, InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::ReplyCreate as u64)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.set_recv_slot(&return_cap);

            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            if (msginfo.extra_caps() != 1 || msginfo.caps_unwrapped() != 0 || msginfo.length() != 1)
            {
                return Err(InvocationError::ServerError);
            }

            Ok(ReplyWrapper {
                handle: ipc_buf.msg_regs()[0] as usize,
                cap: sel4::CPtr::from_bits(return_cap.path().bits()).cast(),
            })
        });
    }

    fn server_handle_cap_create(
        &self,
        publish_hndl: &LocalHandle<ConnectionHandle>, // \/ @alwin: This is super confusing
        ident: usize,
        return_slot: AbsoluteCPtr,
    ) -> Result<(LocalHandle<HandleCapHandle>, AbsoluteCPtr), InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::ServerHandleCapCreate as u64)
            .length(2)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.set_recv_slot(&return_slot);

            ipc_buf.msg_regs_mut()[0] = publish_hndl.idx as u64;
            ipc_buf.msg_regs_mut()[1] = ident as u64;

            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            if msginfo.extra_caps() != 1 || msginfo.caps_unwrapped() != 0 || msginfo.length() != 1 {
                return Err(InvocationError::ServerError);
            }

            Ok((
                LocalHandle::<HandleCapHandle>::new(ipc_buf.msg_regs()[0] as usize),
                return_slot,
            ))
        });
    }

    fn process_spawn(
        &self,
        executable_name: &str,
        fs_name: &str,
        prio: u8,
        argv: Option<&[&str]>,
    ) -> Result<LocalHandle<ProcessHandle>, InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::ProcSpawn as u64)
            .length(2)
            .build();

        let shared_buf_raw = self
            .get_buf_mut()
            .ok_or(InvocationError::DataBufferNotSet)?;
        let mut shared_buf =
            unsafe { slice::from_raw_parts_mut(shared_buf_raw.0, shared_buf_raw.1) };

        /* Copy the executable name into the shared buffer */
        shared_buf = copy_terminated_rust_string_to_buffer(shared_buf, executable_name)?;

        /* Copy the file server name into the shared buffer */
        shared_buf = copy_terminated_rust_string_to_buffer(shared_buf, fs_name)?;

        /* Copy the args for the application into the shared buffer */
        if argv.is_some() {
            for arg in argv.unwrap() {
                shared_buf = copy_terminated_rust_string_to_buffer(shared_buf, arg)?;
            }
        }

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = match argv {
                None => 0,
                Some(v) => v.len() as u64,
            };
            ipc_buf.msg_regs_mut()[1] = prio as u64;
            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            Ok(LocalHandle::new((ipc_buf.msg_regs()[0] as usize)))
        });
    }

    fn process_wait(&self, hndl: LocalHandle<ProcessHandle>) -> Result<(), InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::ProcWait as u64)
            .length(1)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = hndl.idx as u64;
            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;
            // @alwin: error code?
            return Ok(());
            // return Ok(try_unpack_error(ipc_buf.msg_regs()[0], ipc_buf.msg_regs[1..]));
        });
    }

    fn process_exit(&self) -> Result<(), InvocationError> {
        let msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::ProcExit as u64)
            .build();

        sel4::with_ipc_buffer_mut(|ipc_buf| {
            // @alwin: error code?
            // ipc_buf.msg_regs_mut[0] = match err {
            // Some(x) => x as u64,
            // None => InvocationError::NoError as u64,
            // };
            self.ep().call(msginfo)
        });

        // @alwin: Deal with return type
        unreachable!()
    }

    fn load_complete(&self, entry_point: usize, sp: usize) -> Result<(), InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::LoadComplete as u64)
            .length(2)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = entry_point as u64;
            ipc_buf.msg_regs_mut()[1] = sp as u64;
            let msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            Ok(())
        });
    }
}

pub struct ReplyWrapper {
    pub handle: usize,
    pub cap: sel4::cap::Reply,
}

pub trait ObjectServerInterface: ClientConnection {
    fn conn_open(
        &mut self,
        shared_buf: Option<(HandleOrHandleCap<ObjectHandle>, (*mut u8, usize))>,
    ) -> Result<(), InvocationError> {
        let mut msginfo_builder =
            sel4::MessageInfoBuilder::default().label(SMOSInvocation::ConnOpen as u64);

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            msginfo_builder = match shared_buf.as_ref() {
                None => msginfo_builder,
                Some((hndl, buffer)) => {
                    msginfo_builder = match hndl {
                        HandleOrHandleCap::Handle(LocalHandle { idx, .. }) => {
                            ipc_buf.msg_regs_mut()[0] = (*idx).try_into().unwrap();
                            msginfo_builder
                        }
                        HandleOrHandleCap::HandleCap(HandleCap { cptr, .. }) => {
                            ipc_buf.caps_or_badges_mut()[0] = cptr.path().bits();
                            msginfo_builder.extra_caps(1)
                        }
                    };
                    ipc_buf.msg_regs_mut()[1] = buffer.1 as u64;
                    msginfo_builder.length(2)
                }
            };

            let msginfo = self.ep().call(msginfo_builder.build());
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            if shared_buf.is_some() {
                self.set_buf(Some(shared_buf.unwrap().1));
            }

            Ok(())
        });
    }

    fn conn_close(&mut self) -> Result<(), InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::ConnClose as u64)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            let msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;
            self.set_buf(None);
            return Ok(());
        });
    }

    fn obj_create(
        &self,
        name_opt: Option<&str>,
        size: usize,
        rights: sel4::CapRights,
        attributes: ObjAttributes, // @alwin: Should probably also take in VMAttributes
        return_cap: Option<AbsoluteCPtr>,
    ) -> Result<HandleOrHandleCap<ObjectHandle>, InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::ObjCreate as u64)
            .length(5)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[ObjCreateArgs::HasName as usize] = name_opt.is_some() as u64;
            if name_opt.is_some() {
                let name = name_opt.unwrap();
                let shared_buf_raw = self
                    .get_buf_mut()
                    .ok_or(InvocationError::DataBufferNotSet)?;
                let shared_buf =
                    unsafe { slice::from_raw_parts_mut(shared_buf_raw.0, shared_buf_raw.1) };
                copy_terminated_rust_string_to_buffer(shared_buf, name)?;
            }

            ipc_buf.msg_regs_mut()[ObjCreateArgs::Size as usize] = size as u64;
            ipc_buf.msg_regs_mut()[ObjCreateArgs::Rights as usize] =
                rights.into_inner().0.bits()[0];
            ipc_buf.msg_regs_mut()[ObjCreateArgs::Attributes as usize] = attributes.into_inner();
            ipc_buf.msg_regs_mut()[ObjCreateArgs::ReturnCap as usize] = return_cap.is_some() as u64;
            if return_cap.is_some() {
                ipc_buf.set_recv_slot(&return_cap.unwrap());
            }

            let msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            if return_cap.is_some() {
                if msginfo.extra_caps() != 1 || msginfo.caps_unwrapped() != 0 {
                    return Err(InvocationError::ServerError);
                }
                return Ok(HandleOrHandleCap::new_handle_cap(return_cap.unwrap()));
            } else {
                if msginfo.length() != 1 {
                    return Err(InvocationError::ServerError);
                }
                return Ok(HandleOrHandleCap::new_handle(
                    ipc_buf.msg_regs()[0] as usize,
                ));
            }
        });
    }

    fn obj_open(
        &self,
        name: &str,
        rights: sel4::CapRights,
        return_cap: Option<AbsoluteCPtr>,
    ) -> Result<HandleOrHandleCap<ObjectHandle>, InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::ObjOpen as u64)
            .length(3)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            let shared_buf_raw = self
                .get_buf_mut()
                .ok_or(InvocationError::DataBufferNotSet)?;
            let shared_buf =
                unsafe { slice::from_raw_parts_mut(shared_buf_raw.0, shared_buf_raw.1) };
            copy_terminated_rust_string_to_buffer(shared_buf, name)?;

            ipc_buf.msg_regs_mut()[0] = rights.into_inner().0.bits()[0];
            ipc_buf.msg_regs_mut()[1] = return_cap.is_some() as u64;
            if return_cap.is_some() {
                ipc_buf.set_recv_slot(&return_cap.unwrap());
            }

            let msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            if return_cap.is_some() {
                if msginfo.extra_caps() != 1 || msginfo.caps_unwrapped() != 0 {
                    return Err(InvocationError::ServerError);
                }
                return Ok(HandleOrHandleCap::new_handle_cap(return_cap.unwrap()));
            } else {
                if msginfo.length() != 1 {
                    return Err(InvocationError::ServerError);
                }
                return Ok(HandleOrHandleCap::new_handle(
                    ipc_buf.msg_regs()[0] as usize,
                ));
            }
        });
    }

    fn obj_close(&self, hndl: HandleOrHandleCap<ObjectHandle>) -> Result<(), InvocationError> {
        let mut msginfo_builder =
            sel4::MessageInfoBuilder::default().label(SMOSInvocation::ObjClose as u64);
        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            msginfo_builder = match hndl {
                HandleOrHandleCap::Handle(LocalHandle { idx, .. }) => {
                    ipc_buf.msg_regs_mut()[0] = idx.try_into().unwrap();
                    msginfo_builder.length(1)
                }
                HandleOrHandleCap::HandleCap(HandleCap { cptr, .. }) => {
                    ipc_buf.caps_or_badges_mut()[0] = cptr.path().bits();
                    msginfo_builder.extra_caps(1)
                }
            };

            let msginfo = self.ep().call(msginfo_builder.build());
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            Ok(())
        });
    }

    // @alwin: Is obj_destroy actually needed?
    fn obj_destroy(&self, hndl: HandleOrHandleCap<ObjectHandle>) -> Result<(), InvocationError> {
        let mut msginfo_builder =
            sel4::MessageInfoBuilder::default().label(SMOSInvocation::ObjDestroy as u64);
        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            msginfo_builder = match hndl {
                HandleOrHandleCap::Handle(LocalHandle { idx, .. }) => {
                    ipc_buf.msg_regs_mut()[0] = idx.try_into().unwrap();
                    msginfo_builder.length(1)
                }
                HandleOrHandleCap::HandleCap(HandleCap { cptr, .. }) => {
                    ipc_buf.caps_or_badges_mut()[0] = cptr.path().bits();
                    msginfo_builder.extra_caps(1)
                }
            };

            let msginfo = self.ep().call(msginfo_builder.build());
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            Ok(())
        });
    }

    fn obj_stat(&self, hndl: &HandleOrHandleCap<ObjectHandle>) -> Result<ObjStat, InvocationError> {
        let mut msginfo_builder =
            sel4::MessageInfoBuilder::default().label(SMOSInvocation::ObjStat as u64);

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            msginfo_builder = match hndl {
                HandleOrHandleCap::Handle(LocalHandle { idx, .. }) => {
                    ipc_buf.msg_regs_mut()[0] = *idx as u64;
                    msginfo_builder.length(1)
                }
                HandleOrHandleCap::HandleCap(HandleCap { cptr, .. }) => {
                    ipc_buf.caps_or_badges_mut()[0] = cptr.path().bits();
                    msginfo_builder.extra_caps(1)
                }
            };

            let msginfo = self.ep().call(msginfo_builder.build());
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            return Ok(ObjStat {
                size: ipc_buf.msg_regs()[ObjStatReturn::Size as usize] as usize,
                paddr: if ipc_buf.msg_regs_mut()[ObjStatReturn::Paddr as usize] == 0 {
                    None
                } else {
                    Some(ipc_buf.msg_regs_mut()[ObjStatReturn::Paddr as usize] as usize)
                },
            });
        });
    }

    fn view(
        &self,
        win: &HandleOrHandleCap<WindowHandle>,
        obj: &HandleOrHandleCap<ObjectHandle>,
        win_offset: usize,
        obj_offset: usize,
        size: usize,
        rights: sel4::CapRights,
    ) -> Result<LocalHandle<ViewHandle>, InvocationError> {
        let mut msginfo_builder = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::View as u64)
            .length(6);

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            /* Prevent stale data from sticking around in the IPC buffer, which can be dangerous when
            skipping args */
            let mut cap_counter = 0;

            match win {
                HandleOrHandleCap::Handle(LocalHandle { idx, .. }) => {
                    ipc_buf.msg_regs_mut()[ViewArgs::Window as usize] = *idx as u64;
                }
                HandleOrHandleCap::HandleCap(HandleCap { cptr, .. }) => {
                    ipc_buf.caps_or_badges_mut()[cap_counter] = cptr.path().bits();
                    ipc_buf.msg_regs_mut()[ViewArgs::Window as usize] = u64::MAX; // @alwin: Is this the way to do it?
                    cap_counter += 1;
                    msginfo_builder = msginfo_builder.extra_caps(cap_counter);
                }
            };

            match obj {
                HandleOrHandleCap::Handle(LocalHandle { idx, .. }) => {
                    ipc_buf.msg_regs_mut()[ViewArgs::Object as usize] = *idx as u64;
                }
                HandleOrHandleCap::HandleCap(HandleCap { cptr, .. }) => {
                    ipc_buf.caps_or_badges_mut()[cap_counter] = cptr.path().bits();
                    ipc_buf.msg_regs_mut()[ViewArgs::Object as usize] = u64::MAX;
                    cap_counter += 1;
                    msginfo_builder = msginfo_builder.extra_caps(cap_counter)
                }
            };

            ipc_buf.msg_regs_mut()[ViewArgs::WinOffset as usize] = win_offset as u64;
            ipc_buf.msg_regs_mut()[ViewArgs::ObjOffset as usize] = obj_offset as u64;
            ipc_buf.msg_regs_mut()[ViewArgs::Size as usize] = size as u64;
            ipc_buf.msg_regs_mut()[ViewArgs::Rights as usize] = rights.into_inner().0.bits()[0];

            let msginfo = self.ep().call(msginfo_builder.build());
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            if msginfo.length() != 1 {
                return Err(InvocationError::ServerError);
            }
            return Ok(LocalHandle::new(ipc_buf.msg_regs()[0] as usize));
        });
    }

    fn unview(&self, view: LocalHandle<ViewHandle>) -> Result<(), InvocationError> {
        let mut msginfo = sel4::MessageInfoBuilder::default()
            .label(SMOSInvocation::Unview as u64)
            .length(1)
            .build();

        return sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = view.idx as u64;

            let msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs())?;

            return Ok(());
        });
    }
}

pub trait FileServerInterface: ClientConnection {
    fn file_open(&self) -> Result<(), InvocationError> {
        sel4::with_ipc_buffer_mut(|ipc_buf| {
            ipc_buf.msg_regs_mut()[0] = 100;
            let mut msginfo = sel4::MessageInfoBuilder::default()
                .label(SMOSInvocation::TestSimple as u64)
                .length(1)
                .build();
            msginfo = self.ep().call(msginfo);
            try_unpack_error(msginfo.label(), ipc_buf.msg_regs());
            return Ok(());
        })
    }
    fn file_read() {
        todo!()
    }
    fn file_write() {
        todo!()
    }
    fn file_close() {
        todo!()
    }
}
