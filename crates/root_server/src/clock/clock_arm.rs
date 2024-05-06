#![allow(non_snake_case)]

use core::arch::asm;
use sel4_config::{sel4_cfg_bool};

const TIMER_ENABLE: usize = 1 << 0;
pub const TIMEOUT_IRQ: usize = 30;

fn COPROC_WRITE_WORD(register: TimerRegisters, value: usize) {
	// @alwin: this is kinda horrible
	unsafe {
		match register {
			TimerRegisters::CNTP_CTL => {
				asm! {
					"msr cntp_ctl_el0, {value}",
					value = in(reg) value
				}
			},
			TimerRegisters::CNTP_CVAL => {
				asm! {
					"msr cntp_cval_el0, {value}",
					value = in(reg) value
				}
			},
			TimerRegisters::CNTPCT => panic!("Not allowed to write to cntpct_el0"),
			TimerRegisters::CNTFRQ => {
				asm! {
					"msr cntfrq_el0, {value}",
					value = in(reg) value
				}
			},
		}
	}
}

fn COPROC_READ_WORD(register: TimerRegisters) -> usize {
	// @alwin: this is kinda horrible and macros don't help
	let mut res : usize;
	unsafe {
		match register {
			TimerRegisters::CNTP_CTL => {
				asm! {
					"mrs {value}, cntp_ctl_el0",
					value = out(reg) res
				}
			},
			TimerRegisters::CNTP_CVAL => {
				asm! {
					"mrs {value}, cntp_cval_el0",
					value = out(reg) res
				}
			},
			TimerRegisters::CNTPCT => {
				asm! {
					"mrs {value}, cntpct_el0",
					value = out(reg) res
				}
			},
			TimerRegisters::CNTFRQ => {
				asm! {
					"mrs {value}, cntfrq_el0",
					value = out(reg) res
				}
			},
		}
	}
	return res;
}

#[allow(non_camel_case_types)]
enum TimerRegisters {
	CNTP_CTL,
	CNTP_CVAL,
	CNTPCT,
	CNTFRQ
}

fn timer_or_ctrl(bits: usize) {
	let ctrl = COPROC_READ_WORD(TimerRegisters::CNTP_CTL);
	COPROC_WRITE_WORD(TimerRegisters::CNTP_CTL, ctrl | bits);
}

fn timer_enable() {
	timer_or_ctrl(TIMER_ENABLE)
}

fn get_ticks() -> usize {
	return COPROC_READ_WORD(TimerRegisters::CNTPCT);
}

fn timer_set_compare(ticks: usize) {
	COPROC_WRITE_WORD(TimerRegisters::CNTP_CVAL, ticks);
}

const HZ: usize = 1;
const KHZ: usize = 1000 * HZ;
const MHZ: usize = 1000 * KHZ;
const GHZ: usize = 1000 * MHZ;

const MS_IN_S: usize = 1000;
const US_IN_MS: usize = 1000;
const US_IN_S: usize = US_IN_MS * MS_IN_S;
const NS_IN_S: usize = 1000000000;

fn cycles_and_freq_to_ns(cycles: usize, freq: usize) -> usize {
	if freq % GHZ == 0 {
		return cycles / freq / GHZ
	} else if freq % MHZ == 0 {
		return cycles * MS_IN_S / (freq / MHZ)
	} else if freq * KHZ == 0 {
		return (cycles * US_IN_S) / (freq / KHZ)
	} else {
		return (cycles * NS_IN_S) / freq
	}
}

fn ns_and_freq_to_cycles(ns: usize, freq: usize) -> usize {
	return (ns * freq) / NS_IN_S
}

pub fn get_time() -> usize {
	let curr_ticks = get_ticks();
	return cycles_and_freq_to_ns(curr_ticks, timer_get_freq());
}

pub fn timer_get_freq() -> usize {
	return COPROC_READ_WORD(TimerRegisters::CNTFRQ);
}

pub fn plat_timer_init() {
	/* @alwin: it would be better to do this at compile time */
	assert!(sel4_cfg_bool!(EXPORT_PCNT_USER));
	assert!(sel4_cfg_bool!(EXPORT_PTMR_USER));
	timer_set_compare(usize::MAX);
	timer_enable();
}

pub fn disable_timer() {
	let ctrl = COPROC_READ_WORD(TimerRegisters::CNTP_CTL);
	COPROC_WRITE_WORD(TimerRegisters::CNTP_CTL, ctrl & !TIMER_ENABLE);
}

pub fn configure_timeout_at(_curr_time: usize, deadline: usize) {
	timer_set_compare(ns_and_freq_to_cycles(deadline, timer_get_freq()));
}