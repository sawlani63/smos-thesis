use crate::cspace::{CSpace, CSpaceTrait, UserCSpace};
use crate::elf_load::load_elf;
use crate::frame_table::{FrameRef, FrameTable};
use crate::handle::RootServerResource;
use crate::mapping::map_frame;
use crate::object::{handle_obj_destroy_internal, AnonymousMemoryObject};
use crate::page::PAGE_SIZE_4K;
use crate::ut::{UTTable, UTWrapper};
use crate::util::{alloc_retype, dealloc_retyped};
use crate::view::{handle_unview_internal, View};
use crate::vmem_layout::{self, STACK_PAGES};
use crate::window::handle_window_destroy_internal;
use crate::window::Window;
use crate::RSReplyWrapper;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use byteorder::{ByteOrder, LittleEndian};
use core::cell::RefCell;
use elf::ElfBytes;
use smos_common::error::InvocationError;
use smos_common::local_handle;
use smos_common::obj_attributes::ObjAttributes;
use smos_common::string::copy_terminated_rust_string_to_buffer;
use smos_common::util::ROUND_UP;
use smos_server::event::{FAULT_EP_BITS, INVOCATION_EP_BITS};
use smos_server::handle::{HandleAllocater, ServerHandle};
use smos_server::handle_capability::HandleCapabilityTable;
use smos_server::reply::{handle_reply, SMOSReply};
use smos_server::syscalls::{LoadComplete, ProcessSpawn, ProcessWait};

const LOADER_CONTENTS: &[u8] = include_bytes!(env!("LOADER_ELF"));

// @alwin: This should probably be unbounded
const MAX_PROCS: usize = 64;
const MAX_HANDLES: usize = 256;

#[derive(Debug)]
pub enum ProcessType {
    ActiveProcess(UserProcess),
    ZombieProcess(usize), // @alwin: This should probably store the return code
                          // @alwin: What happens to orphans? Does it become the responsibility of the root server
                          // to adopt and do a periodic sweep to reap them?
}

const ARRAY_REPEAT_VALUE: Option<Rc<RefCell<ProcessType>>> = None;
static mut PROCS: [Option<Rc<RefCell<ProcessType>>>; MAX_PROCS] = [ARRAY_REPEAT_VALUE; MAX_PROCS];

pub fn procs_get(i: usize) -> &'static Option<Rc<RefCell<ProcessType>>> {
    unsafe {
        assert!(i < PROCS.len());
        return &PROCS[i];
    }
}

pub fn procs_get_mut(i: usize) -> &'static mut Option<Rc<RefCell<ProcessType>>> {
    unsafe {
        assert!(i < PROCS.len());
        return &mut PROCS[i];
    }
}

pub fn procs_set(i: usize, proc: Option<Rc<RefCell<ProcessType>>>) {
    unsafe {
        assert!(i < PROCS.len());
        PROCS[i] = proc;
    }
}

