use sel4_config::{sel4_cfg, sel4_cfg_if};

sel4_cfg_if! {
	if #[sel4_cfg(PLAT_QEMU_ARM_VIRT)] {
		#[path = "clock/clock_arm.rs"]
		mod imp;
	} else if #[sel4_cfg(PLAT_ODROIDC2)] {
		#[path = "clock/clock_meson.rs"]
		mod imp;
	}
}

use imp::{plat_timer_init};

pub fn clock_init() {
	plat_timer_init();
}