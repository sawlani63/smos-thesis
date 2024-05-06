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
	callback: Option<TimerCallback>,
	callback_data: *const (),
	next: Option<u16>,
	prev: Option<u16>,
}

type TimerCallback = fn(usize, *const ());

const MAX_TIMEOUTS: usize = 1024;
static mut TIMEOUTS_HEAD_FULL: Option<u16> = None;
static mut TIMEOUTS_HEAD_EMPTY: Option<u16> = Some(0);
static mut TIMEOUTS: [TimeoutNode; MAX_TIMEOUTS] = [TimeoutNode { deadline: 0,
															       callback: None,
															       callback_data: core::ptr::null(),
															       next: None,
															       prev: None};
													 MAX_TIMEOUTS];

fn timeouts_empty() -> bool {
	unsafe {
		return TIMEOUTS_HEAD_FULL == None;
	}
}

fn timeouts_full() -> bool {
	unsafe {
		return TIMEOUTS_HEAD_EMPTY == None;
	}
}

fn timeouts_peek() -> Option<usize> {
	if timeouts_empty() {
		return None;
	}

	unsafe {
		return Some(TIMEOUTS[TIMEOUTS_HEAD_FULL.unwrap() as usize].deadline);
	}
}

fn timeouts_remove_min() -> (usize, TimerCallback, *const (), usize) {
	unsafe {
		let rm_index = TIMEOUTS_HEAD_FULL.expect("Head was empty") as usize;
		TIMEOUTS_HEAD_FULL = TIMEOUTS[rm_index].next;
		if TIMEOUTS_HEAD_FULL != None {
			TIMEOUTS[TIMEOUTS_HEAD_FULL.unwrap() as usize].prev = None
		}
		TIMEOUTS[rm_index].next = TIMEOUTS_HEAD_EMPTY;
		TIMEOUTS_HEAD_EMPTY = Some(rm_index.try_into().unwrap());

		return (TIMEOUTS[rm_index].deadline, TIMEOUTS[rm_index].callback.unwrap(),
				TIMEOUTS[rm_index].callback_data, rm_index);
	}
}

fn timer_irq(_data: *const (), _irq: usize, irq_handler: sel4::cap::IrqHandler) -> i32 {

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

	irq_handler.irq_handler_ack().expect("Failed to ack");
	return 0;
}

fn timeouts_init() {
	for i in 0..(MAX_TIMEOUTS - 1) {
		unsafe {
			TIMEOUTS[i].next = Some((i + 1) as u16);
		}
	}
	unsafe { TIMEOUTS[MAX_TIMEOUTS - 1].next = None; };
}

// @alwin: This file has way too much unsafe everywhere
unsafe fn timeouts_insert(deadline: usize, callback: TimerCallback, data: *const ()) -> Option<usize>{
	if timeouts_full() {
		return None;
	}

	if timeouts_empty() {
		TIMEOUTS_HEAD_FULL = TIMEOUTS_HEAD_EMPTY;
		TIMEOUTS_HEAD_EMPTY = TIMEOUTS[TIMEOUTS_HEAD_EMPTY.unwrap() as usize].next;
		let timeouts_head_idx = TIMEOUTS_HEAD_FULL.unwrap() as usize;
		TIMEOUTS[timeouts_head_idx].deadline = deadline;
		TIMEOUTS[timeouts_head_idx].callback = Some(callback);
		TIMEOUTS[timeouts_head_idx].callback_data = data;
    	// timeouts[timeouts_head_idx].valid = true; @alwin: necessary?
    	TIMEOUTS[timeouts_head_idx].next = None;
    	TIMEOUTS[timeouts_head_idx].prev = None;
    	return Some(timeouts_head_idx);
	}

	if deadline < TIMEOUTS[TIMEOUTS_HEAD_FULL.unwrap() as usize].deadline {
    	let insert_index = TIMEOUTS_HEAD_EMPTY.unwrap() as usize;
		TIMEOUTS_HEAD_EMPTY = TIMEOUTS[insert_index].next;
    	TIMEOUTS[insert_index].next = TIMEOUTS_HEAD_FULL;
    	TIMEOUTS[TIMEOUTS_HEAD_FULL.unwrap() as usize].prev = Some(insert_index.try_into().unwrap());
    	TIMEOUTS_HEAD_FULL = Some(insert_index.try_into().unwrap());
        TIMEOUTS[insert_index].deadline = deadline;
        TIMEOUTS[insert_index].callback = Some(callback);
        TIMEOUTS[insert_index].callback_data = data;
        // timeouts[insert_index].valid = true;
        return Some(insert_index);
	}

	let mut tmp_index = TIMEOUTS_HEAD_FULL.unwrap() as usize;
	while TIMEOUTS[tmp_index].next.is_some() &&
		  deadline >= TIMEOUTS[TIMEOUTS[tmp_index].next.unwrap() as usize].deadline {

		  	tmp_index = TIMEOUTS[tmp_index].next.unwrap() as usize;

	}

	let insert_index = TIMEOUTS_HEAD_EMPTY.unwrap() as usize;
	TIMEOUTS[insert_index].next = TIMEOUTS[tmp_index].next;
	TIMEOUTS[insert_index].prev = Some(tmp_index.try_into().unwrap());
	TIMEOUTS[tmp_index].next = Some(insert_index.try_into().unwrap());
	if TIMEOUTS[insert_index].next.is_some() {
		TIMEOUTS[TIMEOUTS[insert_index].next.unwrap() as usize].prev = Some(insert_index.try_into().unwrap());
	}
	TIMEOUTS[insert_index].deadline = deadline;
	TIMEOUTS[insert_index].callback = Some(callback);
	TIMEOUTS[insert_index].callback_data = data;
	// timeouts[insert_index].valid = true;
	return Some(insert_index.try_into().unwrap());
}

pub fn register_timer(delay: usize, callback: TimerCallback, data: *const ()) -> Result<(), sel4::Error>{
	let curr_time = get_time();
	let deadline = curr_time + delay;
	let prev_smallest_deadline = timeouts_peek();

	unsafe { timeouts_insert(deadline, callback, data).ok_or(sel4::Error::NotEnoughMemory) }?;

	if prev_smallest_deadline.is_none() || deadline < prev_smallest_deadline.unwrap() {
		configure_timeout_at(curr_time, deadline);
	}

	return Ok(())
}

pub fn clock_init(cspace: &mut CSpace, irq_dispatch: &mut IRQDispatch, ntfn: sel4::cap::Notification) -> Result<(), sel4::Error>{
	plat_timer_init();
	irq_dispatch.register_irq_handler(cspace, TIMEOUT_IRQ, true, timer_irq, core::ptr::null() as *const (), ntfn)?;

	timeouts_init();
	Ok(())
}

