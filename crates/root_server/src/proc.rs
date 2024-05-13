use crate::ut::{UTTable, UTWrapper};
use crate::frame_table::{FrameTable, FrameRef};
use crate::cspace::{CSpace, UserCSpace, CSpaceTrait};
use crate::vmem_layout;
use crate::page::PAGE_SIZE_4K;
use crate::util::alloc_retype;
use crate::elf_load::load_elf;
use crate::mapping::map_frame;
use array_init::array_init;
use elf::ElfBytes;


const APP_EP_BADGE: usize = 101;

pub struct UserProcess {
    tcb: (sel4::cap::Tcb, UTWrapper),
    vspace: (sel4::cap::VSpace, UTWrapper),
    ipc_buffer: (sel4::cap::SmallPage, FrameRef),
    sched_context: (sel4::cap::SchedContext, UTWrapper),
    cspace: UserCSpace,
    stack: [(sel4::cap::SmallPage, FrameRef); vmem_layout::USER_DEFAULT_STACK_PAGES],
}

impl UserProcess {
    pub fn new(
        tcb: (sel4::cap::Tcb, UTWrapper),
        vspace: (sel4::cap::VSpace, UTWrapper),
        ipc_buffer: (sel4::cap::SmallPage, FrameRef),
        sched_context: (sel4::cap::SchedContext, UTWrapper),
        cspace: UserCSpace,
        stack: [(sel4::cap::SmallPage, FrameRef); vmem_layout::USER_DEFAULT_STACK_PAGES]
    )  -> UserProcess {

        return UserProcess {
            tcb: tcb, vspace: vspace, ipc_buffer: ipc_buffer,
            sched_context: sched_context, cspace: cspace,
            stack: stack
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
pub fn start_first_process(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable,
                       sched_control: sel4::cap::SchedControl, name: &str, ep: sel4::cap::Endpoint,
                       elf_data: &[u8]) -> Result<UserProcess, sel4::Error> {

    /* Create a VSpace */
    let mut vspace = alloc_retype::<sel4::cap_type::VSpace>(cspace, ut_table, sel4::ObjectBlueprint::Arch(
                                                        sel4::ObjectBlueprintArch::SeL4Arch(
                                                        sel4::ObjectBlueprintAArch64::VSpace)))?;

    /* assign the vspace to an asid pool */
    sel4::init_thread::slot::ASID_POOL.cap().asid_pool_assign(vspace.0)?;

    /* Create a simple 1 level CSpace */
    let mut proc_cspace = UserCSpace::new(cspace, ut_table, false)?;

    /* Create an IPC buffer */
    let ipc_buffer_ref = frame_table.alloc_frame(cspace, ut_table).ok_or(sel4::Error::NotEnoughMemory)?;
    let ipc_buffer_cap = sel4::CPtr::from_bits(cspace.alloc_slot()?.try_into().unwrap()).cast::<sel4::cap_type::SmallPage>();
    cspace.root_cnode().relative(ipc_buffer_cap).copy(&cspace.root_cnode().relative(frame_table.frame_from_ref(ipc_buffer_ref).get_cap()),
                                                      sel4::CapRightsBuilder::all().build())?;
    let ipc_buffer = (ipc_buffer_cap, ipc_buffer_ref);

    /* allocate a new slot in the target cspace which we will mint a badged endpoint cap into --
     * the badge is used to identify the process */
     let mut proc_ep = proc_cspace.alloc_slot()?;

     /* now mutate the cap, thereby setting the badge */
     proc_cspace.root_cnode().relative_bits_with_depth(proc_ep.try_into().unwrap(), sel4::WORD_SIZE)
                           .mint(&cspace.root_cnode().relative(ep),
                                 sel4::CapRightsBuilder::all().build(),
                                 APP_EP_BADGE.try_into().unwrap())?;


    /* Create a new TCB object */
    let mut tcb = alloc_retype::<sel4::cap_type::Tcb>(cspace, ut_table, sel4::ObjectBlueprint::Tcb)?;


    /* Configure the TCB */
    // @alwin: changing the WORD_SIZE to 64 causes a panic - not important but maybe understand why?
    tcb.0.tcb_configure(proc_cspace.root_cnode(), sel4::CNodeCapData::new(0, 0), vspace.0, vmem_layout::PROCESS_IPC_BUFFER.try_into().unwrap(), ipc_buffer.0)?;

    /* Create scheduling context */
    let mut sched_context = alloc_retype::<sel4::cap_type::SchedContext>(cspace, ut_table, sel4::ObjectBlueprint::SchedContext{ size_bits: sel4_sys::seL4_MinSchedContextBits.try_into().unwrap()})?;

    /* Configure the scheduling context to use the first core with budget equal to period */
    sched_control.sched_control_configure_flags(sched_context.0, 1000, 1000, 0, 0, 0);

    // @alwin: The endpoint passed in here should actually be badged like the other one
    tcb.0.tcb_set_sched_params(sel4::init_thread::slot::TCB.cap(), 0, 0, sched_context.0, ep)?;

    tcb.0.debug_name(name.as_bytes());

    // @alwin: I've just included a bytearray containing the elf contents, which lets me avoid any
    // CPIO archive stuff, but is a bit more rigid.

    let elf = ElfBytes::<elf::endian::AnyEndian>::minimal_parse(elf_data).or(Err(sel4::Error::InvalidArgument))?;

    let (sp, stack_pages) = init_process_stack(cspace, ut_table, frame_table, vspace.0)?;

    load_elf(cspace, ut_table, frame_table, vspace.0, &elf);

    map_frame(cspace, ut_table, ipc_buffer.0.cast(), vspace.0, vmem_layout::PROCESS_IPC_BUFFER,
          sel4::CapRightsBuilder::all().build(), sel4::VmAttributes::DEFAULT, None)?;

    let mut user_context = sel4::UserContext::default();
    *user_context.pc_mut() = elf.ehdr.e_entry;
    *user_context.sp_mut() = sp.try_into().unwrap();

    tcb.0.tcb_write_registers(true, 2, &mut user_context)?;

    return Ok(UserProcess::new(tcb, vspace, ipc_buffer, sched_context, proc_cspace, stack_pages));
}
