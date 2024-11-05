use core::marker::PhantomData;
use smos_common::channel::Channel;
use smos_common::client_connection::ClientConnection;
use smos_common::connection::{sDDFConnection, RootServerConnection};
use smos_common::error::InvocationError;
use smos_common::local_handle::{ChannelAuthorityHandle, ConnectionHandle, HandleCap, LocalHandle};
use smos_common::sddf::VirtType;
use smos_common::syscall::sDDFInterface;
use smos_common::syscall::RootServerInterface;
use smos_cspace::SMOSUserCSpace;

pub trait NotificationChannelType {}

#[derive(Debug, Copy, Clone)]
pub struct BidirectionalChannel {}
#[derive(Debug, Copy, Clone)]
pub struct SendOnlyChannel {}
#[derive(Debug, Copy, Clone)]
pub struct RecieveOnlyChannel {}

impl NotificationChannelType for BidirectionalChannel {}
impl NotificationChannelType for SendOnlyChannel {}
impl NotificationChannelType for RecieveOnlyChannel {}

pub trait PPCType {}

#[derive(Debug, Copy, Clone)]
pub struct PPCAllowed {}
#[derive(Debug, Copy, Clone)]
pub struct PPCForbidden {}

impl PPCType for PPCAllowed {}
impl PPCType for PPCForbidden {}

#[derive(Debug, Copy, Clone)]
pub struct NotificationChannel<T: NotificationChannelType, U: PPCType> {
    pub from_bit: Option<u8>,
    pub from_hndl_cap: Option<HandleCap<ChannelAuthorityHandle>>, // @alwin: Should these be in one struct?
    pub to_channel: Option<Channel>,
    pub ppc_connection: Option<sDDFConnection>,
    pub marker: PhantomData<(T, U)>,
}

impl<T: NotificationChannelType, U: PPCType> NotificationChannel<T, U> {
    pub fn notify(&self) {
        self.to_channel
            .expect("Channel does not have send rights")
            .ntfn
            .signal();
    }
}

impl<T: NotificationChannelType> NotificationChannel<T, PPCAllowed> {
    pub fn ppcall(&self, msginfo: sel4::MessageInfo) -> sel4::MessageInfo {
        return self.ppc_connection.as_ref().unwrap().ep().call(msginfo);
    }
}

impl<T: PPCType> NotificationChannel<BidirectionalChannel, T> {
    /// This is to be called by the client
    pub fn new(
        rs_conn: &RootServerConnection,
        conn: &sDDFConnection,
        cspace: &mut SMOSUserCSpace,
        publish_hndl: &LocalHandle<ConnectionHandle>,
        virt_type: Option<VirtType>,
    ) -> Result<NotificationChannel<BidirectionalChannel, T>, InvocationError> {
        let from_hndl_cap_slot = cspace
            .alloc_slot()
            .or(Err(InvocationError::InsufficientResources))?;
        let (bit, from_hndl_cap) = rs_conn
            .server_channel_create(publish_hndl, &cspace.to_absolute_cptr(from_hndl_cap_slot))?;

        let to_hndl_cap_slot = cspace
            .alloc_slot()
            .or(Err(InvocationError::InsufficientResources))?;
        let to_hndl_cap = conn.sddf_channel_register_bidirectional(
            from_hndl_cap,
            virt_type,
            &cspace.to_absolute_cptr(to_hndl_cap_slot),
        )?;

        let to_channel_slot = cspace
            .alloc_slot()
            .or(Err(InvocationError::InsufficientResources))?;
        let to_channel =
            rs_conn.channel_open(to_hndl_cap, &cspace.to_absolute_cptr(to_channel_slot))?;

        return Ok(NotificationChannel {
            from_bit: Some(bit),
            from_hndl_cap: Some(from_hndl_cap),
            to_channel: Some(to_channel),
            ppc_connection: Some(conn.clone()),
            marker: PhantomData,
        });
    }

    /// This is to be called by the driver
    pub fn open(
        rs_conn: &RootServerConnection,
        cspace: &mut SMOSUserCSpace,
        publish_hndl: &LocalHandle<ConnectionHandle>,
        to_hndl_cap: HandleCap<ChannelAuthorityHandle>,
    ) -> Result<NotificationChannel<BidirectionalChannel, PPCForbidden>, InvocationError> {
        let to_slot = cspace
            .alloc_slot()
            .or(Err(InvocationError::InsufficientResources))?;
        let to_channel = rs_conn.channel_open(to_hndl_cap, &cspace.to_absolute_cptr(to_slot))?;

        let from_hndl_cap_slot = cspace
            .alloc_slot()
            .or(Err(InvocationError::InsufficientResources))?;
        let (bit, from_hndl_cap) = rs_conn
            .server_channel_create(publish_hndl, &cspace.to_absolute_cptr(from_hndl_cap_slot))?;

        return Ok(NotificationChannel {
            from_bit: Some(bit),
            from_hndl_cap: Some(from_hndl_cap),
            to_channel: Some(to_channel),
            ppc_connection: None,
            marker: PhantomData,
        });
    }
}

impl<T: PPCType> NotificationChannel<SendOnlyChannel, T> {
    pub fn new() {
        todo!();
    }

    /// A recv-only channel from the perspective of the one opening it is a send-only channel
    pub fn open(
        rs_conn: &RootServerConnection,
        cspace: &mut SMOSUserCSpace,
        to_hndl_cap: HandleCap<ChannelAuthorityHandle>,
    ) -> Result<NotificationChannel<SendOnlyChannel, PPCForbidden>, InvocationError> {
        let to_slot = cspace
            .alloc_slot()
            .or(Err(InvocationError::InsufficientResources))?;
        let to_channel = rs_conn.channel_open(to_hndl_cap, &cspace.to_absolute_cptr(to_slot))?;

        return Ok(NotificationChannel {
            from_bit: None,
            from_hndl_cap: None,
            to_channel: Some(to_channel),
            ppc_connection: None,
            marker: PhantomData,
        });
    }
}

impl NotificationChannel<RecieveOnlyChannel, PPCAllowed> {
    pub fn new(
        rs_conn: &RootServerConnection,
        conn: &sDDFConnection,
        cspace: &mut SMOSUserCSpace,
        publish_hndl: &LocalHandle<ConnectionHandle>,
    ) -> Result<NotificationChannel<RecieveOnlyChannel, PPCAllowed>, InvocationError> {
        let from_hndl_cap_slot = cspace
            .alloc_slot()
            .or(Err(InvocationError::InsufficientResources))?;
        let (bit, from_hndl_cap) = rs_conn
            .server_channel_create(publish_hndl, &cspace.to_absolute_cptr(from_hndl_cap_slot))?;

        conn.sddf_channel_register_recv_only(from_hndl_cap)?;

        return Ok(NotificationChannel {
            from_bit: Some(bit),
            from_hndl_cap: Some(from_hndl_cap),
            to_channel: None,
            ppc_connection: Some(conn.clone()),
            marker: PhantomData,
        });
    }

    /// A send-only channel from the perspective of the one opening it is a rcv-only channel
    pub fn open() {
        todo!();
    }
}
