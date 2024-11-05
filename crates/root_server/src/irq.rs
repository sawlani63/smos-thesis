use crate::{
    cspace::{CSpace, CSpaceTrait},
    handle::RootServerResource,
    proc::UserProcess,
    ut::{UTTable, UTWrapper},
    util::dealloc_retyped,
};
use alloc::rc::Rc;
use bitfield::{bf_clr_bit, bf_first_free, bf_set_bit, bitfield_type};
use core::cell::RefCell;
use smos_common::{error::InvocationError, local_handle::LocalHandle, util::BIT};
use smos_server::{
    handle::{HandleAllocater, ServerHandle},
    reply::SMOSReply,
    syscalls::IRQRegister,
};

#[derive(Debug)]
#[allow(dead_code)]
pub struct IRQRegistration {
    irq_number: usize,
    badge_bit: u8,
}

impl IRQRegistration {
    pub fn new(irq_num: usize, badge_bit: u8) -> Self {
        return Self {
            irq_number: irq_num,
            badge_bit: badge_bit,
        };
    }
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
struct IRQHandlerInfo {
    irq: usize,
    irq_handler: sel4::cap::IrqHandler,
    ntfn: sel4::cap::Notification,
    callback: IRQCallback,
    data: *const (), // @alwin: This can be done using an enum instead
}

#[allow(dead_code)]
pub struct IRQDispatch {
    irq_control: sel4::cap::IrqControl,
    ntfn: sel4::cap::Notification,
    flag_bits: usize,
    ident_bits: usize,
    allocated_bits: bitfield_type!(sel4::WORD_SIZE),
    irq_handlers: [Option<IRQHandlerInfo>; sel4::WORD_SIZE],
}

type IRQCallback = fn(*const (), usize, sel4::cap::IrqHandler) -> i32;

impl IRQDispatch {
    pub fn new(
        irq_control: sel4::cap::IrqControl,
        ntfn: sel4::cap::Notification,
        flag_bits: usize,
        ident_bits: usize,
    ) -> Self {
        return Self {
            irq_control: irq_control,
            ntfn: ntfn,
            flag_bits: flag_bits,
            ident_bits: ident_bits,
            // @alwin: is this hacky? kinda?
            allocated_bits: [(!ident_bits).try_into().unwrap(); 1],
            irq_handlers: [None; sel4::WORD_SIZE],
        };
    }

    pub fn handle_irq(self: &Self, mut badge: usize) -> sel4::Word {
        let mut unchecked_bits = badge & self.allocated_bits[0] as usize & self.ident_bits;

        while unchecked_bits != 0 {
            let bit: usize = unchecked_bits.trailing_zeros().try_into().unwrap();

            (self.irq_handlers[bit]
                .expect("IRQ handler not registered for this IRQ")
                .callback)(
                self.irq_handlers[bit].unwrap().data,
                self.irq_handlers[bit].unwrap().irq,
                self.irq_handlers[bit].unwrap().irq_handler,
            );

            badge &= !BIT(bit);
            unchecked_bits &= !badge;
        }

        return badge as sel4::Word;
    }

    #[allow(dead_code)]
    pub fn register_irq_handler(
        self: &mut Self,
        cspace: &mut CSpace,
        irq: usize,
        edge_triggered: bool,
        callback: IRQCallback,
        data: *const (),
        irq_ntfn: sel4::cap::Notification,
    ) -> Result<sel4::cap::IrqHandler, sel4::Error> {
        let ident_bit = self.alloc_irq_bit()?;

        // Allocate cptr for irq handler
        let handler_cptr = cspace.alloc_slot().map_err(|e| {
            self.free_irq_bit(ident_bit);
            e
        })?;

        // Allocate cptr for badged ntfn
        let ntfn_cptr = cspace.alloc_slot().map_err(|e| {
            cspace.free_slot(handler_cptr);
            self.free_irq_bit(ident_bit);
            e
        })?;

        // Get handler cap
        let handler = cspace
            .irq_control_get(handler_cptr, self.irq_control, irq, edge_triggered)
            .map_err(|e| {
                cspace.free_slot(ntfn_cptr);
                cspace.free_slot(handler_cptr);
                self.free_irq_bit(ident_bit);
                e
            })?;

        // Set the bade
        let badge = self.flag_bits | BIT(ident_bit);

        // Mint the badged notification
        let ntfn = sel4::CPtr::from_bits(ntfn_cptr.try_into().unwrap())
            .cast::<sel4::cap_type::Notification>();
        cspace
            .root_cnode
            .relative(ntfn)
            .mint(
                &cspace.root_cnode.relative(irq_ntfn),
                sel4::CapRightsBuilder::none().write(true).build(),
                badge.try_into().unwrap(),
            )
            .map_err(|e| {
                // @alwin: Handling the failure case like this is not clean
                let _ = cspace.delete(handler_cptr);
                cspace.free_slot(ntfn_cptr);
                cspace.free_slot(handler_cptr);
                self.free_irq_bit(ident_bit);
                e
            })?;

        // Set the notification for the IRQ
        handler.irq_handler_set_notification(ntfn).map_err(|e| {
            let _ = cspace.delete(ntfn_cptr);
            let _ = cspace.delete(handler_cptr);
            cspace.free_slot(ntfn_cptr);
            cspace.free_slot(handler_cptr);
            self.free_irq_bit(ident_bit);
            e
        })?;

        self.irq_handlers[ident_bit] = Some(IRQHandlerInfo {
            irq: irq,
            irq_handler: handler,
            ntfn: ntfn,
            callback: callback,
            data: data,
        });

        return Ok(handler);
    }

