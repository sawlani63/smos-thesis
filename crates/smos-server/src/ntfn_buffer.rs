use sel4_shared_ring_buffer::RingBuffer;
use smos_common::local_handle::{LocalHandle, WindowRegistrationHandle};
use sel4_shared_ring_buffer::roles::{Write, Read};
use num_enum::{TryFromPrimitive, IntoPrimitive};
use zerocopy::{AsBytes, FromBytes, FromZeroes};
use sel4_externally_shared::{ExternallySharedRef, ExternallySharedRefExt};
use core::ptr::NonNull;
use sel4_shared_ring_buffer::{RawRingBuffer, InitializationStrategy, InitialState};

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
}

#[derive(Debug)]
pub enum NotificationType {
	VMFaultNotification(VMFaultNotification)
}

impl Into<NtfnBufferData> for NotificationType {

	fn into(self) -> NtfnBufferData {
		match self {
			NotificationType::VMFaultNotification(x) => x.into()
		}
	}
}

// @alwin: Is there a way to automate this?
impl Into<VMFaultNotification> for NtfnBufferData {
	fn into(self) -> VMFaultNotification {
		VMFaultNotification {
			client_id: self.data0,
			reference: self.data1,
			fault_offset: self.data2
		}
	}
}

impl Into<NotificationType> for NtfnBufferData {
	fn into(self) -> NotificationType {
		let label = self.label.try_into().expect("I don't know what this is");

		match label {
			NotificationLabel::VMFaultNotificationLabel => NotificationType::VMFaultNotification(self.into()),
		}
	}
}

#[derive(Debug)]
pub struct VMFaultNotification {
	pub client_id: usize,
	pub reference: usize,
	pub fault_offset: usize,
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

pub unsafe fn init_ntfn_buffer(raw_ntfn_buffer_addr: *mut u8) {
	let ntfn_buffer_addr = NonNull::new_unchecked(raw_ntfn_buffer_addr as *mut RawRingBuffer<NtfnBufferData>);
	let ntfn_buffer = RingBuffer::<sel4_shared_ring_buffer::roles::Write, NtfnBufferData>::new(
    						ExternallySharedRef::new(ntfn_buffer_addr),
    						InitializationStrategy::UseAndWriteState(InitialState::new(0, 0)));
}

// @alwin: This is a bit sus, but I don't like having all the dependencies you need for dealing with
// ring buffers in the application crates.
pub unsafe fn enqueue_ntfn_buffer_msg(raw_ntfn_buffer_addr: *mut u8, msg: NotificationType) -> Result<(), NtfnBufferData> {

    let ntfn_buffer_addr = NonNull::new_unchecked(raw_ntfn_buffer_addr as *mut RawRingBuffer<NtfnBufferData>);
	let mut ntfn_buffer =
		RingBuffer::<sel4_shared_ring_buffer::roles::Write, NtfnBufferData>::new(
			ExternallySharedRef::new(ntfn_buffer_addr),
			InitializationStrategy::ReadState
		);

	ntfn_buffer.enqueue_and_commit(msg.into()).expect("@alwin: deal with this")
}

pub unsafe fn dequeue_ntfn_buffer_msg(raw_ntfn_buffer_addr: *mut u8)
		-> Option<NotificationType> {

    let ntfn_buffer_addr = NonNull::new_unchecked(raw_ntfn_buffer_addr as *mut RawRingBuffer<NtfnBufferData>);
	let mut ntfn_buffer =
		RingBuffer::<sel4_shared_ring_buffer::roles::Read, NtfnBufferData>::new(
			ExternallySharedRef::new(ntfn_buffer_addr),
			InitializationStrategy::ReadState
		);

	if let Some(msg) = ntfn_buffer.dequeue().expect("@alwin: deal with this") {
		return Some(msg.into());
	}

	return None;
}

