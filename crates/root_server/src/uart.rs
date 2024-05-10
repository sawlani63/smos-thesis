use core::fmt::Write;
use crate::mapping::map_device;
use crate::page::{PAGE_SIZE_4K, PAGE_ALIGN_4K};
use crate::ut::UTTable;
use crate::util::MASK;
use crate::cspace::CSpace;
use sel4_config::{sel4_cfg, sel4_cfg_if};

sel4_cfg_if! {
    if #[sel4_cfg(PLAT_QEMU_ARM_VIRT)] {
        #[path = "uart/uart_pl011.rs"]
        mod imp;
    } else if #[sel4_cfg(PLAT_ODROIDC2)] {
        #[path = "uart/uart_meson.rs"]
        mod imp;
    }
}

use imp::{UART, UART_PADDR, plat_uart_init, plat_uart_put_char};

#[derive(Copy, Clone)]
pub struct UARTPrinter {
    uart_vaddr: usize,
}

impl UARTPrinter {
    fn uart_put_char(self: &mut Self, c: char) {
        let uart = UART::from_vaddr(self.uart_vaddr);

        plat_uart_put_char(uart, c);
        if c == '\n' {
            plat_uart_put_char(uart, '\r');
        }
    }
}

impl Write for UARTPrinter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            self.uart_put_char(c);
        }

        return Ok(())
    }
}

impl UART {
    pub fn from_vaddr<'b>(vaddr: usize) -> &'b mut Self{
        unsafe { &mut *(vaddr as *mut Self) }
    }
}

pub fn uart_init(cspace: &mut CSpace, ut_table: &mut UTTable) -> Result<UARTPrinter, sel4::Error> {
    let uart_page_vaddr = map_device(cspace, ut_table, PAGE_ALIGN_4K(UART_PADDR), PAGE_SIZE_4K)?;
    let uart_vaddr = uart_page_vaddr + (UART_PADDR & MASK(sel4_sys::seL4_PageBits.try_into().unwrap()));
    let uart = UART::from_vaddr(uart_vaddr);
    plat_uart_init(uart);

    Ok(UARTPrinter { uart_vaddr: uart_vaddr })
}



