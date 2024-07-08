use smos_common::error::*;

pub fn handle_error(ipc_buf: &mut sel4::IpcBuffer, error: InvocationError) -> sel4::MessageInfo {
    let mut msginfo = sel4::MessageInfoBuilder::default().length(0);
    msginfo = match error {
        InvocationError::NoError => panic!("Unexpected on server side"),
        InvocationError::InvalidInvocation => {
            msginfo.label(InvocationErrorLabel::InvalidInvocation.into())
        }
        InvocationError::NotEnoughArgs { expected, actual } => {
            ipc_buf.msg_regs_mut()[NotEnoughArgsMessage::Expected as usize] =
                expected.try_into().unwrap();
            ipc_buf.msg_regs_mut()[NotEnoughArgsMessage::Actual as usize] =
                actual.try_into().unwrap();
            msginfo
                .label(InvocationErrorLabel::NotEnoughArgs.into())
                .length(NotEnoughArgsMessage::Length.into())
        }
        InvocationError::NotEnoughCaps { expected, actual } => {
            ipc_buf.msg_regs_mut()[NotEnoughCapsMessage::Expected as usize] =
                expected.try_into().unwrap();
            ipc_buf.msg_regs_mut()[NotEnoughCapsMessage::Actual as usize] =
                actual.try_into().unwrap();
            msginfo
                .label(InvocationErrorLabel::NotEnoughCaps.into())
                .length(NotEnoughCapsMessage::Length.into())
        }
        InvocationError::InvalidType { which_arg } => {
            ipc_buf.msg_regs_mut()[InvalidTypeMessage::Which as usize] =
                which_arg.try_into().unwrap();
            msginfo
                .label(InvocationErrorLabel::NotEnoughCaps.into())
                .length(NotEnoughCapsMessage::Length.into())
        }
        InvocationError::CSpaceFull => panic!("Unexpected on server side"),
        InvocationError::UnsupportedInvocation { label } => {
            ipc_buf.msg_regs_mut()[UnsupportedInvocationMessage::Label as usize] = label.into();
            msginfo
                .label(InvocationErrorLabel::UnsupportedInvocation.into())
                .length(UnsupportedInvocationMessage::Length.into())
        }
        InvocationError::OutOfHandles => msginfo.label(InvocationErrorLabel::OutOfHandles.into()),
        InvocationError::OutOfHandleCaps => {
            msginfo.label(InvocationErrorLabel::OutOfHandleCaps.into())
        }
        InvocationError::AlignmentError { which_arg } => {
            ipc_buf.msg_regs_mut()[AlignmentErrorMessage::Which as usize] =
                which_arg.try_into().unwrap();
            msginfo
                .label(InvocationErrorLabel::AlignmentError as u64)
                .length(AlignmentErrorMessage::Length.into())
        }
        InvocationError::InvalidArguments => {
            msginfo.label(InvocationErrorLabel::InvalidArguments.into())
        }
        InvocationError::InvalidHandle { which_arg } => {
            ipc_buf.msg_regs_mut()[InvalidHandleMessage::Which as usize] =
                which_arg.try_into().unwrap();
            msginfo
                .label(InvocationErrorLabel::InvalidHandle as u64)
                .length(InvalidHandleMessage::Length.into())
        }
        InvocationError::InvalidHandleCapability { which_arg } => {
            ipc_buf.msg_regs_mut()[InvalidHandleCapabilityMessage::Which as usize] =
                which_arg.try_into().unwrap();
            msginfo
                .label(InvocationErrorLabel::InvalidHandleCapability as u64)
                .length(InvalidHandleCapabilityMessage::Length.into())
        }
        InvocationError::DataBufferNotSet => {
            msginfo.label(InvocationErrorLabel::DataBufferNotSet.into())
        }
        InvocationError::BufferTooLarge => {
            msginfo.label(InvocationErrorLabel::BufferTooLarge.into())
        } // @alwin: This might eventually need a way to specify which arg
        InvocationError::InsufficientResources => {
            msginfo.label(InvocationErrorLabel::InsufficientResources.into())
        }
        InvocationError::ServerError => panic!("Unexpected on server side"),
    };

    return msginfo.build();
}
