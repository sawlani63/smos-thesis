use crate::ut::{UTTable, UTWrapper};
use crate::frame_table::{FrameTable, FrameRef};
use crate::cspace::{CSpace, UserCSpace, CSpaceTrait};
use crate::vmem_layout;
use crate::page::{PAGE_SIZE_4K, BIT};
use crate::util::{alloc_retype,  INVOCATION_EP_BITS, FAULT_EP_BITS, dealloc_retyped};
use crate::elf_load::load_elf;
use crate::mapping::map_frame;
use crate::handle::Handle;
use crate::connection::Connection;
use elf::ElfBytes;
use smos_common::error::InvocationError;
use smos_server::handle_arg::HandleOrUnwrappedHandleCap;
use alloc::vec::Vec;
use alloc::rc::Rc;
use crate::window::Window;
use crate::view::View;
use core::cell::RefCell;
use crate::handle::SMOSObject;

// @alwin: This should probably be unbounded
const MAX_PROCS: usize = 64;
pub const MAX_PID: usize = 1024;
const MAX_HANDLES: usize = 256;

const ARRAY_REPEAT_VALUE: Option<UserProcess> = None;
static mut procs : [Option<UserProcess>; MAX_PROCS] = [ARRAY_REPEAT_VALUE; MAX_PROCS];

pub fn procs_get(i: usize) -> &'static Option<UserProcess> {
    unsafe {
        assert!(i < procs.len());
        return &procs[i];
    }
}

pub fn procs_get_mut(i: usize) -> &'static mut Option<UserProcess> {
    unsafe {
        assert!(i < procs.len());
        return &mut procs[i];
    }
}

pub fn procs_set(i: usize, proc: Option<UserProcess>) {
    unsafe {
        assert!(i < procs.len());
        procs[i] = proc;
    }
}

pub fn find_free_proc() -> Option<usize> {
    for i in 0..(unsafe { procs.len() }) {
        if procs_get(i).is_none() {
            return Some(i)
        }
    }

    return None;
}

#[derive(Clone)]
pub struct UserProcess {
    tcb: (sel4::cap::Tcb, UTWrapper),
    pub pid: usize,
    vspace: (sel4::cap::VSpace, UTWrapper),
    ipc_buffer: (sel4::cap::SmallPage, FrameRef),
    pub shared_buffer: (sel4::cap::SmallPage, FrameRef),
    sched_context: (sel4::cap::SchedContext, UTWrapper),
    cspace: UserCSpace,
    stack: [(sel4::cap::SmallPage, FrameRef); vmem_layout::USER_DEFAULT_STACK_PAGES],
    fault_ep: sel4::cap::Endpoint,
    handle_table: [Option<Handle>; 256],
    initial_windows: Vec<Rc<RefCell<Window>>>,
    windows: Vec<Rc<RefCell<Window>>>,
    pub views: Vec<Rc<RefCell<View>>>,
    // pub connections: Vec<Rc<Connection>> // @alwin: This stores outgoing conns. Do we need to store incoming conns too?
}

impl UserProcess {
    pub fn new(
        tcb: (sel4::cap::Tcb, UTWrapper),
        pid: usize,
        vspace: (sel4::cap::VSpace, UTWrapper),
        ipc_buffer: (sel4::cap::SmallPage, FrameRef),
        sched_context: (sel4::cap::SchedContext, UTWrapper),
        cspace: UserCSpace,
        stack: [(sel4::cap::SmallPage, FrameRef); vmem_layout::USER_DEFAULT_STACK_PAGES],
        fault_ep: sel4::cap::Endpoint,
        shared_buffer: (sel4::cap::SmallPage, FrameRef),
        initial_windows: Vec<Rc<RefCell<Window>>>,
    )  -> UserProcess {

        const ARRAY_REPEAT_VALUE: Option<Handle> = None;
        return UserProcess {
            tcb: tcb, pid: pid, vspace: vspace, ipc_buffer: ipc_buffer,
            sched_context: sched_context, cspace: cspace,
            stack: stack, fault_ep: fault_ep, handle_table: [ARRAY_REPEAT_VALUE; 256],
            shared_buffer: shared_buffer, initial_windows: initial_windows, windows: Vec::new(),
            views: Vec::new()
            /* connections: Vec::new() */
        };
    }

