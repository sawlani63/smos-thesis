use crate::cspace::{CSpace, CSpaceTrait};
use crate::frame_table::FrameTable;
use crate::proc::{procs_get_mut, ProcessType, UserProcess};
use crate::ut::UTTable;
use crate::vm::handle_vm_fault;
use crate::RSReplyWrapper;
use sel4::Fault;
use smos_server::reply::handle_fault_reply;

// fn handle_vm_fault(fault_info: sel4::VmFault, msg : sel4::MessageInfo, pid: usize) {
//     log_rs!("Handling VM fault");

//     log_rs!("\
// ip: {:x},
// addr: {:x},
// fsr: {:x}", fault_info.ip(), fault_info.addr(), fault_info.fsr());
// }

pub fn handle_fault(
    cspace: &mut CSpace,
    frame_table: &mut FrameTable,
    ut_table: &mut UTTable,
    reply: RSReplyWrapper,
    msg: sel4::MessageInfo,
    pid: usize,
) -> Option<sel4::MessageInfo> {
    let proc_type: &mut ProcessType = &mut procs_get_mut(pid)
        .as_mut()
        .expect("Was called with an invalid badge")
        .borrow_mut();
    let mut p: &mut UserProcess = match proc_type {
        ProcessType::ActiveProcess(x) => x,
        ProcessType::ZombieProcess(_) => panic!("Zombie process faulted"),
    };

    let fault = sel4::with_ipc_buffer(|buf| Fault::new(buf, &msg));

    let ret = match fault {
        sel4::Fault::NullFault(_)
        | sel4::Fault::CapFault(_)
        | sel4::Fault::UnknownSyscall(_)
        | sel4::Fault::UserException(_)
        | sel4::Fault::VGicMaintenance(_)
        | sel4::Fault::VCpuFault(_)
        | sel4::Fault::Timeout(_)
        | sel4::Fault::VPpiEvent(_) => {
            panic!("Don't know how to handle this kind of fault {:?}!", fault)
        }
        sel4::Fault::VmFault(f) => handle_vm_fault(cspace, frame_table, ut_table, reply, &mut p, f),
    }?;

    sel4::with_ipc_buffer_mut(|ipc_buf| handle_fault_reply(ipc_buf, ret))
}
