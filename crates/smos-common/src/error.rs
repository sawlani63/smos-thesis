use crate::invocations::SMOSInvocation;
use num_enum::{IntoPrimitive, TryFromPrimitive};

// @alwin: I don't like how these have to be done seperately,
// is there a cleaner way to do it?
#[derive(Debug, TryFromPrimitive, IntoPrimitive)]
#[repr(u64)]
pub enum InvocationErrorLabel {
    NoError = 0,
    InvalidInvocation,
    NotEnoughArgs,
    NotEnoughCaps,
    InvalidType,
    CSpaceFull,
    UnsupportedInvocation,
    OutOfHandles,
    OutOfHandleCaps,
    AlignmentError,
    InvalidArguments,
    InvalidHandle,
    InvalidHandleCapability,
    DataBufferNotSet,
    BufferTooLarge,
    InsufficientResources,
    ServerError,
}

#[derive(Debug)]
pub enum InvocationError {
    NoError,
    InvalidInvocation,
    NotEnoughArgs { expected: usize, actual: usize },
    NotEnoughCaps { expected: usize, actual: usize },
    InvalidType { which_arg: usize },
    CSpaceFull,
    UnsupportedInvocation { label: SMOSInvocation },
    OutOfHandles,
    OutOfHandleCaps,
    AlignmentError { which_arg: usize },
    InvalidArguments,
    InvalidHandle { which_arg: usize },
    InvalidHandleCapability { which_arg: usize },
    DataBufferNotSet,
    BufferTooLarge,
    InsufficientResources,
    ServerError,
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum NotEnoughArgsMessage {
    Expected = 0,
    Actual = 1,
    Length = 2,
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum NotEnoughCapsMessage {
    Expected = 0,
    Actual = 1,
    Length = 2,
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum InvalidTypeMessage {
    Which = 0,
    Length = 1,
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum UnsupportedInvocationMessage {
    Label = 0,
    Length = 1,
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum AlignmentErrorMessage {
    Which = 0,
    Length = 1,
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum InvalidHandleMessage {
    Which = 0,
    Length = 1,
}

#[derive(IntoPrimitive)]
#[repr(usize)]
pub enum InvalidHandleCapabilityMessage {
    Which = 0,
    Length = 1,
}

pub fn try_unpack_error(label: u64, ipc_buf: &[sel4::Word]) -> Result<(), InvocationError> {
    match label.try_into().expect("This probably shouldn't panic") {
        InvocationErrorLabel::NoError => Ok(()),
        InvocationErrorLabel::InvalidInvocation => Err(InvocationError::InvalidInvocation),
        InvocationErrorLabel::NotEnoughArgs => Err(InvocationError::NotEnoughArgs {
            expected: ipc_buf[usize::from(NotEnoughArgsMessage::Expected)] as usize,
            actual: ipc_buf[usize::from(NotEnoughArgsMessage::Actual)] as usize,
        }),
        InvocationErrorLabel::NotEnoughCaps => Err(InvocationError::NotEnoughCaps {
            expected: ipc_buf[usize::from(NotEnoughCapsMessage::Expected)] as usize,
            actual: ipc_buf[usize::from(NotEnoughCapsMessage::Actual)] as usize,
        }),
        InvocationErrorLabel::InvalidType => {
            todo!();
            // Err(InvocationError::InvalidType { which_arg: todo!() })
        }
        InvocationErrorLabel::CSpaceFull => Err(InvocationError::CSpaceFull),
        InvocationErrorLabel::UnsupportedInvocation => {
            Err(InvocationError::UnsupportedInvocation {
                label: SMOSInvocation::try_from(
                    ipc_buf[usize::from(UnsupportedInvocationMessage::Label)],
                )
                .unwrap(),
            })
        }
        InvocationErrorLabel::OutOfHandles => Err(InvocationError::OutOfHandles),
        InvocationErrorLabel::OutOfHandleCaps => Err(InvocationError::OutOfHandleCaps),
        InvocationErrorLabel::AlignmentError => Err(InvocationError::AlignmentError {
            which_arg: ipc_buf[usize::from(AlignmentErrorMessage::Which)] as usize,
        }),
        InvocationErrorLabel::InvalidArguments => Err(InvocationError::InvalidArguments),
        InvocationErrorLabel::InvalidHandle => Err(InvocationError::InvalidHandle {
            which_arg: ipc_buf[usize::from(InvalidHandleMessage::Which)] as usize,
        }),
        InvocationErrorLabel::InvalidHandleCapability => {
            Err(InvocationError::InvalidHandleCapability {
                which_arg: ipc_buf[usize::from(InvalidHandleCapabilityMessage::Which)] as usize,
            })
        }
        InvocationErrorLabel::DataBufferNotSet => Err(InvocationError::DataBufferNotSet),
        InvocationErrorLabel::BufferTooLarge => Err(InvocationError::BufferTooLarge),
        InvocationErrorLabel::InsufficientResources => Err(InvocationError::InsufficientResources),
        InvocationErrorLabel::ServerError => Err(InvocationError::ServerError),
    }
}