    // @alwin: This should be easy to keep sorted (makes it faster to check if window overlaps)
    pub fn overlapping_window(&self, start: usize, size: usize) -> bool {
        for window in &self.windows {
            let window_borrowed = window.borrow();
            if (start >= window_borrowed.start && start < window_borrowed.start + window_borrowed.size) ||
               (start + size >= window_borrowed.start && start + size < window_borrowed.start + window_borrowed.size ) {

                return true;
            }
        }

        return false;
    }

    // @alwin: How can I make this more type-safe?
    pub fn add_window_unchecked(&mut self, window: Rc<RefCell<Window>>) {
        self.windows.push(window);
    }

    pub fn allocate_handle<'a>(&'a mut self) -> Result<(usize, &'a mut Option<Handle>), InvocationError> {
        // @alwin: This can be made more efficient with bitmaps or some smarter allocation strategy
        // but I don't really care for now.
        for (i, handle) in self.handle_table.iter_mut().enumerate() {
            if (handle.is_none()) {
                return Ok((i, handle));
            }
        }

        return Err(InvocationError::OutOfHandles);
    }

    pub fn get_handle_mut<'a>(&'a mut self, idx: usize) -> Result<&'a mut Option<Handle>, ()> {
        // @alwin: const value here instead
        if idx >= 256 {
            return Err(())
        }
        return Ok(&mut self.handle_table[idx]);
    }

    pub fn cleanup_handle(&mut self, idx: usize) -> Result<(), ()> {
        if idx >= 256 {
            return Err(());
        }
        self.handle_table[idx] = None;
        Ok(())
    }
}

fn init_process_stack(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable,
                      vspace: sel4::cap::VSpace)
                      -> Result<
                                (
                                    usize,
                                    [(sel4::cap::SmallPage, FrameRef); vmem_layout::USER_DEFAULT_STACK_PAGES]
                                ),
                                sel4::Error
                               > {

    let stack_top = vmem_layout::PROCESS_STACK_TOP;
    let mut stack_bottom = stack_top - PAGE_SIZE_4K;
    let mut stack_pages : [Option<(sel4::cap::SmallPage, FrameRef)>; vmem_layout::USER_DEFAULT_STACK_PAGES] = [None; vmem_layout::USER_DEFAULT_STACK_PAGES];

    // @alwin: This leaks memory on failure
    for i in 0..vmem_layout::USER_DEFAULT_STACK_PAGES {
        let frame = frame_table.alloc_frame(cspace, ut_table).ok_or(sel4::Error::NotEnoughMemory)?;
        let user_frame = sel4::CPtr::from_bits(cspace.alloc_slot()?.try_into().unwrap()).cast::<sel4::cap_type::SmallPage>();
        cspace.root_cnode().relative(user_frame).copy(&cspace.root_cnode().relative(frame_table.frame_from_ref(frame).get_cap()),
                                                      sel4::CapRightsBuilder::all().build());
        map_frame(cspace, ut_table, user_frame.cast(), vspace, stack_bottom,
                  sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None);

        stack_pages[vmem_layout::USER_DEFAULT_STACK_PAGES - (i + 1)] = Some((user_frame, frame));
        stack_bottom -= PAGE_SIZE_4K;
    }


    return Ok((stack_top, stack_pages.map(|x| x.unwrap())));
}

