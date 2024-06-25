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