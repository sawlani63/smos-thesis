use smos_common::error::{InvocationError, InvocationErrorLabel, NotEnoughArgsMessage,
						 NotEnoughCapsMessage, UnsupportedInvocationMessage,
						 AlignmentErrorMessage, InvalidHandleMessage, InvalidHandleCapabilityMessage};
use smos_common::invocations::SMOSInvocation;

pub(crate) fn try_unpack_error(label: u64, ipc_buf: &sel4::IpcBuffer) -> Result<(), InvocationError> {
	match label.try_into().expect("This probably shouldn't panic") {
		InvocationErrorLabel::NoError => Ok(()),
		InvocationErrorLabel::InvalidInvocation => Err(InvocationError::InvalidInvocation),
		InvocationErrorLabel::NotEnoughArgs => {
			Err(InvocationError::NotEnoughArgs {
				expected: ipc_buf.msg_regs()[usize::from(NotEnoughArgsMessage::Expected)] as usize,
				actual: ipc_buf.msg_regs()[usize::from(NotEnoughArgsMessage::Actual)] as usize
			})
		},
		InvocationErrorLabel::NotEnoughCaps => {
			Err(InvocationError::NotEnoughCaps {
				expected: ipc_buf.msg_regs()[usize::from(NotEnoughCapsMessage::Expected)] as usize,
				actual: ipc_buf.msg_regs()[usize::from(NotEnoughCapsMessage::Actual)] as usize
			})
		},
		InvocationErrorLabel::InvalidType => {
			Err(InvocationError::InvalidType {
				which_arg: todo!()
			})
		},
		InvocationErrorLabel::CSpaceFull => Err(InvocationError::CSpaceFull),
		InvocationErrorLabel::UnsupportedInvocation => {
			Err(InvocationError::UnsupportedInvocation {
				label: SMOSInvocation::try_from(ipc_buf.msg_regs()[usize::from(UnsupportedInvocationMessage::Label)]).unwrap()
			})
		},
		InvocationErrorLabel::OutOfHandles => Err(InvocationError::OutOfHandles),
		InvocationErrorLabel::OutOfHandleCaps => Err(InvocationError::OutOfHandleCaps),
		InvocationErrorLabel::AlignmentError => {
			Err(InvocationError::AlignmentError {
				which_arg: ipc_buf.msg_regs()[usize::from(AlignmentErrorMessage::Which)] as usize
			})
		},
		InvocationErrorLabel::InvalidArguments => Err(InvocationError::InvalidArguments),
		InvocationErrorLabel::InvalidHandle => {
			Err(InvocationError::InvalidHandle {
				which_arg: ipc_buf.msg_regs()[usize::from(InvalidHandleMessage::Which)] as usize
			})
		},
		InvocationErrorLabel::InvalidHandleCapability => {
			Err(InvocationError::InvalidHandleCapability {
				which_arg: ipc_buf.msg_regs()[usize::from(InvalidHandleCapabilityMessage::Which)] as usize
			})
		}
		InvocationErrorLabel::DataBufferNotSet => Err(InvocationError::DataBufferNotSet),
		InvocationErrorLabel::BufferTooLarge => Err(InvocationError::BufferTooLarge),
		InvocationErrorLabel::InsufficientResources => Err(InvocationError::InsufficientResources),
		InvocationErrorLabel::ServerError => Err(InvocationError::ServerError),
	}
}