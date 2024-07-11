#![no_std]
#![no_main]

use smos_runtime::{smos_declare_main, Never};

#[smos_declare_main]
fn main(rs_conn: RootServerConnection, mut cspace: SMOSUserCSpace) -> sel4::Result<Never> {

}
