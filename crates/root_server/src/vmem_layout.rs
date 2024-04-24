
/* Constants for the layout of the root server's address space */
pub const DMA_SIZE_BITS : usize = 		sel4_sys::seL4_LargePageBits as usize;
pub const SCRATCH : usize = 			0xA0000000;
pub const DEVICE_START: usize = 		0xB0000000;
pub const STACK: usize = 				0xC0000000;
pub const IPC_BUFFER: usize = 			0xD0000000;
pub const STACK_PAGES: usize = 			100;
pub const UT_TABLE: usize = 			0x8000000000;
pub const FRAME_TABLE: usize = 			0x8100000000;
pub const FRAME_DATE: usize = 			0x8200000000;

/* Constants for how SOS will layout the address space of any processes it loads up */
pub const PROCESS_STACK_TOP: usize = 	0x90000000;
pub const PROCESS_IPC_BUFFER: usize = 	0xA0000000;
pub const PROCESS_VMEM_START: usize = 	0xC0000000;