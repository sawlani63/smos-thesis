use core::ptr::NonNull;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use sel4_externally_shared::{ExternallySharedRef, ExternallySharedRefExt};
use sel4_shared_ring_buffer::RingBuffer;
use sel4_shared_ring_buffer::{InitialState, InitializationStrategy, RawRingBuffer};
use zerocopy::{AsBytes, FromBytes, FromZeroes};

const NTFN_BUFFER_CAPACITY: usize = 64;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialOrd, Ord, PartialEq, Eq, AsBytes, FromBytes, FromZeroes)]
pub struct NtfnBufferData {
    label: usize,
    data0: usize,
    data1: usize,
    data2: usize,
}

impl sel4_shared_ring_buffer::Descriptor for NtfnBufferData {}

#[derive(TryFromPrimitive, IntoPrimitive, Debug, PartialEq)]
#[repr(usize)]
pub enum NotificationLabel {
    VMFaultNotificationLabel = 0,
    ConnDestroyNotificationLabel,
    WindowDestroyNotificationLabel,
}

#[derive(Debug)]
pub enum NotificationType {
    VMFaultNotification(VMFaultNotification),
    ConnDestroyNotification(ConnDestroyNotification),
    WindowDestroyNotification(WindowDestroyNotification),
}

impl Into<NtfnBufferData> for NotificationType {
    fn into(self) -> NtfnBufferData {
        match self {
            NotificationType::VMFaultNotification(x) => x.into(),
            NotificationType::ConnDestroyNotification(x) => x.into(),
            NotificationType::WindowDestroyNotification(x) => x.into(),
        }
    }
}

impl Into<NotificationType> for NtfnBufferData {
    fn into(self) -> NotificationType {
        let label = self.label.try_into().expect("I don't know what this is");

        match label {
            NotificationLabel::VMFaultNotificationLabel => {
                NotificationType::VMFaultNotification(self.into())
            }
            NotificationLabel::ConnDestroyNotificationLabel => {
                NotificationType::ConnDestroyNotification(self.into())
            }
            NotificationLabel::WindowDestroyNotificationLabel => {
                NotificationType::WindowDestroyNotification(self.into())
            }
        }
    }
}

/* Related to VM fault forwarding */

#[derive(Debug)]
pub struct VMFaultNotification {
    pub client_id: usize,
    pub reference: usize,
    pub fault_offset: usize,
}

// @alwin: Is there a way to automate this?
impl Into<VMFaultNotification> for NtfnBufferData {
    fn into(self) -> VMFaultNotification {
        VMFaultNotification {
            client_id: self.data0,
            reference: self.data1,
            fault_offset: self.data2,
        }
    }
}

impl Into<NtfnBufferData> for VMFaultNotification {
    fn into(self) -> NtfnBufferData {
        NtfnBufferData {
            label: NotificationLabel::VMFaultNotificationLabel.into(),
            data0: self.client_id,
            data1: self.reference,
            data2: self.fault_offset,
        }
    }
}

/* Related to conn_destroy */

#[derive(Debug)]
pub struct ConnDestroyNotification {
    pub conn_id: usize,
}

impl Into<ConnDestroyNotification> for NtfnBufferData {
    fn into(self) -> ConnDestroyNotification {
        ConnDestroyNotification {
            conn_id: self.data0,
        }
    }
}

impl Into<NtfnBufferData> for ConnDestroyNotification {
    fn into(self) -> NtfnBufferData {
        NtfnBufferData {
            label: NotificationLabel::ConnDestroyNotificationLabel.into(),
            data0: self.conn_id,
            data1: 0,
            data2: 0,
        }
    }
}

/* Related to window_destroy */

#[derive(Debug)]
pub struct WindowDestroyNotification {
    pub client_id: usize,
    pub reference: usize,
}

impl Into<WindowDestroyNotification> for NtfnBufferData {
    fn into(self) -> WindowDestroyNotification {
        WindowDestroyNotification {
            client_id: self.data0,
            reference: self.data1,
        }
    }
}

impl Into<NtfnBufferData> for WindowDestroyNotification {
    fn into(self) -> NtfnBufferData {
        NtfnBufferData {
            label: NotificationLabel::WindowDestroyNotificationLabel.into(),
            data0: self.client_id,
            data1: self.reference,
            data2: 0,
        }
    }
}

/* Methods */
pub unsafe fn init_ntfn_buffer(raw_ntfn_buffer_addr: *mut u8) {
    let ntfn_buffer_addr = NonNull::new_unchecked(
        raw_ntfn_buffer_addr as *mut RawRingBuffer<NtfnBufferData, NTFN_BUFFER_CAPACITY>,
    );
    // @alwin: This is kinda ugly, ideally we should pass this around instead of passing an addr
    // to the enqueue and dequeue functions
    let _ntfn_buffer = RingBuffer::<
        sel4_shared_ring_buffer::roles::Write,
        NtfnBufferData,
        NTFN_BUFFER_CAPACITY,
    >::new(
        ExternallySharedRef::new(ntfn_buffer_addr),
        InitializationStrategy::UseAndWriteState(InitialState::new(0, 0)),
    );
}

// @alwin: This is a bit sus, but I don't like having all the dependencies you need for dealing with
// ring buffers in the application crates.
pub unsafe fn enqueue_ntfn_buffer_msg(
    raw_ntfn_buffer_addr: *mut u8,
    msg: NotificationType,
) -> Result<(), NtfnBufferData> {
    let ntfn_buffer_addr = NonNull::new_unchecked(
        raw_ntfn_buffer_addr as *mut RawRingBuffer<NtfnBufferData, NTFN_BUFFER_CAPACITY>,
    );
    let mut ntfn_buffer = RingBuffer::<
        sel4_shared_ring_buffer::roles::Write,
        NtfnBufferData,
        NTFN_BUFFER_CAPACITY,
    >::new(
        ExternallySharedRef::new(ntfn_buffer_addr),
        InitializationStrategy::ReadState,
    );

    ntfn_buffer
        .enqueue_and_commit(msg.into())
        .expect("@alwin: deal with this")
}

pub unsafe fn dequeue_ntfn_buffer_msg(raw_ntfn_buffer_addr: *mut u8) -> Option<NotificationType> {
    let ntfn_buffer_addr = NonNull::new_unchecked(
        raw_ntfn_buffer_addr as *mut RawRingBuffer<NtfnBufferData, NTFN_BUFFER_CAPACITY>,
    );
    let mut ntfn_buffer = RingBuffer::<
        sel4_shared_ring_buffer::roles::Read,
        NtfnBufferData,
        NTFN_BUFFER_CAPACITY,
    >::new(
        ExternallySharedRef::new(ntfn_buffer_addr),
        InitializationStrategy::ReadState,
    );

    if let Some(msg) = ntfn_buffer.dequeue().expect("@alwin: deal with this") {
        return Some(msg.into());
    }

    return None;
}
