#[repr(C)]
pub struct RegionResource {
    pub vaddr: usize,
    pub size: usize,
}
