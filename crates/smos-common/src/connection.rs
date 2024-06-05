// @alwin: Both the client and the server actually use a RootServerConnection struct, so a
// better idea might be to move the struct itself to smos-common and have the appropriate
// trait impls and stuff in smos-client and smos-server
macro_rules! generate_connection_type {
	($name:ident) => {
		pub struct $name {
			pub ep: sel4::cap::Endpoint,
			pub conn_hndl: ConnectionHandle,
			pub buf: Option<(*mut u8, usize)>,
		}
	};
}

pub type ConnectionHandle = usize;

generate_connection_type!(RootServerConnection);
generate_connection_type!(FileServerConnection);
