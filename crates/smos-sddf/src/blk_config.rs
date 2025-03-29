use crate::{config::RegionResource, device_config::DeviceRegionResource};
use core::{ffi::c_char, ffi::CStr, mem::MaybeUninit};

pub const SDDF_BLK_MAX_CLIENTS: usize = 64;
pub const SDDF_BLK_MAGIC_LEN: usize = 5;
pub const SDDF_BLK_MAGIC: [c_char; SDDF_BLK_MAGIC_LEN] = [
    's' as c_char,
    'D' as c_char,
    'D' as c_char,
    'F' as c_char,
    0x2,
];

#[repr(C)]
pub struct BlkConnectionResource {
    pub storage_info: RegionResource,
    pub req_queue: RegionResource,
    pub resp_queue: RegionResource,
    pub num_buffers: u16,
    pub id: u8,
}

#[repr(C)]
pub struct BlkDriverConfig {
    pub magic: [c_char; SDDF_BLK_MAGIC_LEN],
    pub virt: BlkConnectionResource,
}

#[repr(C)]
pub struct BlkVirtConfigClient {
    pub conn: BlkConnectionResource,
    pub data: DeviceRegionResource,
    pub partition: u32,
}

#[repr(C)]
pub struct BlkVirtConfigDriver {
    pub conn: BlkConnectionResource,
    pub data: DeviceRegionResource,
}

#[repr(C)]
pub struct BlkVirtConfig {
    pub magic: [c_char; SDDF_BLK_MAGIC_LEN],
    pub num_clients: u64,
    pub driver: BlkVirtConfigDriver,
    pub clients: [BlkVirtConfigClient; SDDF_BLK_MAX_CLIENTS],
}

#[repr(C)]
pub struct BlkClientConfig {
    pub magic: [c_char; SDDF_BLK_MAGIC_LEN],
    pub virt: BlkConnectionResource,
    pub data: RegionResource,
}