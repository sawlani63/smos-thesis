use crate::config::RegionResource;
use core::ffi::c_char;
use core::mem::MaybeUninit;

const SDDF_DEVICE_MAGIC_LEN: usize = 5;
pub const SDDF_DEVICE_MAGIC: [c_char; SDDF_DEVICE_MAGIC_LEN] = [
    's' as c_char,
    'D' as c_char,
    'D' as c_char,
    'F' as c_char,
    0x1,
];

pub const DEVICE_MAX_REGIONS: usize = 64;
pub const DEVICE_MAX_IRQS: usize = 64;

#[repr(C)]
pub struct DeviceResources {
    pub magic: [c_char; SDDF_DEVICE_MAGIC_LEN],
    pub num_regions: u8,
    pub num_irqs: u8,
    pub regions: [MaybeUninit<DeviceRegionResource>; DEVICE_MAX_REGIONS],
    pub irqs: [MaybeUninit<DeviceIrqResource>; DEVICE_MAX_IRQS],
}

#[repr(C)]
pub struct DeviceRegionResource {
    pub region: RegionResource,
    pub io_addr: usize,
}

#[repr(C)]
pub struct DeviceIrqResource {
    pub id: u8,
}
