#![no_std]

use core::fmt;

pub mod args;
mod entry;
pub mod env;

pub use entry::run_main;
pub use smos_macros::smos_declare_main;

use sel4_panicking_env::abort;

// NOTE(rustc_wishlist) remove once #![never_type] is stabilized
#[derive(Debug, Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum Never {}

impl fmt::Display for Never {
    fn fmt(&self, _f: &mut fmt::Formatter) -> fmt::Result {
        match *self {}
    }
}

#[macro_export]
macro_rules! smos_declare_main_internal {
    {
    	$main:expr $(,)?
    } => {
		#[no_mangle]
		fn inner_entry() -> ! {
			$crate::run_main($main)
		}
	}
}
