/* Constants for the layout of the root server's address space */
pub const _DMA_SIZE_BITS: usize = sel4_sys::seL4_LargePageBits as usize;
pub const _SCRATCH: usize = 0xA0000000;
pub const _DEVICE_START: usize = 0xB0000000;
pub const STACK: usize = 0xC0000000;
pub const STACK_PAGES: usize = 128;
pub const HEAP: usize = 0xD0000000;
pub const HEAP_PAGES: usize = 8192;
pub const _IPC_BUFFER: usize = 0xE0000000;
pub const UT_TABLE: usize = 0x8000000000;
pub const FRAME_TABLE: usize = 0x8100000000;
pub const FRAME_DATA: usize = 0x8200000000;

/* Constants for how SOS will layout the address space of any processes it loads up */
pub const PROCESS_STACK_TOP: usize = 0x90000000;
pub const PROCESS_IPC_BUFFER: usize = 0xA0000000;
pub const PROCESS_RS_DATA_TRANSFER_PAGE: usize = 0xA0001000;
// pub const USER_DEFAULT_STACK_PAGES: usize = 10;
pub const _PROCESS_VMEM_START: usize = 0xC0000000;
