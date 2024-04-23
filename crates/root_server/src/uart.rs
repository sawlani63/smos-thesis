mod uart_meson;
mod uart_pl011;

use crate::mapping::map_device;
use crate::page::{PAGE_SIZE_4K, PAGE_ALIGN_4K};
use crate::ut::UTTable;
use crate::util::MASK;
use crate::cspace::CSpace;
use sel4::sel4_cfg;

#[sel4_cfg(PLAT = "qemu-arm-virt")]
use crate::uart::uart_pl011::{UART, UART_PADDR, plat_uart_init, plat_uart_put_char};

#[sel4_cfg(PLAT = "odroidc2")]
use crate::uart::uart_meson::{UART, UART_PADDR, plat_uart_init, plat_uart_put_char};

impl UART {
	pub fn from_vaddr<'b>(vaddr: usize) -> &'b mut Self{
	    unsafe { &mut *(vaddr as *mut Self) }
	}
}

static mut uart_vaddr: Option<usize> = None;


pub fn uart_init(cspace: &mut CSpace, ut_table: &mut UTTable) -> Result<(), sel4::Error> {
	let uart_page_vaddr = map_device(cspace, ut_table, PAGE_ALIGN_4K(UART_PADDR), PAGE_SIZE_4K)?;
	let uart = unsafe {
		uart_vaddr = Some(uart_page_vaddr + (UART_PADDR & MASK(sel4_sys::seL4_PageBits.try_into().unwrap())));
		UART::from_vaddr(uart_vaddr.unwrap())
	};

	plat_uart_init(uart);

	Ok(())
}

pub fn uart_put_char(c: char) -> Result<(), sel4::Error> {
	let uart = unsafe {
		if (uart_vaddr.is_none()) {
			sel4::debug_println!("Attempting to use uart_put_char without initializing");
			return Err(sel4::Error::IllegalOperation);
		}
		UART::from_vaddr(uart_vaddr.unwrap())
	};

	plat_uart_put_char(uart, c);
	if c == '\n' {
		plat_uart_put_char(uart, '\r');
	}

	return Ok(());
}

