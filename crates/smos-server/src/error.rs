use smos_common::error::{*};

pub fn handle_error(error: InvocationError, ipc_buf: &mut sel4::IpcBuffer) -> sel4::MessageInfo {
    let mut msginfo = sel4::MessageInfoBuilder::default().length(0);
    msginfo = match error {
        InvocationError::NoError => panic!("Unexpected on server side"),
        InvocationError::InvalidInvocation => {
            msginfo.label(InvocationErrorLabel::InvalidInvocation.into()).length(0)
        },
        InvocationError::NotEnoughArgs{expected, actual} => {
            ipc_buf.msg_regs_mut()[NotEnoughArgsMessage::Expected as usize] = expected.try_into().unwrap();
            ipc_buf.msg_regs_mut()[NotEnoughArgsMessage::Actual as usize] = actual.try_into().unwrap();
            msginfo.label(InvocationErrorLabel::NotEnoughArgs.into()).length(NotEnoughArgsMessage::Length.into())
        },
        InvocationError::NotEnoughCaps{expected, actual} => {
            ipc_buf.msg_regs_mut()[NotEnoughCapsMessage::Expected as usize] = expected.try_into().unwrap();
            ipc_buf.msg_regs_mut()[NotEnoughCapsMessage::Actual as usize] = actual.try_into().unwrap();
            msginfo.label(InvocationErrorLabel::NotEnoughCaps.into()).length(NotEnoughCapsMessage::Length.into())
        },
        InvocationError::InvalidType{which_arg} => {
            ipc_buf.msg_regs_mut()[InvalidTypeMessage::Which as usize] = which_arg.try_into().unwrap();
            msginfo.label(InvocationErrorLabel::NotEnoughCaps.into()).length(NotEnoughCapsMessage::Length.into())
        },
        InvocationError::CSpaceFull => panic!("Unexpected on server side"),
        InvocationError::UnsupportedInvocation{label} => {
            ipc_buf.msg_regs_mut()[UnsupportedInvocationMessage::Label as usize] = label.into();
            msginfo.label(InvocationErrorLabel::UnsupportedInvocation.into()).length(UnsupportedInvocationMessage::Length.into())
        }
    };

    return msginfo.build();
}