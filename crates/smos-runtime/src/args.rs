use byteorder::{ByteOrder, LittleEndian};
use smos_common::string::rust_str_from_buffer;

static mut args_inner: Option<ArgsInner> = None;

pub unsafe fn init_args(argc: usize, argv: *const u8) {
	let mut args_inner_tmp = ArgsInner {
		inner: [0; 32], // @alwin: HACK
		size: argc,
		index: 0
	};
	if argc > 0 {
	    LittleEndian::read_u64_into(core::slice::from_raw_parts(argv, argc * 8), &mut args_inner_tmp.inner[0..argc]);
	}
	args_inner = Some(args_inner_tmp);
}

pub fn args() -> Args {
	Args {inner: unsafe {args_inner.unwrap()} }
}

pub struct Args {
	inner: ArgsInner,
}

impl Iterator for Args {
	type Item = &'static str;

	fn next(&mut self) -> Option<&'static str> {
		self.inner.next()
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		self.inner.size_hint()
	}
}

impl<'a> Iterator for ArgsInner {
	type Item = &'static str;

	fn next(&mut self) -> Option<&'static str> {
		if self.index < self.size {
            let slice = unsafe { core::slice::from_raw_parts(self.inner[self.index] as *const u8,
            												 crate::env::stack_top() - self.inner[self.index] as usize) };
            let result = Some(rust_str_from_buffer(slice).ok()?.0);
			self.index += 1;
			result
		} else {
			None
		}
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		(self.size - self.index, Some(self.size - self.index))
	}
}

#[derive(Debug, Copy, Clone)]
struct ArgsInner {
	inner: [u64; 32],
	size: usize,
	index: usize
}