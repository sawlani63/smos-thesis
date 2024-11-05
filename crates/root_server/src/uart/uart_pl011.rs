use sel4::sel4_cfg;
use tock_registers::interfaces::{Readable, Writeable};
use tock_registers::registers::ReadWrite;
use tock_registers::{register_bitfields, register_structs};

#[sel4_cfg(PLAT = "qemu-arm-virt")]
pub const UART_PADDR: usize = 0x9000000;

#[sel4_cfg(PLAT = "qemu-arm-virt")]
// #[repr(C)]
register_structs! {
    pub UART {
        (0x000 => dr: ReadWrite<u32>),                      /* 0x000 Data Register */
        (0x004 => rsr_ecr: ReadWrite<u32>),                 /* 0x004 Receive Status Register/Error Clear Register */
        (0x008 => _reserved1),                              /* 0x00c Reserved */
        (0x018 => fr: ReadWrite<u32, FR::Register>),                      /* 0x018 Flag Register */
        (0x01c => _reserved2),                              /* 0x01c Reserved */
        (0x020 => ilpr: ReadWrite<u32>),                    /* 0x020 IrDA Low-Power Counter Register */
        (0x024 => ibrd: ReadWrite<u32>),                    /* 0x024 Integer Baud Rate Register */
        (0x028 => fbrd: ReadWrite<u32>),                    /* 0x028 Fractional Baud Rate Register */
        (0x02c => lcr_h: ReadWrite<u32>),                   /* 0x02c Line Control Register */
        (0x030 => tcr: ReadWrite<u32>),                     /* 0x030 Control Register */
        (0x034 => ifls: ReadWrite<u32>),                    /* 0x034 Interrupt FIFO Level Select Register */
        (0x038 => imsc: ReadWrite<u32, IMSC::Register>),    /* 0x038 Interrupt Mask Set/Clear Register */
        (0x03c => ris: ReadWrite<u32>),                     /* 0x03C Raw Interrupt Status Register */
        (0x040 => mis: ReadWrite<u32>),                     /* 0x040 Masked Interrupt Status Register */
        (0x044 => icr: ReadWrite<u32>),                     /* 0x044 Interrupt Clear Register */
        (0x048 => dmacr: ReadWrite<u32>),                  /* 0x048 DMA Control Register */
        /* The rest of the registers are either reserved or not used */
        (0x04c => @END),
    }
}

#[sel4_cfg(PLAT = "qemu-arm-virt")]
register_bitfields![u32,
    IMSC [
        RXIM OFFSET(5) NUMBITS(1) []
    ],
    FR [
        TXFF OFFSET(5) NUMBITS(1) [],
        RXFE OFFSET(4) NUMBITS(1) [],
    ],
];

#[sel4_cfg(PLAT = "qemu-arm-virt")]
pub fn plat_uart_init(uart: &mut UART) {
    uart.imsc.write(IMSC::RXIM::SET);
}

#[sel4_cfg(PLAT = "qemu-arm-virt")]
pub fn plat_uart_put_char(uart: &mut UART, c: char) {
    while uart.fr.is_set(FR::TXFF) {}
    uart.dr.set(c.into());
}
