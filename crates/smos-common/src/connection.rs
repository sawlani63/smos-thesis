#![allow(non_camel_case_types)]

use crate::local_handle::{ConnectionHandle, LocalHandle};
use sel4::cap::Endpoint;

// @alwin: Both the client and the server actually use a RootServerConnection struct, so a
// better idea might be to move the struct itself to smos-common and have the appropriate
// trait impls and stuff in smos-client and smos-server
macro_rules! generate_connection_type {
    ($name:ident) => {
        #[derive(Debug, Copy, Clone)]
        pub struct $name {
            pub ep: Endpoint,
            pub conn_hndl: LocalHandle<ConnectionHandle>,
            pub buf: Option<(*mut u8, usize)>,
        }
    };
}

generate_connection_type!(RootServerConnection);
generate_connection_type!(ObjectServerConnection);
// @alwin: This should not be here
generate_connection_type!(sDDFConnection);
