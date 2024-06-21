pub enum ConnectionCreateReturn {
	ConnectionHandle = 0
}

#[repr(usize)]
pub enum ObjStatReturn {
	Size = 0,
	Length,
}

#[derive(Debug)]
pub struct ObjStat {
	pub size: usize
}