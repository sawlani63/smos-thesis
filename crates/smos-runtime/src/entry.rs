use core::arch::global_asm;
use sel4_panicking_env::abort;
use core::ptr;
use sel4_panicking::catch_unwind;
use core::panic::UnwindSafe;
use smos_cspace::SMOSUserCSpace;
use smos_common::client_connection::ClientConnection;
use smos_common::connection::RootServerConnection;
use smos_common::init::InitCNodeSlots::{*};
use smos_common::local_handle::{LocalHandle, ConnectionHandle};
use linked_list_allocator::LockedHeap;
use smos_common::string::rust_str_from_buffer;
use crate::args::init_args;
use crate::env;

// @alwin: Do this more properly. Map in a heap from the root server and initialize this
// instead
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

static mut heap: [u8; 4096] = [0; 4096];

global_asm! {
    r"
        .extern sel4_runtime_rust_entry

        .section .text

        .global _start
        _start:
            ldrsw x0, [sp]
            ldr x1, [sp, #4]
            ldr x2, [sp, #12]
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
unsafe extern "C" fn sel4_runtime_rust_entry(argc: u32, argv: *const u8, envp: *const u8) -> ! {
    unsafe extern "C" fn cont_fn(_cont_arg: *mut sel4_runtime_common::ContArg) -> ! {
        inner_entry()
    }

    unsafe { init_args(argc as usize, argv) };
    unsafe { env::init_env(envp) };
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
        ::sel4::set_ipc_buffer((env::ipc_buffer() as *mut sel4::IpcBuffer).as_mut().unwrap());
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

    unsafe {
        ALLOCATOR.lock().init(heap.as_mut_ptr(), heap.len());
    }

    // @alwin: There is no conn_hndl associated with the connection to the root server
    // @alwin: Use some constant instread for page size
    let conn = RootServerConnection::new(smos_common::init::slot::RS_EP.cap(),
                                         LocalHandle::<ConnectionHandle>::new(0),
                                         Some((env::rs_shared_buf() as *mut u8, 4096)));

    // @alwin: Revisit this: I don't really get unwinding
    // match catch_unwind(f, cspace) {
    //     Ok(never) => never,
    //     Err(_) => abort!("main() panicked"),
    // };

    f(conn, cspace);

    loop {}
}

