use core::ptr;
use crate::page::BIT;
use sel4::sel4_cfg;

#[sel4_cfg(PLAT = "qemu-arm-virt")]
pub const UART_PADDR: usize = 0x9000000;
const PL011_UARTFR_TXFF: u32 = BIT(5) as u32;

#[sel4_cfg(PLAT = "qemu-arm-virt")]
#[repr(C)]
pub struct UART {
    dr: u32,        /* 0x000 Data Register */
    rsr_ecr: u32,   /* 0x004 Receive Status Register/Error Clear Register */
    res1: u32,      /* 0x008 Reserved */
    res2: u32,      /* 0x00c Reserved */
    res3: u32,      /* 0x010 Reserved */
    res4: u32,      /* 0x014 Reserved */
    fr: u32,        /* 0x018 Flag Register */
    res5: u32,      /* 0x01c Reserved */
    ilpr: u32,      /* 0x020 IrDA Low-Power Counter Register */
    ibrd: u32,      /* 0x024 Integer Baud Rate Register */
    fbrd: u32,      /* 0x028 Fractional Baud Rate Register */
    lcr_h: u32,     /* 0x02c Line Control Register */
    tcr: u32,       /* 0x030 Control Register */
    ifls: u32,      /* 0x034 Interrupt FIFO Level Select Register */
    imsc: u32,      /* 0x038 Interrupt Mask Set/Clear Register */
    ris: u32,       /* 0x03C Raw Interrupt Status Register */
    mis: u32,       /* 0x040 Masked Interrupt Status Register */
    icr: u32,       /* 0x044 Interrupt Clear Register */
    dmacr: u32,     /* 0x048 DMA Control Register */
    /* The rest of the registers are either reserved or not used */
}

#[sel4_cfg(PLAT = "qemu-arm-virt")]
pub fn plat_uart_init(uart: &mut UART) {
    unsafe {
        ptr::write_volatile(&mut uart.imsc as *mut u32, 0x50);
    }
}


#[sel4_cfg(PLAT = "qemu-arm-virt")]
pub fn plat_uart_put_char(uart: &mut UART, c: char) {
    unsafe {
        while (ptr::read_volatile(&uart.fr as *const u32) & PL011_UARTFR_TXFF) != 0 {};
        ptr::write_volatile(&mut uart.dr as *mut u32, c as u32)
    }
}