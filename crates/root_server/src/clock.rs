use sel4_config::{sel4_cfg, sel4_cfg_if};
use crate::irq::IRQDispatch;
use crate::cspace::CSpace;

sel4_cfg_if! {
	if #[sel4_cfg(PLAT_QEMU_ARM_VIRT)] {
		#[path = "clock/clock_arm.rs"]
		mod imp;
	} else if #[sel4_cfg(PLAT_ODROIDC2)] {
		#[path = "clock/clock_meson.rs"]
		mod imp;
	}
}

use imp::{plat_timer_init, TIMEOUT_IRQ, get_time, configure_timeout_at, disable_timer};

#[derive(Copy, Clone)]
struct TimeoutNode {
	deadline: usize,
	callback: Option<timer_callback>,
	callback_data: *const (),
	next: Option<u16>,
	prev: Option<u16>,
}

type timer_callback = fn(usize, *const ());

const MAX_TIMEOUTS: usize = 1024;
static mut timeouts_head_full: Option<u16> = None;
static mut timeouts_head_empty: Option<u16> = Some(0);
static mut timeouts: [TimeoutNode; MAX_TIMEOUTS] = [TimeoutNode { deadline: 0,
															       callback: None,
															       callback_data: core::ptr::null(),
															       next: None,
															       prev: None};
													 MAX_TIMEOUTS];

fn timeouts_empty() -> bool {
	unsafe {
		return timeouts_head_full == None;
	}
}

fn timeouts_full() -> bool {
	unsafe {
		return timeouts_head_empty == None;
	}
}

fn timeouts_peek() -> Option<usize> {
	if timeouts_empty() {
		return None;
	}

	unsafe {
		return Some(timeouts[timeouts_head_full.unwrap() as usize].deadline);
	}
}

fn timeouts_remove_min() -> (usize, timer_callback, *const (), usize) {
	unsafe {
		let rm_index = timeouts_head_full.expect("Head was empty") as usize;
		timeouts_head_full = timeouts[rm_index].next;
		if (timeouts_head_full != None) {
			timeouts[timeouts_head_full.unwrap() as usize].prev = None
		}
		timeouts[rm_index].next = timeouts_head_empty;
		timeouts_head_empty = Some(rm_index.try_into().unwrap());

		return (timeouts[rm_index].deadline, timeouts[rm_index].callback.unwrap(),
				timeouts[rm_index].callback_data, rm_index);
	}
}

fn timer_irq(data: *const (), irq: usize, irq_handler: sel4::cap::IrqHandler) -> i32{

	// @alwin: should this return something
	let curr_time = get_time();
	let mut deadline = timeouts_peek();
	while deadline.is_some() && curr_time >= deadline.unwrap() {
		let (_, callback, data, min_index) = timeouts_remove_min();
		callback(min_index + 1, data);
		deadline = timeouts_peek();
	}

	if deadline.is_some() {
		configure_timeout_at(curr_time, deadline.unwrap());
	} else {
		disable_timer();
	}

	irq_handler.irq_handler_ack();
	return 0;
}

fn timeouts_init() {
	for i in 0..(MAX_TIMEOUTS - 1) {
		unsafe {
			timeouts[i].next = Some((i + 1) as u16);
		}
	}
	unsafe { timeouts[MAX_TIMEOUTS - 1].next = None; };
}

// @alwin: This file has way too much unsafe everywhere
unsafe fn timeouts_insert(deadline: usize, callback: timer_callback, data: *const ()) -> Option<usize>{
	if timeouts_full() {
		return None;
	}

	if timeouts_empty() {
		timeouts_head_full = timeouts_head_empty;
		timeouts_head_empty = timeouts[timeouts_head_empty.unwrap() as usize].next;
		let timeouts_head_idx = timeouts_head_full.unwrap() as usize;
		timeouts[timeouts_head_idx].deadline = deadline;
		timeouts[timeouts_head_idx].callback = Some(callback);
		timeouts[timeouts_head_idx].callback_data = data;
    	// timeouts[timeouts_head_idx].valid = true; @alwin: necessary?
    	timeouts[timeouts_head_idx].next = None;
    	timeouts[timeouts_head_idx].prev = None;
    	return Some(timeouts_head_idx);
	}

	if (deadline < timeouts[timeouts_head_full.unwrap() as usize].deadline) {
    	let insert_index = timeouts_head_empty.unwrap() as usize;
		timeouts_head_empty = timeouts[insert_index].next;
    	timeouts[insert_index].next = timeouts_head_full;
    	timeouts[timeouts_head_full.unwrap() as usize].prev = Some(insert_index.try_into().unwrap());
    	timeouts_head_full = Some(insert_index.try_into().unwrap());
        timeouts[insert_index].deadline = deadline;
        timeouts[insert_index].callback = Some(callback);
        timeouts[insert_index].callback_data = data;
        // timeouts[insert_index].valid = true;
        return Some(insert_index);
	}

	let mut tmp_index = timeouts_head_full.unwrap() as usize;
	while timeouts[tmp_index].next.is_some() &&
		  deadline >= timeouts[timeouts[tmp_index].next.unwrap() as usize].deadline {

		  	tmp_index = timeouts[tmp_index].next.unwrap() as usize;

	}

	let insert_index = timeouts_head_empty.unwrap() as usize;
	timeouts[insert_index].next = timeouts[tmp_index].next;
	timeouts[insert_index].prev = Some(tmp_index.try_into().unwrap());
	timeouts[tmp_index].next = Some(insert_index.try_into().unwrap());
	if timeouts[insert_index].next.is_some() {
		timeouts[timeouts[insert_index].next.unwrap() as usize].prev = Some(insert_index.try_into().unwrap());
	}
	timeouts[insert_index].deadline = deadline;
	timeouts[insert_index].callback = Some(callback);
	timeouts[insert_index].callback_data = data;
	// timeouts[insert_index].valid = true;
	return Some(insert_index.try_into().unwrap());
}

pub fn register_timer(delay: usize, callback: timer_callback, data: *const ()) -> Result<(), sel4::Error>{
	let curr_time = get_time();
	let deadline = curr_time + delay;
	let prev_smallest_deadline = timeouts_peek();

	let insert_index = unsafe { timeouts_insert(deadline, callback, data).ok_or(sel4::Error::NotEnoughMemory) }?;

	if (prev_smallest_deadline.is_none() || deadline < prev_smallest_deadline.unwrap()) {
		configure_timeout_at(curr_time, deadline);
	}

	return Ok(())
}

pub fn clock_init(cspace: &mut CSpace, irq_dispatch: &mut IRQDispatch, ntfn: sel4::cap::Notification) {
	plat_timer_init();
	irq_dispatch.register_irq_handler(cspace, TIMEOUT_IRQ, true, timer_irq, core::ptr::null() as *const (), ntfn);

	timeouts_init()
}

