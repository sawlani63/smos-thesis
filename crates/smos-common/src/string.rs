use crate::error::InvocationError;

pub fn copy_terminated_rust_string_to_buffer<'a>(buffer: &'a mut [u8], string: &str) -> Result<&'a mut [u8], InvocationError> {
	if string.len() + 1 > buffer.len() {
		return Err(InvocationError::BufferTooLarge);
	}

	buffer[0..string.len()].copy_from_slice(string.as_bytes());
	buffer[string.len()] = 0;

	let length = buffer.len();
	return Ok(&mut buffer[string.len() + 1..length]);
}

pub fn copy_rust_string_from_buffer<'a>(buffer: &'a [u8]) -> Result<(&'a str, &'a [u8]), InvocationError> {
	let index = buffer.iter().position(|&x| x == 0).ok_or(InvocationError::InvalidArguments)?;
	let length = buffer.len();

	return Ok((core::str::from_utf8(&buffer[0..index]).or(Err(InvocationError::InvalidArguments))?, &buffer[index + 1..length]));
}