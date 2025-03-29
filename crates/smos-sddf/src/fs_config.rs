use crate::config::RegionResource;
use core::{ffi::c_char, ffi::CStr, mem::MaybeUninit};

pub const LIONS_FS_MAGIC_LEN: usize = 8;
pub const LIONS_FS_MAGIC: [c_char; LIONS_FS_MAGIC_LEN] = [
    'L' as c_char,
    'i' as c_char,
    'o' as c_char,
    'n' as c_char,
    's' as c_char,
    'O' as c_char,
    'S' as c_char,
    0x1,
];

#[repr(C)]
pub struct FsConnectionResource {
    pub command_queue: RegionResource,
    pub completion_queue: RegionResource,
    pub share: RegionResource,
    pub queue_len: u16,
    pub id: u8,
}

#[repr(C)]
pub struct FsServerConfig {
    pub magic: [c_char; LIONS_FS_MAGIC_LEN],
    pub client: FsConnectionResource,
}

#[repr(C)]
pub struct FsClientConfig {
    pub magic: [c_char; LIONS_FS_MAGIC_LEN],
    pub server: FsConnectionResource,
}