use sel4_sys::seL4_Fault_Splayed::{*};


fn handle_vm_fault(fault_info: sel4_sys::seL4_Fault_VMFault, msg : sel4::MessageInfo, badge: sel4::Word) {
    log_rs!("Handling VM fault");

    log_rs!(
"ip: {:x},
addr: {:x},
fsr: {:x}", fault_info.get_IP(), fault_info.get_Addr(), fault_info.get_FSR());
}

pub fn handle_fault(msg : sel4::MessageInfo, badge: sel4::Word) {
	let fault = sel4::with_ipc_buffer(|buf| sel4_sys::seL4_Fault::get_from_ipc_buffer(msg.inner(), buf.inner()).splay());
    match fault {
        NullFault(_) |
        CapFault(_) |
        UnknownSyscall(_) |
        UserException(_) |
        VGICMaintenance(_) |
        VCPUFault(_) |
        Timeout(_) |
        VPPIEvent(_) => {
            panic!("Don't know how to handle this kind of fault {:?}!", fault)
        },
        VMFault(f) => {
            handle_vm_fault(f, msg, badge);
        },
    }
}