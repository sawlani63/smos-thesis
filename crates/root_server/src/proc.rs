use crate::ut::{UTTable, UTWrapper};
use crate::frame_table::{FrameTable, FrameRef};
use crate::cspace::{CSpace, UserCSpace, CSpaceTrait};
use crate::vmem_layout;
use crate::page::PAGE_SIZE_4K;
use smos_common::util::BIT;
use crate::util::{alloc_retype, dealloc_retyped};
use crate::elf_load::load_elf;
use crate::mapping::map_frame;
use crate::connection::{Connection, Server};
use elf::ElfBytes;
use smos_common::error::InvocationError;
use smos_server::handle_arg::ServerReceivedHandleOrHandleCap;
use alloc::vec::Vec;
use alloc::rc::Rc;
use crate::window::Window;
use crate::view::View;
use core::cell::RefCell;
use crate::handle::RootServerResource;
use smos_server::syscalls::{ProcessSpawn, LoadComplete};
use smos_server::reply::SMOSReply;
use smos_common::local_handle;
use crate::object::AnonymousMemoryObject;
use smos_server::event::{INVOCATION_EP_BITS, FAULT_EP_BITS};
use smos_server::handle::{HandleInner, ServerHandle, HandleAllocater};
use smos_common::string::copy_terminated_rust_string_to_buffer;
use smos_common::util::{ROUND_UP, ROUND_DOWN};
use alloc::vec;
use alloc::string::String;
use byteorder::{ByteOrder, LittleEndian};

const LOADER_CONTENTS: &[u8] = include_bytes!(env!("LOADER_ELF"));

// @alwin: This should probably be unbounded
const MAX_PROCS: usize = 64;
pub const MAX_PID: usize = 1024;
const MAX_HANDLES: usize = 256;

const ARRAY_REPEAT_VALUE: Option<Rc<RefCell<UserProcess>>> = None;
static mut procs : [Option<Rc<RefCell<UserProcess>>>; MAX_PROCS] = [ARRAY_REPEAT_VALUE; MAX_PROCS];

pub fn procs_get(i: usize) -> &'static Option<Rc<RefCell<UserProcess>>> {
    unsafe {
        assert!(i < procs.len());
        return &procs[i];
    }
}

pub fn procs_get_mut(i: usize) -> &'static mut Option<Rc<RefCell<UserProcess>>>{
    unsafe {
        assert!(i < procs.len());
        return &mut procs[i];
    }
}