// @alwin: this leaks cslots and caps!
pub fn start_process(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable,
                       sched_control: sel4::cap::SchedControl, name: &str, ep: sel4::cap::Endpoint,
                       elf_data: &[u8]) -> Result<usize, sel4::Error> {

    /* We essentially use the position in the table as the pid. Don't think this is the right way to 
       do it properly */
    let pos = find_free_proc().ok_or(sel4::Error::NotEnoughMemory)?;

    /* Create a VSpace */
    let mut vspace = alloc_retype::<sel4::cap_type::VSpace>(cspace, ut_table, sel4::ObjectBlueprint::Arch(
                                                        sel4::ObjectBlueprintArch::SeL4Arch(
                                                        sel4::ObjectBlueprintAArch64::VSpace)))?;

    /* assign the vspace to an asid pool */
    sel4::init_thread::slot::ASID_POOL.cap().asid_pool_assign(vspace.0).map_err(|e| {
        err_rs!("Failed to assign vspace to ASID pool");
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Create a simple 1 level CSpace */
    let mut proc_cspace = UserCSpace::new(cspace, ut_table, false)?;

    /* Allocate a frame for the IPC buffer */
    let ipc_buffer_ref = frame_table.alloc_frame(cspace, ut_table)
                                    .ok_or(sel4::Error::NotEnoughMemory)
                                    .map_err(|e| {
        err_rs!("Failed to allocate frame for ipc buffer");
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Allocate a slot to hold cap used for the user mapping*/
    let ipc_buffer_slot = cspace.alloc_slot().map_err(|e| {
        err_rs!("Failed to allocate CNode slot for IPC buffer");
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Copy the root server's copy of the frame cap into the user mapping slot */
    let ipc_buffer_cap = sel4::CPtr::from_bits(ipc_buffer_slot.try_into().unwrap()).cast::<sel4::cap_type::SmallPage>();
    cspace.root_cnode().relative(ipc_buffer_cap).copy(&cspace.root_cnode().relative(frame_table.frame_from_ref(ipc_buffer_ref).get_cap()),
                                                      sel4::CapRightsBuilder::all().build()).map_err(|e| {

        err_rs!("Failed to copy frame cap for user ipc buffer mapping");
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    });
    let ipc_buffer = (ipc_buffer_cap, ipc_buffer_ref);

    /* allocate a new slot in the target cspace which we will mint a badged endpoint cap into --
     * the badge is used to identify the process */
     let proc_ep = proc_cspace.alloc_slot().map_err(|e| {
        err_rs!("Failed to allocate slot for user endpoint cap");
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
     })?;
     // Make sure the slot selected is what the runtime expects
     assert!(proc_ep == smos_common::init::InitCNodeSlots::SMOS_RootServerEP as usize);

     /* now mutate the cap, thereby setting the badge */
     proc_cspace.root_cnode().relative_bits_with_depth(proc_ep.try_into().unwrap(), sel4::WORD_SIZE)
                             .mint(&cspace.root_cnode().relative(ep),
                                   sel4::CapRightsBuilder::all().build(),
                                   (pos | INVOCATION_EP_BITS).try_into().unwrap()).map_err(|e| {

        err_rs!("Failed to mint user endpoint cap");
        proc_cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Allocate a slot for a self-referential cspace cap */
    let proc_self_cspace = proc_cspace.alloc_slot().map_err(|e| {
        err_rs!("Failed to allocate slot for self-referential cap");
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;
    // Make sure the slot selected is what the runtime expects
    assert!(proc_self_cspace == smos_common::init::InitCNodeSlots::SMOS_CNodeSelf as usize);

     /* Copy the CNode cap into the new process cspace*/
    proc_cspace.root_cnode().relative_bits_with_depth(proc_self_cspace.try_into().unwrap(), sel4::WORD_SIZE)
                            .copy(&cspace.root_cnode().relative(proc_cspace.root_cnode()),
                                  sel4::CapRightsBuilder::all().build()).map_err(|e| {

        err_rs!("Failed to copy self-refernetial cnode cap");
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;


    /* Create a new TCB object */
    let mut tcb = alloc_retype::<sel4::cap_type::Tcb>(cspace, ut_table, sel4::ObjectBlueprint::Tcb)
                                                     .map_err(|e| {

        err_rs!("Failed to allocate new TCB object");
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
     })?;


    /* Configure the TCB */
    // @alwin: changing the WORD_SIZE to 64 causes a panic - not important but maybe understand why?
    tcb.0.tcb_configure(proc_cspace.root_cnode(), sel4::CNodeCapData::new(0, 0),
                        vspace.0, vmem_layout::PROCESS_IPC_BUFFER.try_into().unwrap(),
                        ipc_buffer.0).map_err(|e| {

        err_rs!("Failed to configure TCB");
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Create scheduling context */
    let mut sched_context = alloc_retype::<sel4::cap_type::SchedContext>(cspace,
                                                                         ut_table,
                                                                         sel4::ObjectBlueprint::SchedContext{ size_bits: sel4_sys::seL4_MinSchedContextBits.try_into().unwrap()})
                                                                         .map_err(|e| {
        err_rs!("Failed to create scheduling context");
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Configure the scheduling context to use the first core with budget equal to period */
    sched_control.sched_control_configure_flags(sched_context.0, 1000, 1000, 0, 0, 0).map_err(|e| {
        err_rs!("Failed to configure scheduling context");
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Allocate a slot for a badged fault endpoint capability */
    let fault_ep = cspace.alloc_cap::<sel4::cap_type::Endpoint>().map_err(|e| {
        err_rs!("Failed to allocate slot for fault endpoint");
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Mint the badged fault EP capability into the slot */
    cspace.root_cnode().relative(fault_ep).mint(&cspace.root_cnode().relative(ep),
                                                sel4::CapRightsBuilder::all().build(),
                                                (pos | FAULT_EP_BITS).try_into().unwrap()).map_err(|e| {

        err_rs!("Failed to mint badged fault endpoint");
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Set up the TCB scheduling parameters */
    tcb.0.tcb_set_sched_params(sel4::init_thread::slot::TCB.cap(), 0, 0, sched_context.0, fault_ep)
         .map_err(|e| {

        err_rs!("Failed to sceduling parameters");
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
     });

    tcb.0.debug_name(name.as_bytes());

    // @alwin: I've just included a bytearray containing the elf contents, which lets me avoid any
    // CPIO archive stuff, but is a bit more rigid.
    let elf = ElfBytes::<elf::endian::AnyEndian>::minimal_parse(elf_data)
                                                               .or(Err(sel4::Error::InvalidArgument))
                                                               .map_err(|e| {
        err_rs!("Failed to parse ELF file");
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
   })?;

    /* Load the ELF file into the virtual address space */
    let initial_windows = load_elf(cspace, ut_table, frame_table, vspace.0, &elf).map_err(|e| {
        err_rs!("Failed to load ELF file");
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Map the IPC buffer into the virtual address space */
    map_frame(cspace, ut_table, ipc_buffer.0.cast(), vspace.0, vmem_layout::PROCESS_IPC_BUFFER,
              sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None).map_err(|e| {

        err_rs!("Failed to set IPC buffer");
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    // @alwin: This should probably be done properly w.r.t. windows and stuff. Maybe we don't need
    // memory objects, but we should at least make a window so a client can't create a window
    // on top of a region that is predefined by the process initialization.

    /* Allocate a frame for the shared page used for communication between this process and the root server */
    let shared_buffer_ref = frame_table.alloc_frame(cspace, ut_table)
                                       .ok_or(sel4::Error::NotEnoughMemory)
                                       .map_err(|e| {
        err_rs!("Failed to allocate frame for shared buffer between RS and proc");
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Allocate a slot to hold cap used for the user mapping*/
    let shared_buffer_slot = cspace.alloc_slot().map_err(|e| {
        err_rs!("Failed to allocate CNode slot for shared buffer");
        frame_table.free_frame(shared_buffer_ref);
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Copy the root server's copy of the frame cap into the user mapping slot */
    let shared_buffer_cap = sel4::CPtr::from_bits(shared_buffer_slot.try_into().unwrap()).cast::<sel4::cap_type::SmallPage>();
    cspace.root_cnode().relative(shared_buffer_cap).copy(&cspace.root_cnode().relative(frame_table.frame_from_ref(shared_buffer_ref).get_cap()),
                                                      sel4::CapRightsBuilder::all().build()).map_err(|e| {

        err_rs!("Failed to copy frame cap for user shared buffer mapping");
        cspace.free_slot(shared_buffer_slot);
        frame_table.free_frame(shared_buffer_ref);
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    });
    let shared_buffer = (shared_buffer_cap, shared_buffer_ref);

    /* Map in the shared page used for communication between this process and the root server */
    map_frame(cspace, ut_table, shared_buffer.0.cast(), vspace.0, vmem_layout::PROCESS_RS_DATA_TRANSFER_PAGE,
              sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None).map_err(|e| {

        err_rs!("Failed to map shared buffer");
        cspace.delete(shared_buffer_slot);
        cspace.free_slot(shared_buffer_slot);
        frame_table.free_frame(shared_buffer_ref);
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Set up the process stack */
    let (sp, stack_pages) = init_process_stack(cspace, ut_table, frame_table, vspace.0).map_err(|e| {
        err_rs!("Failed to initialize stack");
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace);
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    let mut user_context = sel4::UserContext::default();
    *user_context.pc_mut() = elf.ehdr.e_entry;
    *user_context.sp_mut() = sp.try_into().unwrap();

    // @alwin: Clean up the stack if this fails
    tcb.0.tcb_write_registers(true, 2, &mut user_context)?;

    procs_set(pos, Some(UserProcess::new(tcb, pos, vspace, ipc_buffer, sched_context,
                                         proc_cspace, stack_pages, fault_ep, shared_buffer, initial_windows)));

    return Ok(pos);
}
