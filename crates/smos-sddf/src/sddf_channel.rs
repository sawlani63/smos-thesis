use sel4::MessageInfo;

use crate::irq_channel::IrqChannel;
use crate::notification_channel::{
    BidirectionalChannel, NotificationChannel, PPCAllowed, PPCForbidden, RecieveOnlyChannel,
    SendOnlyChannel,
};

#[derive(Debug, Copy, Clone)]
#[allow(non_camel_case_types)]
pub enum sDDFChannel {
    IrqChannel(IrqChannel),
    NotificationChannelBi(NotificationChannel<BidirectionalChannel, PPCForbidden>),
    NotificationChannelSend(NotificationChannel<SendOnlyChannel, PPCForbidden>),
    NotificationChannelRecvPPC(NotificationChannel<RecieveOnlyChannel, PPCAllowed>),
}

impl sDDFChannel {
    pub fn irq_ack(&self) {
        match self {
            sDDFChannel::IrqChannel(ic) => ic.ack(),
            _ => panic!("Cannot ack a notification channel"),
        }
    }

    pub fn notify(&self) {
        match self {
            sDDFChannel::IrqChannel(_) => panic!("Cannot notify an irq channel"),
            sDDFChannel::NotificationChannelBi(nc) => nc.notify(),
            sDDFChannel::NotificationChannelSend(nc) => nc.notify(),
            sDDFChannel::NotificationChannelRecvPPC(_) => {
                panic!("Cannot notify a recieve only channel")
            }
        }
    }

    pub fn ppcall(&self, msginfo: MessageInfo) -> MessageInfo {
        match self {
            sDDFChannel::NotificationChannelRecvPPC(nc) => nc.ppcall(msginfo),
            _ => panic!("Cannot ppcall this kind of channel"),
        }
    }
}
