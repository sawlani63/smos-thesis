use crate::uart::UARTPrinter;
use core::fmt;

static mut UART_PRINTER: Option<UARTPrinter> = None;

pub const COLOR_RED: &str = "\x1B[31m";
pub const COLOR_YELLOW: &str = "\x1B[33m";
pub const COLOR_RESET: &str = "\x1B[0m";

#[doc(hidden)]
pub fn print_helper(args: fmt::Arguments) {
    fmt::write(
        unsafe { &mut UART_PRINTER.expect("Attempting to use printing before initializing") },
        args,
    )
    .unwrap_or_else(|err| panic!("write error: {:?}", err))
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

#[macro_export]
macro_rules! err_rs {
    () => {
        sel4::debug_println!();
    };
    ($($arg:tt)*) => {{
        $crate::print!("{}root_server|ERR: ", $crate::printing::COLOR_RED);
        $crate::println!($($arg)*);
        $crate::print!("{}", $crate::printing::COLOR_RESET);
    }};
}

#[macro_export]
macro_rules! warn_rs {
    () => {
        sel4::debug_println!();
    };
    ($($arg:tt)*) => {{
        $crate::print!("{}root_server|WARN: ", $crate::printing::COLOR_YELLOW);
        $crate::println!($($arg)*);
        $crate::print!("{}", $crate::printing::COLOR_RESET);
    }};
}

pub fn print_init(uart_printer: UARTPrinter) {
    unsafe {
        UART_PRINTER = Some(uart_printer);
    }
}
