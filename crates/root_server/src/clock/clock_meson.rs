
enum TimeoutID {
	TimerA = 0,
	TimerB = 1,
	TimerC = 2,
	TimerD = 3
}

impl TimeoutID {
	pub const fn irq(self: &Self) -> usize {
		match self {
			TimerA => 42,
			TimerB => 43,
			TimerC => 38,
			TimerD => 61
		}
	}

	pub const fn clk(self: &Self) -> usize {
		match self {
			TimerA => 0,
			TimerB => 2,
			TimerC => 4
			TimerD => 6
		}
	}

	pub const fn en(self: &Self) -> usize {
		match self {
			TimerA => BIT(16),
			TimerB => BIT(17),
			TimerC => BIT(18),
			TimerD => BIT(19)
		}
	}

	pub const fn mode(self: &Self) -> usize {
		match self {
			TimerA => BIT(12),
			TimerB => BIT(13),
			TimerC => BIT(14),
			TimerD => BIT(15)
		}
	}
}

pub const TIMEOUT_IRQ = TimeoutID::TimerA.irq();

// map_device(cspace, ut_table, PAGE_ALIGN_4K(TIMER_MAP_BASE), PAGE_SIZE_4K);
