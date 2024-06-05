#[derive(Debug, Copy, Clone)]
pub enum HandleOrUnwrappedHandleCap {
	Handle(usize),
	UnwrappedHandleCap(usize)
}