use core::arch::global_asm;
use sel4_panicking_env::abort;
use core::ptr;
use sel4_panicking::catch_unwind;
use core::panic::UnwindSafe;
use smos_cspace::SMOSUserCSpace;
use smos_client::connection::ClientConnection;
use smos_common::connection::RootServerConnection;
use smos_common::init::InitCNodeSlots::{*};

// @alwin: should this be passed on the stack somehow? I think yes, but I'm not too sure how yet (
// at least with minimal changes and hackery around the initialize_tls_on_stack thing)
pub const IPC_BUFFER: *mut sel4::IpcBuffer =    0xA0000000 as *mut sel4::IpcBuffer;
pub const RS_SHARED_BUFFER: *mut u8 =           0xA0001000 as *mut u8;


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
    f: impl FnOnce(RootServerConnection, SMOSUserCSpace) -> T + UnwindSafe
) -> ! {

    #[cfg(all(feature = "unwinding", panic = "unwind"))]
    {
        ::sel4_runtime_common::set_eh_frame_finder().unwrap();
    }

    unsafe {
        ::sel4::set_ipc_buffer(IPC_BUFFER.as_mut().unwrap());
        ::sel4_runtime_common::run_ctors();
    }

    // Set up the cspace
    let mut cspace = SMOSUserCSpace::new(sel4::CPtr::from_bits(SMOS_CNodeSelf.try_into().unwrap())
                                                          .cast::<sel4::cap_type::CNode>());

    // Alocate the zeroeth sentinel slot
    let mut slot = cspace.alloc_slot().expect("Failed to allocate initial slot");
    assert!(slot == SMOS_CapNull as usize);

    // Allocate the slot used by the cnode self cap
    slot = cspace.alloc_slot().expect("Failed to allocate CNode slot");
    assert!(slot == SMOS_RootServerEP as usize);

    // Allocate the slot used by the endpoint to the root server
    slot = cspace.alloc_slot().expect("Failed to allocate RS ep slot");
    assert!(slot == SMOS_CNodeSelf as usize);

    // @alwin: There is no conn_hndl associated with the connection to the root server
    // @alwin: Use some constant instread for page size
    let conn = RootServerConnection::new(smos_common::init::slot::RS_EP.cap(), 0, Some((RS_SHARED_BUFFER, 4096)));

    // @alwin: Revisit this: I don't really get unwinding
    // match catch_unwind(f, cspace) {
    //     Ok(never) => never,
    //     Err(_) => abort!("main() panicked"),
    // };

    // @alwin: HACK
    f(conn, cspace);

    loop {}
}

