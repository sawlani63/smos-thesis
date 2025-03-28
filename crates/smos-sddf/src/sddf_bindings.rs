use crate::sddf_channel::sDDFChannel;
use alloc::collections::btree_map::BTreeMap;
use core::ffi::{c_char, CStr};
use smos_common::client_connection::ClientConnection;
use smos_common::connection::sDDFConnection;
use smos_common::syscall::ReplyWrapper;
use smos_server::event::{decode_entry_type, smos_serv_replyrecv, EntryType};

#[allow(dead_code)]
const MAX_CHANNELS: usize = 64;

static mut CHANNELS: BTreeMap<usize, sDDFChannel> = BTreeMap::new();
static mut PPCID_TO_CHANNELID: BTreeMap<usize, usize> = BTreeMap::new();

extern "C" {
    pub fn init();
    pub fn notified(id: u32);
    pub fn protected(id: u32, msginfo: sel4_sys::seL4_MessageInfo) -> sel4_sys::seL4_MessageInfo;
}

pub fn sddf_set_channel(id: usize, ppc_id: Option<usize>, channel: sDDFChannel) -> Result<(), ()> {
    unsafe {
        if CHANNELS.contains_key(&id) {
            return Err(());
        }
        CHANNELS.insert(id, channel);
        if let Some(x) = ppc_id {
            PPCID_TO_CHANNELID.insert(x, id);
        }
        return Ok(());
    }
}

pub fn ppc_get_channel_id(ppc_id: usize) -> usize {
    return unsafe { PPCID_TO_CHANNELID[&ppc_id] };
}

#[no_mangle]
pub unsafe extern "C" fn __assert_fail(
    msg: *mut c_char,
    file: *mut c_char,
    line: i32,
    function: *mut c_char,
) {
    sel4::debug_println!(
        "{}:{} {} -> {}",
        CStr::from_ptr(file).to_str().unwrap(),
        line,
        CStr::from_ptr(function).to_str().unwrap(),
        CStr::from_ptr(msg).to_str().unwrap()
    );
}

#[no_mangle]
pub unsafe extern "C" fn sddf_deferred_notify(id: u32) {
    sddf_notify(id)
}

#[no_mangle]
pub unsafe extern "C" fn sddf_notify(id: u32) {
    CHANNELS[&(id as usize)].notify();
}

#[no_mangle]
pub unsafe extern "C" fn sddf_deferred_notify_curr() -> u32 {
    return u32::MAX;
}

#[no_mangle]
pub unsafe extern "C" fn sddf_set_mr(idx: u32, val: u64) {
    sel4::with_ipc_buffer_mut(|ipc_buf| {
        ipc_buf.msg_regs_mut()[idx as usize] = val;
    });
}

#[no_mangle]
pub unsafe extern "C" fn sddf_ppcall(
    id: u32,
    msginfo_raw: sel4_sys::seL4_MessageInfo,
) -> sel4_sys::seL4_MessageInfo {
    let msginfo = sel4::MessageInfo::from_inner(msginfo_raw);
    CHANNELS[&(id as usize)].ppcall(msginfo).into_inner()
}

#[no_mangle]
pub unsafe extern "C" fn sddf_get_mr(idx: u32) -> u64 {
    return sel4::with_ipc_buffer(|ipc_buf| ipc_buf.msg_regs()[idx as usize]);
}

#[no_mangle]
pub unsafe extern "C" fn sddf_irq_ack(id: u32) {
    CHANNELS[&(id as usize)].irq_ack();
}

#[no_mangle]
pub unsafe extern "C" fn sddf_deferred_irq_ack(id: u32) {
    CHANNELS[&(id as usize)].irq_ack();
}

pub fn sddf_event_loop_ppc(listen_conn: sDDFConnection, reply: ReplyWrapper) -> ! {
    let mut reply_msg_info = None;
    loop {
        let (msg, badge) = if reply_msg_info.is_some() {
            listen_conn
                .ep()
                .reply_recv(reply_msg_info.unwrap(), reply.cap)
        } else {
            listen_conn.ep().recv(reply.cap)
        };

        match decode_entry_type(badge.try_into().unwrap()) {
            EntryType::Notification(bits) => {
                for ch in bits {
                    unsafe { notified(ch as u32) }
                }
                reply_msg_info = None;
            }
            EntryType::Invocation(id) => {
                reply_msg_info = Some(sel4::MessageInfo::from_inner(unsafe {
                    protected(ppc_get_channel_id(id) as u32, msg.into_inner())
                }));
            }
            _ => {
                sel4::debug_println!("This sDDF component cannot handle faults");
                reply_msg_info = None;
            }
        }
    }
}

pub fn sddf_event_loop(listen_conn: sDDFConnection, reply: ReplyWrapper) -> ! {
    let mut reply_msg_info = None;
    loop {
        let (_msg, badge) = smos_serv_replyrecv(&listen_conn, &reply, reply_msg_info);

        match decode_entry_type(badge.try_into().unwrap()) {
            EntryType::Notification(bits) => {
                for ch in bits {
                    unsafe { notified(ch as u32) }
                }
                reply_msg_info = None;
            }
            _ => {
                sel4::debug_println!("This sDDF component cannot handle faults or ppc",);
                reply_msg_info = None;
            }
        }
    }
}
