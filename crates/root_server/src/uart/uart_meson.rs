use sel4::sel4_cfg;
use crate::page::BIT;

pub const UART_PADDR: usize =  0xc81004c0;
const UART_CONTROL_TX_ENABLE: u32 = BIT(12) as u32;
const UART_STATUS_TX_FIFO_FULL: u32 = BIT(21) as u32;

#[repr(C)]
pub struct UART {
	pub wfifo: u32,
	pub rfifo: u32,
	pub ctrl: u32,
	pub status: u32,
	pub misc: u32,
	pub reg5: u32
}

pub fn plat_uart_init(uart: &mut UART) {
	uart.ctrl |= UART_CONTROL_TX_ENABLE;
}

pub fn plat_uart_put_char(uart: &mut UART, c: char) {
	while (uart.status == UART_STATUS_TX_FIFO_FULL) {}
	uart.wfifo = c as u32;
}