    #[allow(dead_code)]
    fn alloc_irq_bit(self: &mut Self) -> Result<usize, sel4::Error> {
        let bit = bf_first_free(&self.allocated_bits).map_err(|_| sel4::Error::NotEnoughMemory)?;
        bf_set_bit(&mut self.allocated_bits, bit);
        return Ok(bit);
    }

    #[allow(dead_code)]
    fn free_irq_bit(self: &mut Self, bit: usize) {
        bf_clr_bit(&mut self.allocated_bits, bit);
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UserNotificationDispatch {
    irq_control: sel4::cap::IrqControl,
    ntfn: (sel4::cap::Notification, UTWrapper),
    flag_bits: usize,
    ident_bits: usize,
    allocated_bits: bitfield_type!(sel4::WORD_SIZE),
    badged_ntfns: [Option<sel4::cap::Notification>; sel4::WORD_SIZE],
    irq_handler_caps: [Option<sel4::cap::IrqHandler>; sel4::WORD_SIZE],
}

impl UserNotificationDispatch {
    pub fn new(
        irq_control: sel4::cap::IrqControl,
        ntfn: (sel4::cap::Notification, UTWrapper),
        flag_bits: usize,
        ident_bits: usize,
    ) -> Self {
        return Self {
            irq_control: irq_control,
            ntfn: ntfn,
            flag_bits: flag_bits,
            ident_bits: ident_bits,
            // @alwin: is this hacky? kinda?
            allocated_bits: [(!ident_bits).try_into().unwrap(); 1],
            badged_ntfns: [None; sel4::WORD_SIZE],
            irq_handler_caps: [None; sel4::WORD_SIZE],
        };
    }

    pub fn destroy(&mut self, cspace: &mut CSpace, ut_table: &mut UTTable) {
        for ntfn in self.badged_ntfns {
            if ntfn.is_some() {
                cspace
                    .delete_cap(ntfn.unwrap())
                    .expect("Failed t destroy badged notification");
                cspace.free_cap(ntfn.unwrap());
            }
        }

        for irq_hndler in self.irq_handler_caps {
            if irq_hndler.is_some() {
                cspace
                    .delete_cap(irq_hndler.unwrap())
                    .expect("Failed to delete IRQ handler cap");
                cspace.free_cap(irq_hndler.unwrap());
            }
        }

        dealloc_retyped(cspace, ut_table, self.ntfn);
    }

    pub fn ntfn_register(
        &mut self,
        cspace: &mut CSpace,
    ) -> Result<(u8, sel4::cap::Notification), InvocationError> {
        // Allocate a  bit
        let ident_bit = self
            .alloc_ntfn_bit()
            .or(Err(InvocationError::InsufficientResources))?;

        let ntfn = cspace.alloc_cap().map_err(|_| {
            self.free_ntfn_bit(ident_bit);
            InvocationError::InsufficientResources
        })?;

        let badge = self.flag_bits | BIT(ident_bit);

        // Mint the badged notification
        cspace
            .root_cnode()
            .relative(ntfn)
            .mint(
                &cspace.root_cnode().relative(self.ntfn.0),
                sel4::CapRights::write_only(),
                badge.try_into().unwrap(),
            )
            .expect("Failed to mint badged notification");

        self.badged_ntfns[ident_bit] = Some(ntfn);

        return Ok((ident_bit.try_into().unwrap(), ntfn));
    }

    pub fn rs_badged_ntfn(&self) -> sel4::cap::Notification {
        assert!(self.badged_ntfns[0].is_some());
        assert!(self.irq_handler_caps[0].is_none());

        return self.badged_ntfns[0].unwrap();
    }

    pub fn irq_register(
        &mut self,
        cspace: &mut CSpace,
        irq_num: usize,
        edge_triggered: bool,
    ) -> Result<(u8, sel4::cap::IrqHandler), InvocationError> {
        // Allocate a bit
        let ident_bit = self
            .alloc_ntfn_bit()
            .or(Err(InvocationError::InsufficientResources))?;

        // Allocate a cptr for the IRQ handler
        let handler_cptr = cspace.alloc_slot().map_err(|_| {
            self.free_ntfn_bit(ident_bit);
            InvocationError::InsufficientResources
        })?;

        // Allocate a cptr for the badged notification
        let ntfn = cspace.alloc_cap().map_err(|_| {
            cspace.free_slot(handler_cptr);
            self.free_ntfn_bit(ident_bit);
            InvocationError::InsufficientResources
        })?;

        // Get the handler cap
        let handler = cspace
            .irq_control_get(handler_cptr, self.irq_control, irq_num, edge_triggered)
            .map_err(|_| {
                cspace.free_cap(ntfn);
                cspace.free_slot(handler_cptr);
                self.free_ntfn_bit(ident_bit);
                InvocationError::InsufficientResources // @alwin: is this the right error?
            })?;

        // Set the badge
        let badge = self.flag_bits | BIT(ident_bit);

        // Mint the badged notification
        cspace
            .root_cnode()
            .relative(ntfn)
            .mint(
                &cspace.root_cnode().relative(self.ntfn.0),
                sel4::CapRights::write_only(),
                badge.try_into().unwrap(),
            )
            .expect("Failed to mint badged notification");

        // Set the notification for the IRQ
        handler.irq_handler_set_notification(ntfn).map_err(|_| {
            cspace.delete_cap(ntfn).unwrap();
            cspace.delete_cap(handler).unwrap();
            cspace.free_cap(ntfn);
            cspace.free_cap(handler);
            self.free_ntfn_bit(ident_bit);
            InvocationError::InvalidArguments // @alwin: Is this the right error
        })?;

        self.badged_ntfns[ident_bit] = Some(ntfn);
        self.irq_handler_caps[ident_bit] = Some(handler);

        return Ok((ident_bit.try_into().unwrap(), handler));
    }

    fn alloc_ntfn_bit(self: &mut Self) -> Result<usize, sel4::Error> {
        let bit = bf_first_free(&self.allocated_bits).map_err(|_| sel4::Error::NotEnoughMemory)?;
        bf_set_bit(&mut self.allocated_bits, bit);
        return Ok(bit);
    }

    fn free_ntfn_bit(self: &mut Self, bit: usize) {
        bf_clr_bit(&mut self.allocated_bits, bit);
    }
}

pub fn handle_irq_register(
    cspace: &mut CSpace,
    p: &mut UserProcess,
    args: &IRQRegister,
) -> Result<SMOSReply, InvocationError> {
    let publish_hndl_ref = p
        .get_handle(args.publish_hndl.idx)
        .or(Err(InvocationError::InvalidHandle { which_arg: 0 }))?;

    let server = match publish_hndl_ref.as_ref().unwrap().inner() {
        RootServerResource::Server(x) => Ok(x.clone()),
        _ => Err(InvocationError::InvalidHandle { which_arg: 0 }),
    }?;

    let (badge_bit, irq_handler) = server.borrow_mut().ntfn_dispatch.irq_register(
        cspace,
        args.irq_num,
        args.edge_triggered,
    )?;

    let (idx, handle_ref) = p.allocate_handle()?;

    let irq_reg = Rc::new(RefCell::new(IRQRegistration::new(args.irq_num, badge_bit)));

    *handle_ref = Some(ServerHandle::new(RootServerResource::IRQRegistration(
        irq_reg,
    )));

    return Ok(SMOSReply::IRQRegister {
        hndl: LocalHandle::new(idx),
        irq_handler: irq_handler,
        badge_bit: badge_bit,
    });
}

#[allow(dead_code)] // @alwin: Remove once implemented
pub fn handle_irq_deregister() {
    todo!();
}
