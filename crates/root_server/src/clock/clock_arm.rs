use core::arch::asm;
use sel4_config::{sel4_cfg_bool};

const TIMER_ENABLE: usize = 1 << 0;

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
	// @alwin: this is kinda horrible
	let mut res : usize = 0;
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

fn timer_set_compare(ticks: usize) {
	COPROC_WRITE_WORD(TimerRegisters::CNTP_CVAL, ticks);
}

pub fn plat_timer_init() {
	/* @alwin: it would be better to do this at compile time */
	assert!(sel4_cfg_bool!(EXPORT_PCNT_USER));
	assert!(sel4_cfg_bool!(EXPORT_PTMR_USER));
	timer_set_compare(usize::MAX);
	timer_enable();
}