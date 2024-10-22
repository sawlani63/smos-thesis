use sel4::sel4_cfg;
use smos_common::util::BIT;
use tock_registers::interfaces::{Readable, Writeable};
use tock_registers::registers::{ReadOnly, ReadWrite};
use tock_registers::{register_bitfields, register_structs};

#[sel4_cfg(PLAT = "odroidc2")]
pub const UART_PADDR: usize = 0xc81004c0;

#[sel4_cfg(PLAT = "odroidc2")]
register_structs! {
    pub UART {
        (0x000 => wfifo: ReadWrite<u32>),
        (0x004 => rfifo: ReadWrite<u32>),
        (0x008 => ctrl: ReadWrite<u32, CONTROL::Register>),
        (0x00c => status: ReadOnly<u32, STATUS::Register>),
        (0x010 => misc: ReadWrite<u32>),
        (0x014 => reg5: ReadWrite<u32>),
        (0x018 => @END),
    }
}

#[sel4_cfg(PLAT = "odroidc2")]
register_bitfields![u32,
    STATUS [
        TX_FIFO_FULL 21
    ],
    CONTROL [
        TX_ENABLE 12
    ],
];

#[sel4_cfg(PLAT = "odroidc2")]
pub fn plat_uart_init(uart: &mut UART) {
    uart.ctrl.write(CONTROL::TX_ENABLE::SET);
}

#[sel4_cfg(PLAT = "odroidc2")]
pub fn plat_uart_put_char(uart: &mut UART, c: char) {
    while uart.status.is_set(STATUS::TX_FIFO_FULL) {}
    uart.wfifo.set(c.into());
}
