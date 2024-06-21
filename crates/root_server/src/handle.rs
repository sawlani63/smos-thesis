use crate::cspace::{CSpace, CSpaceTrait};
use alloc::vec::Vec;
use alloc::rc::Rc;
use core::cell::RefCell;
use crate::window::Window;
use crate::object::AnonymousMemoryObject;
use crate::view::View;
use crate::connection::{Connection, Server};
use crate::proc::UserProcess;
use crate::ut::UTWrapper;
use smos_server::handle::HandleInner;
use smos_server::handle_capability::HandleCapability;

const MAX_HANDLE_CAPS: usize = 256;

pub fn create_handle_cap_table(cspace: &mut CSpace, ep: sel4::cap::Endpoint) -> Result<Vec<HandleCapability<RootServerResource>>, sel4::Error>{
    let mut vec: Vec<HandleCapability<RootServerResource>> = Vec::new();

    for i in 0..MAX_HANDLE_CAPS {
        let tmp = cspace.alloc_slot()?;

        // @alwin: Think more about what badge these get. Maybe OR them with some handle cap bit
        // so they can't be spoofed from normal endpoint caps
        cspace.root_cnode().relative_bits_with_depth(tmp.try_into().unwrap(), sel4::WORD_SIZE)
                           .mint(&cspace.root_cnode().relative(ep),
                                 sel4::CapRightsBuilder::none().build(), i.try_into().unwrap());

        vec.push(HandleCapability {
            handle: None,
            root_cap: Some(cspace.root_cnode().relative_bits_with_depth(tmp.try_into().unwrap(), sel4::WORD_SIZE))
        });
    }

    Ok(vec)
}

#[derive(Debug, Clone)]
pub enum RootServerResource {
    Window(Rc<RefCell<Window>>),
    Object(Rc<RefCell<AnonymousMemoryObject>>),
    ConnRegistration(Rc<RefCell<Connection>>),
    WindowRegistration(Rc<RefCell<View>>),
    View(Rc<RefCell<View>>),
    Connection(Rc<RefCell<Connection>>), // Does this need a refcell?
    Server(Rc<RefCell<Server>>),
    Process(Rc<RefCell<UserProcess>>),
    Reply((sel4::cap::Reply, UTWrapper))
}

impl HandleInner for RootServerResource {}
