use crate::ut::{UTTable, UTWrapper};
use crate::frame_table::{FrameTable, FrameRef};
use crate::cspace::{CSpace, UserCSpace, CSpaceTrait};
use crate::vmem_layout;
use crate::page::{PAGE_SIZE_4K, BIT};
use crate::util::{alloc_retype,  FAULT_EP_BIT, dealloc_retyped};
use crate::elf_load::load_elf;
use crate::mapping::map_frame;
use elf::ElfBytes;

// @alwin: This should probably be unbounded
const MAX_PROCS: usize = 64;
pub const MAX_PID: usize = 1024;

const ARRAY_REPEAT_VALUE: Option<UserProcess> = None;
static mut procs : [Option<UserProcess>; MAX_PROCS] = [ARRAY_REPEAT_VALUE; MAX_PROCS];

pub fn procs_get(i: usize) -> &'static Option<UserProcess> {
    unsafe {
        assert!(i < procs.len());
        return &procs[i];
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
    vspace: (sel4::cap::VSpace, UTWrapper),
    ipc_buffer: (sel4::cap::SmallPage, FrameRef),
    sched_context: (sel4::cap::SchedContext, UTWrapper),
    cspace: UserCSpace,
    stack: [(sel4::cap::SmallPage, FrameRef); vmem_layout::USER_DEFAULT_STACK_PAGES],
    fault_ep: sel4::cap::Endpoint
}

impl UserProcess {
    pub fn new(
        tcb: (sel4::cap::Tcb, UTWrapper),
        vspace: (sel4::cap::VSpace, UTWrapper),
        ipc_buffer: (sel4::cap::SmallPage, FrameRef),
        sched_context: (sel4::cap::SchedContext, UTWrapper),
        cspace: UserCSpace,
        stack: [(sel4::cap::SmallPage, FrameRef); vmem_layout::USER_DEFAULT_STACK_PAGES],
        fault_ep: sel4::cap::Endpoint
    )  -> UserProcess {

        return UserProcess {
            tcb: tcb, vspace: vspace, ipc_buffer: ipc_buffer,
            sched_context: sched_context, cspace: cspace,
            stack: stack, fault_ep: fault_ep
        };
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
     let mut proc_ep = proc_cspace.alloc_slot().map_err(|e| {
        err_rs!("Failed to allocate slot for user endpoint cap");
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
     })?;

     /* now mutate the cap, thereby setting the badge */
     proc_cspace.root_cnode().relative_bits_with_depth(proc_ep.try_into().unwrap(), sel4::WORD_SIZE)
                             .mint(&cspace.root_cnode().relative(ep),
                                   sel4::CapRightsBuilder::all().build(),
                                   pos.try_into().unwrap()).map_err(|e| {

        err_rs!("Failed to mint user endpoint cap");
        proc_cspace.free_slot(proc_ep);
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
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;


    cspace.root_cnode().relative(fault_ep).mint(&cspace.root_cnode().relative(ep),
                                                sel4::CapRightsBuilder::all().build(),
                                                (pos | FAULT_EP_BIT).try_into().unwrap()).map_err(|e| {

        err_rs!("Failed to mint badged fault endpoint");
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    tcb.0.tcb_set_sched_params(sel4::init_thread::slot::TCB.cap(), 0, 0, sched_context.0, fault_ep)
         .map_err(|e| {

        err_rs!("Failed to sceduling parameters");
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
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
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
   })?;

    load_elf(cspace, ut_table, frame_table, vspace.0, &elf).map_err(|e| {
        err_rs!("Failed to load ELF file");
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    map_frame(cspace, ut_table, ipc_buffer.0.cast(), vspace.0, vmem_layout::PROCESS_IPC_BUFFER,
              sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None).map_err(|e| {

        err_rs!("Failed to initialize stack");
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
        proc_cspace.delete(proc_ep);
        cspace.free_slot(proc_ep);
        cspace.delete(ipc_buffer_slot);
        cspace.free_slot(ipc_buffer_slot);
        frame_table.free_frame(ipc_buffer_ref);
        dealloc_retyped(cspace, ut_table, vspace);
        e
    })?;

    let (sp, stack_pages) = init_process_stack(cspace, ut_table, frame_table, vspace.0).map_err(|e| {
        err_rs!("Failed to initialize stack");
        cspace.delete_cap(fault_ep);
        cspace.free_cap(fault_ep);
        dealloc_retyped(cspace, ut_table, tcb);
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

    procs_set(pos, Some(UserProcess::new(tcb, vspace, ipc_buffer, sched_context,
                                         proc_cspace, stack_pages, fault_ep)));

    return Ok(pos);
}
