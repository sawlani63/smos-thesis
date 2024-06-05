use sel4::cap::Endpoint;
use smos_common::connection::{*};
use crate::syscall::{*};

pub trait ClientConnection {
	fn ep(&self) -> Endpoint;
	fn new(ep: Endpoint, conn_hndl: ConnectionHandle, buf: Option<(*mut u8, usize)>) -> Self;
	fn hndl(&self) -> ConnectionHandle;
	fn set_buf(&mut self, buf: Option<(*mut u8, usize)>);
	fn get_buf(&self) -> Option<(*const u8, usize)>;
	fn get_buf_mut(&self) -> Option<(*mut u8, usize)>;
}

macro_rules! generate_connection_impl {
	($name:ident) => {
		impl ClientConnection for $name {
			fn ep(&self) -> Endpoint {
				self.ep
			}

			fn new(ep: Endpoint, conn_hndl: ConnectionHandle, buf: Option<(*mut u8, usize)>) -> Self {
				Self {
					conn_hndl: conn_hndl,
					ep: ep,
					buf: buf
				}
			}

			fn set_buf(&mut self, buf: Option<(*mut u8, usize)>) {
				self.buf = buf;
			}

			fn get_buf(&self) -> Option<(*const u8, usize)> {
				match self.buf {
					Some(x) => Some((x.0 as *const u8, x.1)),
					None => None
				}
			}

			fn get_buf_mut(&self) -> Option<(*mut u8, usize)> {
				self.buf
			}

			fn hndl(&self) -> ConnectionHandle {
				self.conn_hndl
			}
		}
	};
}

generate_connection_impl!{RootServerConnection}
impl RootServerInterface for RootServerConnection {}
impl ObjectServerInterface for RootServerConnection {}
impl FileServerInterface for RootServerConnection {}

generate_connection_impl!{FileServerConnection}
impl ObjectServerInterface for FileServerConnection {}
impl FileServerInterface for FileServerConnection{}