pub fn procs_set(i: usize, proc: Option<Rc<RefCell<UserProcess>>>) {
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

#[derive(Debug, Clone)]
pub struct UserProcess {
    pub tcb: (sel4::cap::Tcb, UTWrapper),
    pub pid: usize,
    pub vspace: (sel4::cap::VSpace, UTWrapper),
    ipc_buffer: (sel4::cap::SmallPage, FrameRef), // @alwin: Maybe make this a window/object/view
    pub shared_buffer: (sel4::cap::SmallPage, FrameRef), // @alwin: Maybe make this a window/object/view
    sched_context: (sel4::cap::SchedContext, UTWrapper),
    cspace: UserCSpace,
    stack: [(sel4::cap::SmallPage, FrameRef); vmem_layout::USER_DEFAULT_STACK_PAGES], // @alwin: Maybe make this a window/object/view
    fault_ep: sel4::cap::Endpoint,
    handle_table: [Option<ServerHandle<RootServerResource>>; 256],
    initial_windows: Vec<Rc<RefCell<Window>>>,
    windows: Vec<Rc<RefCell<Window>>>,
    pub views: Vec<Rc<RefCell<View>>>,
    pub actual_args: Option<Vec<String>>
    // pub connections: Vec<Rc<Connection>> // @alwin: This stores outgoing conns. Do we need to store incoming conns too?
}

impl HandleAllocater<RootServerResource> for UserProcess {
    fn handle_table_size(&self) -> usize {
        return self.handle_table.len();
    }

    fn handle_table(&self) -> &[Option<ServerHandle<RootServerResource>>] {
        return &self.handle_table
    }

    fn handle_table_mut(&mut self) -> &mut [Option<ServerHandle<RootServerResource>>] {
        return &mut self.handle_table
    }
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

        const ARRAY_REPEAT_VALUE: Option<ServerHandle<RootServerResource>> = None;
        return UserProcess {
            tcb: tcb, pid: pid, vspace: vspace, ipc_buffer: ipc_buffer,
            sched_context: sched_context, cspace: cspace,
            stack: stack, fault_ep: fault_ep, handle_table: [ARRAY_REPEAT_VALUE; 256],
            shared_buffer: shared_buffer, /* bfs_shared_buffer: None, */ initial_windows: initial_windows,
            windows: Vec::new(), views: Vec::new(), actual_args: None
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

    pub fn find_window_containing(&self, vaddr: usize) -> Option<Rc<RefCell<Window>>> {
        /* Do we need to check initial windows? */
        /* @ I think in most cases no but for the boot file server to forward vm faults, yes */
        for window in &self.initial_windows {
            if vaddr >= window.borrow().start && vaddr < window.borrow().start + window.borrow().size {
                return Some(window.clone());
            }
        }

        for window in &self.windows {
            if vaddr >= window.borrow().start && vaddr < window.borrow().start + window.borrow().size {
                return Some(window.clone());
            }
        }

        return None;
    }

    // @alwin: How can I make this more type-safe?
    pub fn add_window_unchecked(&mut self, window: Rc<RefCell<Window>>) {
        self.windows.push(window);
    }

    pub fn write_args_to_stack(&self, frame_table: &mut FrameTable, args: Option<Vec<&str>>) -> usize {
        let mut argv: Vec<u64> = Vec::new();
        let mut curr_stack_vaddr = vmem_layout::PROCESS_STACK_TOP;

        if args.is_some() {
            /* Write args to stack */
            for arg in args.as_ref().unwrap().iter() {
                if ROUND_UP(curr_stack_vaddr, sel4_sys::seL4_PageBits.try_into().unwrap()) !=
                   ROUND_UP(curr_stack_vaddr - (arg.as_bytes().len() + 1), sel4_sys::seL4_PageBits.try_into().unwrap()) {
                    /* @alwin: When part of an arg ends up on a different page to another part */
                    todo!();
                }

                curr_stack_vaddr = curr_stack_vaddr - (arg.as_bytes().len() + 1);
                argv.push(curr_stack_vaddr as u64);
                self.write_str_to_stack(frame_table, curr_stack_vaddr, arg);
            }

            /* pad to word alignment */
            curr_stack_vaddr = curr_stack_vaddr - (curr_stack_vaddr % 8);

            /* Write argv array to stack */
            curr_stack_vaddr = curr_stack_vaddr - (argv.len() * 8);
            self.write_words_to_stack(frame_table, curr_stack_vaddr, &argv);

            /* Write argv pointer to stack */
            let argv_ptr = curr_stack_vaddr as u64;
            curr_stack_vaddr = curr_stack_vaddr - 8;
            self.write_words_to_stack(frame_table, curr_stack_vaddr, &[argv_ptr]);
        } else {
            /* Write dummy argv pointer to stack */
            curr_stack_vaddr = curr_stack_vaddr - 8;
            self.write_words_to_stack(frame_table, curr_stack_vaddr, &[0]);
        }

        /* Write argc to stack */
        // @alwin: Do we need to word align?
        curr_stack_vaddr = curr_stack_vaddr - 4;
        self.write_half_words_to_stack(frame_table, curr_stack_vaddr, &[argv.len().try_into().unwrap()]);

        return curr_stack_vaddr;
    }

    fn write_str_to_stack(&self, frame_table: &FrameTable, vaddr: usize, string: &str) {
        let offset = vmem_layout::PROCESS_STACK_TOP - vaddr;
        let idx = vmem_layout::USER_DEFAULT_STACK_PAGES - (offset / PAGE_SIZE_4K) - 1;

        let frame_data = frame_table.frame_data(self.stack[idx].1);
        let offset_page = PAGE_SIZE_4K - (offset % PAGE_SIZE_4K);
        let string_len_with_null = string.as_bytes().len() + 1;

        copy_terminated_rust_string_to_buffer(&mut frame_data[offset_page..offset_page + string_len_with_null], string);
    }

    fn write_words_to_stack(&self, frame_table: &FrameTable, vaddr: usize, words: &[u64]) {
        let offset = vmem_layout::PROCESS_STACK_TOP - vaddr;
        let idx = vmem_layout::USER_DEFAULT_STACK_PAGES - (offset / PAGE_SIZE_4K) - 1;

        let frame_data = frame_table.frame_data(self.stack[idx].1);
        let offset_page = PAGE_SIZE_4K - (offset % PAGE_SIZE_4K);
        let bytes_length = words.len() * 8;

        LittleEndian::write_u64_into(words, &mut frame_data[offset_page..offset_page + bytes_length]);
    }

    fn write_half_words_to_stack(&self, frame_table: &FrameTable, vaddr: usize, data: &[u32]) {
        let offset = vmem_layout::PROCESS_STACK_TOP - vaddr;
        let idx = vmem_layout::USER_DEFAULT_STACK_PAGES - (offset / PAGE_SIZE_4K) - 1;

        let frame_data = frame_table.frame_data(self.stack[idx].1);
        let offset_page = PAGE_SIZE_4K - (offset % PAGE_SIZE_4K);
        let bytes_length = data.len() * 4;

        LittleEndian::write_u32_into(data, &mut frame_data[offset_page..offset_page + bytes_length]);
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
                       elf_data: &[u8], args: Option<Vec<&str>>) -> Result<Rc<RefCell<UserProcess>>, sel4::Error> {

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
    let (mut sp, stack_pages) = init_process_stack(cspace, ut_table, frame_table, vspace.0).map_err(|e| {
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

    let proc = Rc::new( RefCell::new( UserProcess::new(tcb, pos, vspace, ipc_buffer,
                                                                sched_context, proc_cspace,
                                                                stack_pages, fault_ep,
                                                                shared_buffer, initial_windows)));

    sp = proc.borrow_mut().write_args_to_stack(frame_table, args);

    let mut user_context = sel4::UserContext::default();
    *user_context.pc_mut() = elf.ehdr.e_entry;
    *user_context.sp_mut() = sp.try_into().unwrap();

    // @alwin: Clean up the stack if this fails
    tcb.0.tcb_write_registers(true, 2, &mut user_context)?;

    procs_set(pos, Some(proc.clone()));

    return Ok(proc);
}


pub fn handle_process_spawn(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable,
                            sched_control: sel4::cap::SchedControl, ep: sel4::cap::Endpoint,
                            p: &mut UserProcess, args: ProcessSpawn)
                            -> Result<SMOSReply, InvocationError> {

    let (idx, handle_ref) = p.allocate_handle()?;

    let loader_args = Some(vec!{args.exec_name.as_str(), args.fs_name.as_str()});
    // @alwin: This is a lazy way of handling the error;
    let proc = start_process(cspace, ut_table, frame_table, sched_control,
                             &args.exec_name, ep, LOADER_CONTENTS, loader_args)
                            .map_err(|_| InvocationError::InsufficientResources)?;
    proc.borrow_mut().actual_args = args.args;

    *handle_ref = Some(ServerHandle::new(RootServerResource::Process(proc)));

    return Ok(SMOSReply::ProcessSpawn{hndl: local_handle::LocalHandle::new(idx)});
}

pub fn handle_load_complete(cspace: &mut CSpace, frame_table: &mut FrameTable, p: &mut UserProcess,
                            args: LoadComplete) -> Result<SMOSReply, InvocationError> {

    for window in &p.initial_windows {
        /* Clean up the memory object by freeing the frames in it */
        for frame in window.borrow_mut().bound_view.as_ref().unwrap()
                  .borrow_mut().bound_object.as_ref().unwrap()
                  .borrow_mut().frames
        {
            if frame.is_some() {
                cspace.root_cnode.relative(frame.as_ref().unwrap().0).revoke();
                frame_table.free_frame(frame.as_ref().unwrap().1);
            }
        }

        /* The revoke above should delete these caps, just need to free the slots */
        for cap in window.borrow_mut().bound_view.as_ref().unwrap().borrow_mut().caps {
            if cap.is_some() {
                cspace.free_cap(cap.unwrap());
            }
        }
    }

    // The objects and the views should be cleaned up by doing this since they are RCs that should
    // not be referenced anywhere else.
    p.initial_windows.clear();

    /* Reset the stack */
    for stack_page in p.stack {
        let frame_data = frame_table.frame_data(stack_page.1);
        frame_data[0..4096].fill(0);
    }

    let proc_args = match &p.actual_args {
        None => None,
        Some(x) => Some(x.iter().map(|z| z.as_str()).collect())
    };

    let sp = p.write_args_to_stack(frame_table, proc_args);
    p.actual_args = None;

    let mut user_context = sel4::UserContext::default();
    *user_context.pc_mut() = args.entry_point as u64;
    *user_context.sp_mut() = sp as u64;

    // @alwin: deal with error case properly here
    p.tcb.0.tcb_write_registers(true, 2, &mut user_context).expect("@alwin: This shouldn't be an assert");

    return Ok(SMOSReply::LoadComplete);
}
