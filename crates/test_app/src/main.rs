#![no_std]
#![no_main]
#![feature(core_intrinsics)]
#![allow(internal_features)]
#![feature(lang_items)]

const IPC_BUFFER: *mut sel4::IpcBuffer = 0xA0000000 as *mut sel4::IpcBuffer;

use core::arch::global_asm;
use sel4_panicking::catch_unwind;
use core::ptr;
use sel4_panicking_env::abort;
use sel4::CapTypeForFrameObjectOfFixedSize;

fn main()  -> sel4::Result<Never> {
    sel4::debug_println!("hi there");

    let root_server_ep = sel4::CPtr::from_bits(1).cast::<sel4::cap_type::Endpoint>();

    sel4::with_ipc_buffer_mut(|ipc_buf| {
        ipc_buf.msg_regs_mut()[0] = 0xbeefcafe
    });
    sel4::debug_println!("after accessing ipc buffer");
    let msg_info = sel4::MessageInfoBuilder::default().label(1).length(1).build();
    root_server_ep.call(msg_info);

    unreachable!()
}

global_asm! {
    r"
        .extern sel4_runtime_rust_entry

        .section .text

        .global _start
        _start:
            b sel4_runtime_rust_entry

            1: b 1b
    "
}

sel4_panicking_env::register_debug_put_char!(sel4::debug_put_char);

enum Never {}

#[no_mangle]
unsafe extern "C" fn sel4_runtime_rust_entry() -> ! {
    unsafe extern "C" fn cont_fn(_cont_arg: *mut sel4_runtime_common::ContArg) -> ! {
        inner_entry()
    }

    sel4_runtime_common::initialize_tls_on_stack_and_continue(cont_fn, ptr::null_mut())
}

fn inner_entry() -> ! {
    #[cfg(panic = "unwind")]
    {
        sel4_runtime_common::set_eh_frame_finder().unwrap();
    }

    unsafe {
        sel4::set_ipc_buffer(IPC_BUFFER.as_mut().unwrap());
        sel4_runtime_common::run_ctors();
    }

    match catch_unwind(main) {
        Ok(never) => never,
        Err(_) => abort!("main() panicked"),
    };

    loop {}
}
