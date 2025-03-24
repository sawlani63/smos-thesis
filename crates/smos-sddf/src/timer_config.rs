use core::ffi::c_char;

pub const SDDF_TIMER_MAGIC_LEN: usize = 5;
pub const SDDF_TIMER_MAGIC: [c_char; SDDF_TIMER_MAGIC_LEN] = [
    's' as c_char,
    'D' as c_char,
    'D' as c_char,
    'F' as c_char,
    0x6,
];

pub struct TimerClientConfig {
    pub magic: [c_char; SDDF_TIMER_MAGIC_LEN],
    pub driver_id: u8,
}
