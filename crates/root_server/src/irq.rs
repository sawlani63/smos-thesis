use bitfield::{bitfield_type, bf_first_free, bf_set_bit, bf_clr_bit};
use crate::cspace::{CSpace, CSpaceTrait};
use crate::page::BIT;

#[derive(Copy, Clone)]
struct IRQHandlerInfo {
    irq: usize,
    irq_handler: sel4::cap::IrqHandler,
    ntfn: sel4::cap::Notification,
    callback: IRQCallback,
    data: *const () // @alwin: This can be done using an enum instead
}

pub struct IRQDispatch {
    irq_control: sel4::cap::IrqControl,
    ntfn: sel4::cap::Notification,
    flag_bits: usize,
    ident_bits: usize,
    allocated_bits: bitfield_type!(sel4::WORD_SIZE),
    irq_handlers: [Option<IRQHandlerInfo>; sel4::WORD_SIZE]
}

type IRQCallback = fn(*const (), usize, sel4::cap::IrqHandler) -> i32;

impl IRQDispatch {
    pub fn new(irq_control: sel4::cap::IrqControl, ntfn: sel4::cap::Notification, flag_bits: usize,
               ident_bits: usize) -> Self {

        return Self {
            irq_control: irq_control,
            ntfn: ntfn,
            flag_bits: flag_bits,
            ident_bits: ident_bits,
            // @alwin: is this hacky? kinda?
            allocated_bits: [(!ident_bits).try_into().unwrap(); 1],
            irq_handlers: [None; sel4::WORD_SIZE]
        }
    }

    pub fn handle_irq(self: &Self, mut badge: usize) -> sel4::Word {
        let mut unchecked_bits = badge & self.allocated_bits[0] as usize & self.ident_bits;

        while unchecked_bits != 0 {
            let bit: usize = unchecked_bits.trailing_zeros().try_into().unwrap();

            (self.irq_handlers[bit].expect("IRQ handler not registered for this IRQ")
                                  .callback)(self.irq_handlers[bit].unwrap().data, self.irq_handlers[bit].unwrap().irq,
                                            self.irq_handlers[bit].unwrap().irq_handler);

            badge &= !BIT(bit);
            unchecked_bits &= !badge;
        }

        return badge as sel4::Word;
    }

    pub fn register_irq_handler(self: &mut Self, cspace: &mut CSpace, irq: usize, edge_triggered: bool,
                                callback: IRQCallback, data: *const (),
                                irq_ntfn: sel4::cap::Notification) -> Result<sel4::cap::IrqHandler, sel4::Error> {

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
        let handler = cspace.irq_control_get(handler_cptr, self.irq_control, irq, edge_triggered).map_err(|e| {
            cspace.free_slot(ntfn_cptr);
            cspace.free_slot(handler_cptr);
            self.free_irq_bit(ident_bit);
            e
        })?;

        // Set the bade
        let badge = self.flag_bits | BIT(ident_bit);

        // Mint the badged notification
        let ntfn = sel4::CPtr::from_bits(ntfn_cptr.try_into().unwrap()).cast::<sel4::cap_type::Notification>();
        cspace.root_cnode.relative(ntfn).mint(&cspace.root_cnode.relative(irq_ntfn),
                                              sel4::CapRightsBuilder::none().write(true).build(),
                                              badge.try_into().unwrap()).map_err(|e| {

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
            data: data
        });

        return Ok(handler);
    }

    fn alloc_irq_bit(self: &mut Self) -> Result<usize, sel4::Error> {
        let bit = bf_first_free(&self.allocated_bits).map_err(|_| sel4::Error::NotEnoughMemory)?;
        bf_set_bit(&mut self.allocated_bits, bit);
        return Ok(bit);
    }

    fn free_irq_bit(self: &mut Self, bit: usize) {
        bf_clr_bit(&mut self.allocated_bits, bit);
    }
}