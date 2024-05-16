use core::arch::global_asm;
use sel4_panicking_env::abort;
use core::ptr;
use sel4_panicking::catch_unwind;
use core::panic::UnwindSafe;

// @alwin: should this be passed on the stack somehow?
pub const IPC_BUFFER: *mut sel4::IpcBuffer = 0xA0000000 as *mut sel4::IpcBuffer;

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

extern "Rust" {
    fn inner_entry() -> !;
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

#[doc(hidden)]
pub fn run_main<T>(
    f: impl FnOnce() -> T + UnwindSafe
) -> ! {

    #[cfg(all(feature = "unwinding", panic = "unwind"))]
    {
        ::sel4_runtime_common::set_eh_frame_finder().unwrap();
    }

    unsafe {
        ::sel4::set_ipc_buffer(IPC_BUFFER.as_mut().unwrap());
        ::sel4_runtime_common::run_ctors();
    }

    match catch_unwind(f) {
        Ok(never) => never,
        Err(_) => abort!("main() panicked"),
    };

    loop {}
}

// fn inner_entry() -> ! {
//     #[cfg(panic = "unwind")]
//     {
//         sel4_runtime_common::set_eh_frame_finder().unwrap();
//     }

//     unsafe {
//         sel4::set_ipc_buffer(IPC_BUFFER.as_mut().unwrap());
//         sel4_runtime_common::run_ctors();
//     }

//     match catch_unwind(main) {
//         Ok(never) => never,
//         Err(_) => abort!("main() panicked"),
//     };

//     loop {}
// }