use sel4::Fault;


fn handle_vm_fault(fault_info: sel4::VmFault, msg : sel4::MessageInfo, badge: sel4::Word) {
    log_rs!("Handling VM fault");

    log_rs!("\
ip: {:x},
addr: {:x},
fsr: {:x}", fault_info.ip(), fault_info.addr(), fault_info.fsr());
}

pub fn handle_fault(msg : sel4::MessageInfo, badge: sel4::Word) -> Option<sel4::MessageInfo> {
	let fault = sel4::with_ipc_buffer(|buf| Fault::new(buf, &msg));
    match fault {
        sel4::Fault::NullFault(_) |
        sel4::Fault::CapFault(_) |
        sel4::Fault::UnknownSyscall(_) |
        sel4::Fault::UserException(_) |
        sel4::Fault::VGicMaintenance(_) |
        sel4::Fault::VCpuFault(_) |
        sel4::Fault::Timeout(_) |
        sel4::Fault::VPpiEvent(_) => {
            panic!("Don't know how to handle this kind of fault {:?}!", fault)
        },
        sel4::Fault::VmFault(f) => {
            handle_vm_fault(f, msg, badge);
        },
    }

    return None;
}