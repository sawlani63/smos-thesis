use crate::config::RegionResource;
use core::{ffi::c_char, ffi::CStr, mem::MaybeUninit};

pub const SDDF_SERIAL_MAX_CLIENTS: usize = 64;
pub const SDDF_SERIAL_BEGIN_STR_MAX_LEN: usize = 128;
pub const SDDF_NAME_LENGTH: usize = 64;

pub const SDDF_SERIAL_MAGIC_LEN: usize = 5;
pub const SDDF_SERIAL_MAGIC: [c_char; SDDF_SERIAL_MAGIC_LEN] = [
    's' as c_char,
    'D' as c_char,
    'D' as c_char,
    'F' as c_char,
    0x3,
];

#[repr(C)]
pub struct SerialConnectionResource {
    pub queue: RegionResource,
    pub data: RegionResource,
    pub id: u8,
}

#[repr(C)]
pub struct SerialDriverConfig {
    pub magic: [c_char; SDDF_SERIAL_MAGIC_LEN],
    pub rx: SerialConnectionResource,
    pub tx: SerialConnectionResource,
    pub default_baud: u64,
    pub rx_enabled: bool,
}

#[repr(C)]
pub struct SerialVirtRxConfig {
    pub magic: [c_char; SDDF_SERIAL_MAGIC_LEN],
    pub driver: SerialConnectionResource,
    pub clients: [MaybeUninit<SerialConnectionResource>; SDDF_SERIAL_MAX_CLIENTS],
    pub num_clients: u8,
    pub switch_char: c_char,
    pub terminate_num_char: c_char,
}

#[repr(C)]
pub struct SerialVirtTxClientConfig {
    pub conn: SerialConnectionResource,
    pub name: [c_char; SDDF_NAME_LENGTH], // @alwin: Idk if Cstr is the right way to do this
}

#[repr(C)]
pub struct SerialVirtTxConfig {
    pub magic: [c_char; SDDF_SERIAL_MAGIC_LEN],
    pub driver: SerialConnectionResource,
    pub clients: [MaybeUninit<SerialVirtTxClientConfig>; SDDF_SERIAL_MAX_CLIENTS],
    pub num_clients: u8,
    pub begin_str: [c_char; SDDF_SERIAL_BEGIN_STR_MAX_LEN], // @alwin: Idk if Cstr is the right way to do this
    pub begin_str_len: u8,
    pub enable_colour: bool,
    pub enable_rx: bool,
}

#[repr(C)]
pub struct SerialClientConfig {
    pub magic: [c_char; SDDF_SERIAL_MAGIC_LEN],
    pub rx: SerialConnectionResource,
    pub tx: SerialConnectionResource,
}
