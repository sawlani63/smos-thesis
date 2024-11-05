use smos_common::connection::RootServerConnection;
use smos_common::error::InvocationError;
use smos_common::local_handle::{ConnectionHandle, IRQRegistrationHandle, LocalHandle};
use smos_common::syscall::RootServerInterface;
use smos_cspace::SMOSUserCSpace;

#[derive(Debug, Copy, Clone)]
pub struct IrqChannel {
    pub irq_reg_hndl: LocalHandle<IRQRegistrationHandle>,
    pub bit: u8,
    pub irq_handler: sel4::cap::IrqHandler,
}

impl IrqChannel {
    pub fn new(
        rs_conn: &RootServerConnection,
        cspace: &mut SMOSUserCSpace,
        publish_hndl: &LocalHandle<ConnectionHandle>,
        irq_number: usize,
    ) -> Result<IrqChannel, InvocationError> {
        let irq_handler_slot = cspace
            .alloc_slot()
            .or(Err(InvocationError::InsufficientResources))?;

        let (irq_reg_hndl, irq_bit, irq_hndlr_cap) = rs_conn.irq_register(
            publish_hndl,
            irq_number,
            true,
            &cspace.to_absolute_cptr(irq_handler_slot),
        )?;

        return Ok(Self {
            irq_reg_hndl: irq_reg_hndl,
            bit: irq_bit,
            irq_handler: irq_hndlr_cap,
        });
    }

    pub fn ack(&self) {
        self.irq_handler
            .irq_handler_ack()
            .expect("Failed to ack an IRQ handler");
    }
}
