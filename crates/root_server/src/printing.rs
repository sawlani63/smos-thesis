use core::fmt;
use crate::uart::UARTPrinter;

static mut UART_PRINTER : Option<UARTPrinter> = None;

#[doc(hidden)]
pub fn print_helper(args: fmt::Arguments) {
    fmt::write(unsafe { &mut UART_PRINTER.expect("Attempting to use printing before initializing") }, args).unwrap_or_else(|err| {
        panic!("write error: {:?}", err)
    })
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::printing::print_helper(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::println!(""));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[macro_export]
macro_rules! log_rs {
    () => {
        sel4::debug_println!();
    };
    ($($arg:tt)*) => {{
        $crate::print!("root_server|INFO: ");
        $crate::println!($($arg)*);
    }};
}

pub fn print_init(uart_printer: UARTPrinter) {
    unsafe {
        UART_PRINTER = Some(uart_printer);
    }
}