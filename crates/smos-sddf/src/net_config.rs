use crate::{config::RegionResource, device_config::DeviceRegionResource};
use core::ffi::c_char;
use core::mem::MaybeUninit;

pub const SDDF_NET_MAGIC_LEN: usize = 5;
pub const SDDF_NET_MAGIC: [c_char; SDDF_NET_MAGIC_LEN] = [
    's' as c_char,
    'D' as c_char,
    'D' as c_char,
    'F' as c_char,
    0x5,
];

pub const SDDF_LIB_SDDF_LWIP_MAGIC_LEN: usize = 5;
pub const SDDF_LIB_SDDF_LWIP_MAGIC: [c_char; SDDF_LIB_SDDF_LWIP_MAGIC_LEN] = [
    's' as c_char,
    'D' as c_char,
    'D' as c_char,
    'F' as c_char,
    0x8,
];

pub const SDDF_NET_MAX_CLIENTS: usize = 64;
const MAC_ADDR_LEN: usize = 6;

#[repr(C)]
pub struct NetConnectionResource {
    pub free_queue: RegionResource,
    pub active_queue: RegionResource,
    pub num_buffers: u16,
    pub id: u8,
}

#[repr(C)]
pub struct NetDriverConfig {
    pub magic: [c_char; SDDF_NET_MAGIC_LEN],
    pub virt_rx: NetConnectionResource,
    pub virt_tx: NetConnectionResource,
}

#[repr(C)]
pub struct NetVirtRxConfig {
    pub magic: [c_char; SDDF_NET_MAGIC_LEN],
    pub driver: NetConnectionResource,
    pub data: DeviceRegionResource,
    // The system designer must allocate a buffer metadata region for internal
    // use by the RX virtualiser. The size of this region must be at least
    // 4*drv_queue_capacity. It must be mapped R-W and zero-initialised.
    pub buffer_metadata: RegionResource,
    pub clients: [MaybeUninit<NetVirtRxClientConfig>; SDDF_NET_MAX_CLIENTS],
    pub num_clients: u8,
}

#[repr(C)]
pub struct NetVirtRxClientConfig {
    pub conn: NetConnectionResource,
    pub mac_addr: [u8; MAC_ADDR_LEN],
}

#[repr(C)]
pub struct NetVirtTxClientConfig {
    pub conn: NetConnectionResource,
    pub data: DeviceRegionResource,
}

#[repr(C)]
pub struct NetVirtTxConfig {
    pub magic: [c_char; SDDF_NET_MAGIC_LEN],
    pub driver: NetConnectionResource,
    pub clients: [MaybeUninit<NetVirtTxClientConfig>; SDDF_NET_MAX_CLIENTS],
    pub num_clients: u8,
}

#[repr(C)]
pub struct NetCopyConfig {
    pub magic: [c_char; SDDF_NET_MAGIC_LEN],
    pub virt_rx: NetConnectionResource,
    pub device_data: RegionResource,

    pub client: NetConnectionResource,
    pub client_data: RegionResource,
}

#[repr(C)]
pub struct NetClientConfig {
    pub magic: [c_char; SDDF_NET_MAGIC_LEN],
    pub rx: NetConnectionResource,
    pub rx_data: RegionResource,

    pub tx: NetConnectionResource,
    pub tx_data: RegionResource,

    pub mac_addr: [u8; MAC_ADDR_LEN],
}

#[repr(C)]
pub struct LibSddfLwipConfig {
    pub magic: [c_char; SDDF_NET_MAGIC_LEN],
    pub pbuf_pool: RegionResource,
    pub num_pbufs: u64,
}
