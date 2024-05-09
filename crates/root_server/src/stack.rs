use core::arch::asm;
use sel4::{BootInfo, BootInfoPtr};

use crate::cspace::CSpace;
use crate::ut::UTTable;

pub fn utils_run_on_stack(stack_top: usize, func: unsafe extern "C" fn(*const BootInfo, *mut CSpace, *mut UTTable) -> !,
                          bootinfo: &sel4::BootInfoPtr, cspace: &mut CSpace, ut_table: &mut UTTable) {
    unsafe {
        asm!(
            "mov x20, sp",
            "mov sp, {new_stack}",
            "blr {func}",
            "mov sp, x20",
            new_stack = in(reg) stack_top,
            func = in(reg) func,
            in("x0") bootinfo.ptr(),
            in("x1") cspace as *mut CSpace,
            in("x2") ut_table as *mut UTTable,
        )

        /* @alwin: This doesn't work for some reason - seems like same reg is
           used for arg1 and func? */
        // asm!(
        //     "mov x20, sp",
        //     "mov sp, {new_stack}",
        //     "mov x0, {arg0}",
        //     "mov x1, {arg1}",
        //     "blr {func}",
        //     "mov sp, x20",
        //     new_stack = in(reg) stack_top,
        //     func = in(reg) func,
        //     arg0 = in(reg) cspace as *mut CSpace,
        //     arg1 = in(reg) ut_table as *mut UTTable,
        // )
    }
}