pub fn find_free_proc() -> Option<usize> {
    for i in 0..(unsafe { PROCS.len() }) {
        if procs_get(i).is_none() {
            return Some(i);
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
    fault_ep: sel4::cap::Endpoint,
    handle_table: [Option<ServerHandle<RootServerResource>>; MAX_HANDLES],
    pub created_handle_caps: Vec<usize>, // @alwin: this is a temporary hack, but doing it as Vec<HandleCapability> completely screws up the generic handle abstraction I have
    initial_windows: Vec<Rc<RefCell<Window>>>,
    windows: Vec<Rc<RefCell<Window>>>,
    pub views: Vec<Rc<RefCell<View>>>,
    pub waiter: Option<RSReplyWrapper>,
    // pub connections: Vec<Rc<Connection>> // @alwin: This stores outgoing conns. Do we need to store incoming conns too?
}

impl HandleAllocater<RootServerResource> for UserProcess {
    fn handle_table_size(&self) -> usize {
        return self.handle_table.len();
    }

    fn handle_table(&self) -> &[Option<ServerHandle<RootServerResource>>] {
        return &self.handle_table;
    }

    fn handle_table_mut(&mut self) -> &mut [Option<ServerHandle<RootServerResource>>] {
        return &mut self.handle_table;
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
        fault_ep: sel4::cap::Endpoint,
        shared_buffer: (sel4::cap::SmallPage, FrameRef),
        initial_windows: Vec<Rc<RefCell<Window>>>,
    ) -> UserProcess {
        const HNDL_REPEAT_VALUE: Option<ServerHandle<RootServerResource>> = None;
        return UserProcess {
            tcb: tcb,
            pid: pid,
            vspace: vspace,
            ipc_buffer: ipc_buffer,
            sched_context: sched_context,
            cspace: cspace,
            fault_ep: fault_ep,
            handle_table: [HNDL_REPEAT_VALUE; 256],
            created_handle_caps: Vec::new(),
            shared_buffer: shared_buffer,
            /* bfs_shared_buffer: None, */ initial_windows: initial_windows,
            windows: Vec::new(),
            views: Vec::new(), /* connections: Vec::new() */
            waiter: None,
        };
    }

    pub fn destroy(
        &mut self,
        cspace: &mut CSpace,
        ut_table: &mut UTTable,
        frame_table: &mut FrameTable,
        _handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
    ) {
        /* Clean up the handle table */
        for handle in &self.handle_table {
            if handle.is_none() {
                continue;
            }

            match handle.as_ref().unwrap().inner() {
                RootServerResource::Window(win) => {
                    handle_window_destroy_internal(cspace, win.clone(), false);
                }
                RootServerResource::Object(obj) => {
                    handle_obj_destroy_internal(cspace, frame_table, obj.clone(), true);
                }
                RootServerResource::ConnRegistration(_) => {
                    todo!()
                }
                RootServerResource::WindowRegistration(_) => {
                    todo!()
                }
                RootServerResource::View(view) => {
                    handle_unview_internal(cspace, view.clone());
                }
                RootServerResource::Connection(_) => {
                    todo!()
                }
                RootServerResource::Server(_) => {
                    todo!()
                }
                RootServerResource::Process(_) => {
                    todo!()
                }
                RootServerResource::Reply(_) => {
                    todo!()
                }
                RootServerResource::HandleCap(_) => {
                    todo!()
                }
                RootServerResource::IRQRegistration(_) => {
                    todo!()
                }
                RootServerResource::ChannelAuthority(_) => {
                    todo!()
                }
            }
        }

        self.handle_table.fill(None);

        /* @alwin: Do the same thing for the handle cap table */

        dealloc_retyped(cspace, ut_table, self.sched_context);

        dealloc_retyped(cspace, ut_table, self.tcb);

        dealloc_retyped(cspace, ut_table, self.vspace);

        /* @alwin: Should this just be a window/view/obj */
        cspace
            .delete_cap(self.ipc_buffer.0)
            .expect("Failed to delete IPC buffer");
        frame_table.free_frame(self.ipc_buffer.1);

        /* @alwin: Should this just be a window/view/obj */
        cspace
            .delete_cap(self.shared_buffer.0)
            .expect("Failed to delete shared buffer");
        frame_table.free_frame(self.shared_buffer.1);

        cspace
            .delete_cap(self.fault_ep)
            .expect("Failed to delete fault endpoint");

        self.cspace.destroy(cspace, ut_table);
    }

    // @alwin: This should be easy to keep sorted (makes it faster to check if window overlaps)
    pub fn overlapping_window(&self, start: usize, size: usize) -> Option<Rc<RefCell<Window>>> {
        for window in &self.windows {
            let window_borrowed = window.borrow();
            if (start >= window_borrowed.start
                && start < window_borrowed.start + window_borrowed.size)
                || (start + size > window_borrowed.start
                    && start + size < window_borrowed.start + window_borrowed.size)
            {
                return Some(window.clone());
            }
        }

        return None;
    }

    pub fn find_window_containing(&self, vaddr: usize) -> Option<Rc<RefCell<Window>>> {
        /* Do we need to check initial windows? */
        /* @ I think in most cases no but for the boot file server to forward vm faults, yes */
        for window in &self.initial_windows {
            if vaddr >= window.borrow().start
                && vaddr < window.borrow().start + window.borrow().size
            {
                return Some(window.clone());
            }
        }

        for window in &self.windows {
            if vaddr >= window.borrow().start
                && vaddr < window.borrow().start + window.borrow().size
            {
                return Some(window.clone());
            }
        }

        return None;
    }

    pub fn add_window_unchecked(&mut self, window: Rc<RefCell<Window>>) {
        self.windows.push(window);
    }

    pub fn remove_window(&mut self, window: Rc<RefCell<Window>>) {
        let pos = self.windows.iter().position(|x| Rc::ptr_eq(x, &window));
        match pos {
            Some(x) => {
                self.windows.swap_remove(x);
            }
            None => {}
        }
    }

    pub fn write_args_to_stack(
        &self,
        frame_table: &mut FrameTable,
        stack: Rc<RefCell<Window>>,
        loader_args: Option<Vec<&str>>,
        exec_args: Option<Vec<&str>>,
    ) -> usize {
        let mut argv: Vec<u64> = Vec::new();
        let mut envp: Vec<u64> = Vec::new();
        let mut curr_stack_vaddr = vmem_layout::PROCESS_STACK_TOP;

        let envp_ptr = {
            /* envp looks like the following */
            /* [STACK_TOP, IPC_BUFFER_ADDR, RS_SHARED_BUF, NULL] */
            curr_stack_vaddr = curr_stack_vaddr - 8;
            self.write_words_to_stack(
                frame_table,
                &stack,
                curr_stack_vaddr,
                &[crate::vmem_layout::PROCESS_STACK_TOP as u64],
            );
            envp.push(curr_stack_vaddr as u64);

            curr_stack_vaddr = curr_stack_vaddr - 8;
            self.write_words_to_stack(
                frame_table,
                &stack,
                curr_stack_vaddr,
                &[crate::vmem_layout::PROCESS_IPC_BUFFER as u64],
            );
            envp.push(curr_stack_vaddr as u64);

            curr_stack_vaddr = curr_stack_vaddr - 8;
            self.write_words_to_stack(
                frame_table,
                &stack,
                curr_stack_vaddr,
                &[crate::vmem_layout::PROCESS_RS_DATA_TRANSFER_PAGE as u64],
            );
            envp.push(curr_stack_vaddr as u64);

            /* Add null terminator to envp */
            envp.push(0);

            curr_stack_vaddr = curr_stack_vaddr - (envp.len() * 8);
            self.write_words_to_stack(frame_table, &stack, curr_stack_vaddr, &envp);

            curr_stack_vaddr as u64
        };

        let argv_ptr = if loader_args.is_some() || exec_args.is_some() {
            if loader_args.is_some() {
                for arg in loader_args.as_ref().unwrap().iter() {
                    if ROUND_UP(
                        curr_stack_vaddr,
                        sel4_sys::seL4_PageBits.try_into().unwrap(),
                    ) != ROUND_UP(
                        curr_stack_vaddr - (arg.as_bytes().len() + 1),
                        sel4_sys::seL4_PageBits.try_into().unwrap(),
                    ) {
                        /* @alwin: When part of an arg ends up on a different page to another part */
                        todo!();
                    }

                    curr_stack_vaddr = curr_stack_vaddr - (arg.as_bytes().len() + 1);
                    argv.push(curr_stack_vaddr as u64);
                    self.write_str_to_stack(frame_table, &stack, curr_stack_vaddr, arg);
                }
            }

            if exec_args.is_some() {
                for arg in exec_args.as_ref().unwrap().iter() {
                    if ROUND_UP(
                        curr_stack_vaddr,
                        sel4_sys::seL4_PageBits.try_into().unwrap(),
                    ) != ROUND_UP(
                        curr_stack_vaddr - (arg.as_bytes().len() + 1),
                        sel4_sys::seL4_PageBits.try_into().unwrap(),
                    ) {
                        /* @alwin: When part of an arg ends up on a different page to another part */
                        todo!();
                    }

                    curr_stack_vaddr = curr_stack_vaddr - (arg.as_bytes().len() + 1);
                    argv.push(curr_stack_vaddr as u64);
                    self.write_str_to_stack(frame_table, &stack, curr_stack_vaddr, arg);
                }
            }

            /* pad to word alignment */
            curr_stack_vaddr = curr_stack_vaddr - (curr_stack_vaddr % 8);

            /* Write argv array to stack */
            curr_stack_vaddr = curr_stack_vaddr - (argv.len() * 8);
            self.write_words_to_stack(frame_table, &stack, curr_stack_vaddr, &argv);

            /* Write argv pointer to stack */
            curr_stack_vaddr as u64
        } else {
            0
            /* Write dummy argv pointer to stack */
        };

        /* Write ptr to envp to stack */
        curr_stack_vaddr = curr_stack_vaddr - 8;
        self.write_words_to_stack(frame_table, &stack, curr_stack_vaddr, &[envp_ptr]);

        /* Write ptr to argv to stack */
        curr_stack_vaddr = curr_stack_vaddr - 8;
        self.write_words_to_stack(frame_table, &stack, curr_stack_vaddr, &[argv_ptr]);

        /* Write argc to stack */
        curr_stack_vaddr = curr_stack_vaddr - 4;
        self.write_half_words_to_stack(
            frame_table,
            &stack,
            curr_stack_vaddr,
            &[argv.len().try_into().unwrap()],
        );

        return curr_stack_vaddr;
    }

    fn write_str_to_stack(
        &self,
        frame_table: &FrameTable,
        stack_win: &Rc<RefCell<Window>>,
        vaddr: usize,
        string: &str,
    ) {
        const STACK_BOTTOM: usize = vmem_layout::PROCESS_STACK_TOP - STACK_PAGES * PAGE_SIZE_4K;
        let offset = vaddr - STACK_BOTTOM;

        let obj = stack_win
            .borrow_mut()
            .bound_view
            .as_ref()
            .unwrap()
            .borrow_mut()
            .bound_object
            .as_ref()
            .unwrap()
            .clone();
        let frame_data = frame_table.frame_data(
            obj.borrow_mut()
                .lookup_frame(offset)
                .expect("Could not get frame")
                .frame_ref,
        );
        let offset_page = vaddr % PAGE_SIZE_4K;

        let string_len_with_null = string.as_bytes().len() + 1;
        copy_terminated_rust_string_to_buffer(
            &mut frame_data[offset_page..offset_page + string_len_with_null],
            string,
        )
        .expect("Failed to write string to stack");
    }

    fn write_words_to_stack(
        &self,
        frame_table: &FrameTable,
        stack_win: &Rc<RefCell<Window>>,
        vaddr: usize,
        words: &[u64],
    ) {
        const STACK_BOTTOM: usize = vmem_layout::PROCESS_STACK_TOP - STACK_PAGES * PAGE_SIZE_4K;
        let offset = vaddr - STACK_BOTTOM;

        let obj = stack_win
            .borrow_mut()
            .bound_view
            .as_ref()
            .unwrap()
            .borrow_mut()
            .bound_object
            .as_ref()
            .unwrap()
            .clone();
        let frame_data = frame_table.frame_data(
            obj.borrow_mut()
                .lookup_frame(offset)
                .expect("Could not get frame")
                .frame_ref,
        );
        let offset_page = vaddr % PAGE_SIZE_4K;

        let bytes_length = words.len() * 8;
        LittleEndian::write_u64_into(
            words,
            &mut frame_data[offset_page..offset_page + bytes_length],
        );
    }

    fn write_half_words_to_stack(
        &self,
        frame_table: &FrameTable,
        stack_win: &Rc<RefCell<Window>>,
        vaddr: usize,
        data: &[u32],
    ) {
        const STACK_BOTTOM: usize = vmem_layout::PROCESS_STACK_TOP - STACK_PAGES * PAGE_SIZE_4K;
        let offset = vaddr - STACK_BOTTOM;

        let obj = stack_win
            .borrow_mut()
            .bound_view
            .as_ref()
            .unwrap()
            .borrow_mut()
            .bound_object
            .as_ref()
            .unwrap()
            .clone();
        let frame_data = frame_table.frame_data(
            obj.borrow_mut()
                .lookup_frame(offset)
                .expect("Could not get frame")
                .frame_ref,
        );
        let offset_page = vaddr % PAGE_SIZE_4K;

        let bytes_length = data.len() * 4;
        LittleEndian::write_u32_into(
            data,
            &mut frame_data[offset_page..offset_page + bytes_length],
        );
    }
}

fn init_process_stack(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
    frame_table: &mut FrameTable,
) -> Result<(usize, Rc<RefCell<Window>>), sel4::Error> {
    let window = Rc::new(RefCell::new(Window {
        start: vmem_layout::PROCESS_STACK_TOP - vmem_layout::STACK_PAGES * PAGE_SIZE_4K,
        size: vmem_layout::STACK_PAGES * PAGE_SIZE_4K,
        bound_view: None,
    }));

    let object = Rc::new(RefCell::new(AnonymousMemoryObject::new(
        vmem_layout::STACK_PAGES * PAGE_SIZE_4K,
        sel4::CapRights::all(),
        ObjAttributes::DEFAULT,
    )));

    let view = Rc::new(RefCell::new(View::new(
        window.clone(),
        Some(object.clone()),
        None,
        sel4::CapRights::all(),
        0,
        0,
    )));

    window.borrow_mut().bound_view = Some(view.clone());
    object.borrow_mut().associated_views.push(view.clone());

    /* Preallocate the stack */
    // @alwin: This just makes my life a little bit easier, but isn't strictly necessary
    for i in 0..STACK_PAGES {
        let frame_ref = frame_table
            .alloc_frame(cspace, ut_table)
            .ok_or(sel4::Error::NotEnoughMemory)?;
        let orig_frame_cap = frame_table.frame_from_ref(frame_ref).get_cap();
        object
            .borrow_mut()
            .insert_frame_at(i * PAGE_SIZE_4K, (orig_frame_cap, frame_ref))
            .expect("Failed to insert frame into object");

        let loadee_frame = sel4::CPtr::from_bits(cspace.alloc_slot()?.try_into().unwrap())
            .cast::<sel4::cap_type::UnspecifiedPage>();

        cspace
            .root_cnode()
            .absolute_cptr(loadee_frame)
            .copy(
                &cspace
                    .root_cnode()
                    .absolute_cptr(frame_table.frame_from_ref(frame_ref).get_cap()),
                sel4::CapRightsBuilder::all().build(),
            )
            .expect("Failed to copy frame");
        view.borrow_mut()
            .insert_cap_at(i * PAGE_SIZE_4K, loadee_frame.cast())
            .expect("Failed to insert frame into view");
    }

    return Ok((vmem_layout::PROCESS_STACK_TOP, window));
}

// @alwin: this leaks cslots and caps!
pub fn start_process(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
    frame_table: &mut FrameTable,
    sched_control: sel4::cap::SchedControl,
    name: &str,
    ep: sel4::cap::Endpoint,
    elf_data: &[u8],
    loader_args: Option<Vec<&str>>,
    exec_args: Option<Vec<&str>>,
    prio: u8,
) -> Result<Rc<RefCell<ProcessType>>, sel4::Error> {
    /* We essentially use the position in the table as the pid. Don't think this is the right way to
    do it properly */
    let pos = find_free_proc().ok_or(sel4::Error::NotEnoughMemory)?;

    /* Create a VSpace */
    let vspace = alloc_retype::<sel4::cap_type::VSpace>(
        cspace,
        ut_table,
        sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SeL4Arch(
            sel4::ObjectBlueprintAArch64::VSpace,
        )),
    )?;

    /* assign the vspace to an asid pool */
    sel4::init_thread::slot::ASID_POOL
        .cap()
        .asid_pool_assign(vspace.0)
        .map_err(|e| {
            err_rs!("Failed to assign vspace to ASID pool");
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Create a simple 1 level CSpace */
    let mut proc_cspace = UserCSpace::new(cspace, ut_table, false)?;

    /* Allocate a frame for the IPC buffer */
    let ipc_buffer_ref = frame_table
        .alloc_frame(cspace, ut_table)
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
    let ipc_buffer_cap = sel4::CPtr::from_bits(ipc_buffer_slot.try_into().unwrap())
        .cast::<sel4::cap_type::SmallPage>();
    cspace
        .root_cnode()
        .absolute_cptr(ipc_buffer_cap)
        .copy(
            &cspace
                .root_cnode()
                .absolute_cptr(frame_table.frame_from_ref(ipc_buffer_ref).get_cap()),
            sel4::CapRightsBuilder::all().build(),
        )
        .map_err(|e| {
            err_rs!("Failed to copy frame cap for user ipc buffer mapping");
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;
    let ipc_buffer = (ipc_buffer_cap, ipc_buffer_ref);

    /* allocate a new slot in the target cspace which we will mint a badged endpoint cap into --
     * the badge is used to identify the process */
    let proc_ep = proc_cspace.alloc_slot().map_err(|e| {
        err_rs!("Failed to allocate slot for user endpoint cap");
        cspace.delete(ipc_buffer_slot).unwrap();
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;
    // Make sure the slot selected is what the runtime expects
    assert!(proc_ep == smos_common::init::InitCNodeSlots::SMOS_RootServerEP as usize);

    /* now mutate the cap, thereby setting the badge */
    proc_cspace
        .root_cnode()
        .absolute_cptr_from_bits_with_depth(proc_ep.try_into().unwrap(), sel4::WORD_SIZE)
        .mint(
            &cspace.root_cnode().absolute_cptr(ep),
            sel4::CapRightsBuilder::all().build(),
            (pos | INVOCATION_EP_BITS).try_into().unwrap(),
        )
        .map_err(|e| {
            err_rs!("Failed to mint user endpoint cap");
            proc_cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Allocate a slot for a self-referential cspace cap */
    let proc_self_cspace = proc_cspace.alloc_slot().map_err(|e| {
        err_rs!("Failed to allocate slot for self-referential cap");
        proc_cspace.delete(proc_ep).unwrap();
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot).unwrap();
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;
    // Make sure the slot selected is what the runtime expects
    assert!(proc_self_cspace == smos_common::init::InitCNodeSlots::SMOS_CNodeSelf as usize);

    /* Copy the CNode cap into the new process cspace*/
    proc_cspace
        .root_cnode()
        .absolute_cptr_from_bits_with_depth(proc_self_cspace.try_into().unwrap(), sel4::WORD_SIZE)
        .copy(
            &cspace.root_cnode().absolute_cptr(proc_cspace.root_cnode()),
            sel4::CapRightsBuilder::all().build(),
        )
        .map_err(|e| {
            err_rs!("Failed to copy self-refernetial cnode cap");
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Create a new TCB object */
    let tcb = alloc_retype::<sel4::cap_type::Tcb>(cspace, ut_table, sel4::ObjectBlueprint::Tcb)
        .map_err(|e| {
            err_rs!("Failed to allocate new TCB object");
            proc_cspace.delete(proc_self_cspace).unwrap();
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Configure the TCB */
    // @alwin: changing the WORD_SIZE to 64 causes a panic - not important but maybe understand why?
    tcb.0
        .tcb_configure(
            proc_cspace.root_cnode(),
            sel4::CNodeCapData::new(0, 0),
            vspace.0,
            vmem_layout::PROCESS_IPC_BUFFER.try_into().unwrap(),
            ipc_buffer.0,
        )
        .map_err(|e| {
            err_rs!("Failed to configure TCB");
            dealloc_retyped(cspace, ut_table, tcb);
            proc_cspace.delete(proc_self_cspace).unwrap();
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Create scheduling context */
    let sched_context = alloc_retype::<sel4::cap_type::SchedContext>(
        cspace,
        ut_table,
        sel4::ObjectBlueprint::SchedContext {
            size_bits: sel4_sys::seL4_MinSchedContextBits.try_into().unwrap(),
        },
    )
    .map_err(|e| {
        err_rs!("Failed to create scheduling context");
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace).unwrap();
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep).unwrap();
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot).unwrap();
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Configure the scheduling context to use the first core with budget equal to period */
    sched_control
        .sched_control_configure_flags(sched_context.0, 1000, 1000, 0, 0, 0)
        .map_err(|e| {
            err_rs!("Failed to configure scheduling context");
            dealloc_retyped(cspace, ut_table, tcb);
            proc_cspace.delete(proc_self_cspace).unwrap();
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Allocate a slot for a badged fault endpoint capability */
    let fault_ep = cspace
        .alloc_cap::<sel4::cap_type::Endpoint>()
        .map_err(|e| {
            err_rs!("Failed to allocate slot for fault endpoint");
            dealloc_retyped(cspace, ut_table, tcb);
            proc_cspace.delete(proc_self_cspace).unwrap();
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Mint the badged fault EP capability into the slot */
    cspace
        .root_cnode()
        .absolute_cptr(fault_ep)
        .mint(
            &cspace.root_cnode().absolute_cptr(ep),
            sel4::CapRightsBuilder::all().build(),
            (pos | FAULT_EP_BITS).try_into().unwrap(),
        )
        .map_err(|e| {
            err_rs!("Failed to mint badged fault endpoint");
            cspace.free_cap(fault_ep);
            dealloc_retyped(cspace, ut_table, tcb);
            proc_cspace.delete(proc_self_cspace).unwrap();
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Set up the TCB scheduling parameters */
    tcb.0
        .tcb_set_sched_params(
            sel4::init_thread::slot::TCB.cap(),
            0,
            prio.into(),
            sched_context.0,
            fault_ep,
        )
        .map_err(|e| {
            err_rs!("Failed to sceduling parameters");
            cspace.delete_cap(fault_ep).unwrap();
            cspace.free_cap(fault_ep);
            dealloc_retyped(cspace, ut_table, tcb);
            proc_cspace.delete(proc_self_cspace).unwrap();
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    tcb.0.debug_name(name.as_bytes());

    // @alwin: I've just included a bytearray containing the elf contents, which lets me avoid any
    // CPIO archive stuff, but is a bit more rigid.
    let elf = ElfBytes::<elf::endian::AnyEndian>::minimal_parse(elf_data)
        .or(Err(sel4::Error::InvalidArgument))
        .map_err(|e| {
            err_rs!("Failed to parse ELF file");
            cspace.delete_cap(fault_ep).unwrap();
            cspace.free_cap(fault_ep);
            dealloc_retyped(cspace, ut_table, tcb);
            proc_cspace.delete(proc_self_cspace).unwrap();
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Load the ELF file into the virtual address space */
    let mut initial_windows =
        load_elf(cspace, ut_table, frame_table, vspace.0, &elf).map_err(|e| {
            err_rs!("Failed to load ELF file");
            cspace.delete_cap(fault_ep).unwrap();
            cspace.free_cap(fault_ep);
            dealloc_retyped(cspace, ut_table, tcb);
            proc_cspace.delete(proc_self_cspace).unwrap();
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Map the IPC buffer into the virtual address space */
    map_frame(
        cspace,
        ut_table,
        ipc_buffer.0.cast(),
        vspace.0,
        vmem_layout::PROCESS_IPC_BUFFER,
        sel4::CapRightsBuilder::all().build(),
        sel4::VmAttributes::DEFAULT,
        None,
    )
    .map_err(|e| {
        err_rs!("Failed to set IPC buffer");
        cspace.delete_cap(fault_ep).unwrap();
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace).unwrap();
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep).unwrap();
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot).unwrap();
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    // @alwin: This should probably be done properly w.r.t. windows and stuff. Maybe we don't need
    // memory objects, but we should at least make a window so a client can't create a window
    // on top of a region that is predefined by the process initialization.

    /* Allocate a frame for the shared page used for communication between this process and the root server */
    let shared_buffer_ref = frame_table
        .alloc_frame(cspace, ut_table)
        .ok_or(sel4::Error::NotEnoughMemory)
        .map_err(|e| {
            err_rs!("Failed to allocate frame for shared buffer between RS and proc");
            cspace.delete_cap(fault_ep).unwrap();
            cspace.free_cap(fault_ep);
            dealloc_retyped(cspace, ut_table, tcb);
            proc_cspace.delete(proc_self_cspace).unwrap();
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;

    /* Allocate a slot to hold cap used for the user mapping*/
    let shared_buffer_slot = cspace.alloc_slot().map_err(|e| {
        err_rs!("Failed to allocate CNode slot for shared buffer");
        frame_table.free_frame(shared_buffer_ref);
        cspace.delete_cap(fault_ep).unwrap();
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace).unwrap();
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep).unwrap();
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot).unwrap();
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Copy the root server's copy of the frame cap into the user mapping slot */
    let shared_buffer_cap = sel4::CPtr::from_bits(shared_buffer_slot.try_into().unwrap())
        .cast::<sel4::cap_type::SmallPage>();
    cspace
        .root_cnode()
        .absolute_cptr(shared_buffer_cap)
        .copy(
            &cspace
                .root_cnode()
                .absolute_cptr(frame_table.frame_from_ref(shared_buffer_ref).get_cap()),
            sel4::CapRightsBuilder::all().build(),
        )
        .map_err(|e| {
            err_rs!("Failed to copy frame cap for user shared buffer mapping");
            cspace.free_slot(shared_buffer_slot);
            frame_table.free_frame(shared_buffer_ref);
            cspace.delete_cap(fault_ep).unwrap();
            cspace.free_cap(fault_ep);
            dealloc_retyped(cspace, ut_table, tcb);
            proc_cspace.delete(proc_self_cspace).unwrap();
            proc_cspace.free_slot(proc_self_cspace);
            proc_cspace.delete(proc_ep).unwrap();
            cspace.free_slot(proc_ep);
            cspace.delete(ipc_buffer_slot).unwrap();
            cspace.free_slot(ipc_buffer_slot);
            frame_table.free_frame(ipc_buffer_ref);
            dealloc_retyped(cspace, ut_table, vspace);
            e
        })?;
    let shared_buffer = (shared_buffer_cap, shared_buffer_ref);

    /* Map in the shared page used for communication between this process and the root server */
    map_frame(
        cspace,
        ut_table,
        shared_buffer.0.cast(),
        vspace.0,
        vmem_layout::PROCESS_RS_DATA_TRANSFER_PAGE,
        sel4::CapRightsBuilder::all().build(),
        sel4::VmAttributes::DEFAULT,
        None,
    )
    .map_err(|e| {
        err_rs!("Failed to map shared buffer");
        cspace.delete(shared_buffer_slot).unwrap();
        cspace.free_slot(shared_buffer_slot);
        frame_table.free_frame(shared_buffer_ref);
        cspace.delete_cap(fault_ep).unwrap();
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace).unwrap();
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep).unwrap();
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot).unwrap();
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    /* Set up the process stack */
    let (_, stack_window) = init_process_stack(cspace, ut_table, frame_table).map_err(|e| {
        err_rs!("Failed to initialize stack");
        cspace.delete_cap(fault_ep).unwrap();
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_self_cspace).unwrap();
        proc_cspace.free_slot(proc_self_cspace);
        proc_cspace.delete(proc_ep).unwrap();
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot).unwrap();
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    initial_windows.push(stack_window.clone());

    let proc = UserProcess::new(
        tcb,
        pos,
        vspace,
        ipc_buffer,
        sched_context,
        proc_cspace,
        fault_ep,
        shared_buffer,
        initial_windows,
    );

    let sp = proc.write_args_to_stack(frame_table, stack_window, loader_args, exec_args);

    let mut user_context = sel4::UserContext::default();
    *user_context.pc_mut() = elf.ehdr.e_entry;
    *user_context.sp_mut() = sp.try_into().unwrap();

    // @alwin: Clean up the stack if this fails
    tcb.0.tcb_write_registers(true, 2, &mut user_context)?;

    let proc_saved = Rc::new(RefCell::new(ProcessType::ActiveProcess(proc)));
    procs_set(pos, Some(proc_saved.clone()));

    return Ok(proc_saved);
}

pub fn handle_process_spawn(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
    frame_table: &mut FrameTable,
    sched_control: sel4::cap::SchedControl,
    ep: sel4::cap::Endpoint,
    p: &mut UserProcess,
    args: ProcessSpawn,
) -> Result<SMOSReply, InvocationError> {
    let (idx, handle_ref) = p.allocate_handle()?;

    let loader_args = Some(vec![args.exec_name, args.fs_name]);
    // @alwin: This is a lazy way of handling the error;
    let proc = start_process(
        cspace,
        ut_table,
        frame_table,
        sched_control,
        &args.exec_name,
        ep,
        LOADER_CONTENTS,
        loader_args,
        args.args,
        args.prio,
    )
    .map_err(|_| InvocationError::InsufficientResources)?;

    *handle_ref = Some(ServerHandle::new(RootServerResource::Process(proc)));

    return Ok(SMOSReply::ProcessSpawn {
        hndl: local_handle::LocalHandle::new(idx),
    });
}

pub fn handle_process_wait(
    p: &mut UserProcess,
    reply: RSReplyWrapper,
    args: &ProcessWait,
) -> Option<Result<SMOSReply, InvocationError>> {
    let wait_proc_ref = match p.get_handle_mut(args.hndl.idx) {
        Ok(x) => x,
        Err(_) => return Some(Err(InvocationError::InvalidHandle { which_arg: 0 })),
    };
    let wait_proc = match wait_proc_ref.as_ref().unwrap().inner() {
        RootServerResource::Process(proc) => proc.clone(),
        _ => return Some(Err(InvocationError::InvalidHandle { which_arg: 0 })),
    };

    let wait_proc_type: &mut ProcessType = &mut wait_proc.borrow_mut();
    match wait_proc_type {
        ProcessType::ActiveProcess(x) => {
            x.waiter = Some(reply);
            None
        }
        ProcessType::ZombieProcess(x) => {
            procs_set(*x, None);
            Some(Ok(SMOSReply::ProcessWait))
        }
    }
}

pub fn handle_process_exit(
    cspace: &mut CSpace,
    ut_table: &mut UTTable,
    frame_table: &mut FrameTable,
    handle_cap_table: &mut HandleCapabilityTable<RootServerResource>,
    p: &mut UserProcess,
) {
    // @alwin: Clean up the process resources
    p.destroy(cspace, ut_table, frame_table, handle_cap_table);

    match p.waiter {
        Some(x) => {
            /* There is a process waiting for this one to terminate */
            let msginfo =
                sel4::with_ipc_buffer_mut(|ipc_buf| handle_reply(ipc_buf, SMOSReply::ProcessWait));

            /* Send a message saying that this process terminated */
            x.0.send(msginfo);

            /* Destroy the reply object*/
            dealloc_retyped(cspace, ut_table, x);
            procs_set(p.pid, None);
            warn_rs!("Sending message to waiter");
        }
        None => {
            // Transition the process to a zombie
            procs_set(
                p.pid,
                Some(Rc::new(RefCell::new(ProcessType::ZombieProcess(p.pid)))),
            );
            warn_rs!("Setting the process to a zombie");
        }
    }
}

pub fn handle_load_complete(
    cspace: &mut CSpace,
    frame_table: &mut FrameTable,
    p: &mut UserProcess,
    args: LoadComplete,
) -> Result<SMOSReply, InvocationError> {
    for window in &p.initial_windows {
        /* Clean up the memory object by freeing the frames in it */
        window
            .borrow_mut()
            .bound_view
            .as_ref()
            .unwrap()
            .borrow_mut()
            .bound_object
            .as_ref()
            .unwrap()
            .borrow_mut()
            .cleanup_frame_table(cspace, frame_table);

        /* The above should delete these caps, just need to free the slots */
        window
            .borrow_mut()
            .bound_view
            .as_ref()
            .unwrap()
            .borrow_mut()
            .cleanup_cap_table(cspace, false);
    }

    // The objects and the views should be cleaned up by doing this since they are RCs that should
    // not be referenced anywhere else.
    p.initial_windows.clear();

    let mut user_context = sel4::UserContext::default();
    *user_context.pc_mut() = args.entry_point as u64;
    *user_context.sp_mut() = args.sp as u64;

    // @alwin: deal with error case properly here
    p.tcb
        .0
        .tcb_write_registers(true, 2, &mut user_context)
        .expect("@alwin: This shouldn't be an assert");

    return Ok(SMOSReply::LoadComplete);
}
