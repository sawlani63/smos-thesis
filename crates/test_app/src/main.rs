#![no_std]
#![no_main]
#![feature(core_intrinsics)]
#![allow(internal_features)]
#![feature(lang_items)]

use core::arch::global_asm;

fn main()  -> sel4::Result<Never> {
    sel4::debug_println!("hi there");

    unsafe {
        *(0xdeadbeef as *mut u8) = 1;
    }

    unreachable!()
}

// global_asm! {
//  r"
//      .extern __rust_entry
//      .extern __stack_top

//      .section .text

//      .global _start
//      _start:
//          ldr x9, =__stack_top
//          ldr x9, [x9]
//          mov sp, x9
//          b __rust_entry

//          1: b 1b
//  "
// }

global_asm! {
    r"
        .extern __rust_entry

        .section .text

        .global _start
        _start:
            b __rust_entry

            1: b 1b
    "
}


enum Never {}

#[no_mangle]
unsafe extern "C" fn __rust_entry() -> ! {
    match main() {
        Ok(absurdity) => match absurdity {},
        Err(err) => panic!("Error: {}", err),
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    sel4::debug_println!("{}", info);
    core::intrinsics::abort()
}

#[lang = "eh_personality"]
fn eh_personality() -> ! {
    panic!("unexpected call to eh_personality")
}

// @alwin: idk what's happening here, why do I need this
#[no_mangle]
extern "C" fn _Unwind_Resume() -> ! {
    unreachable!("Unwinding not supported");
}