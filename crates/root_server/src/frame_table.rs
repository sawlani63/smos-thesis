use smos_common::util::BIT;
use crate::vmem_layout;
use crate::cspace::{CSpace, CSpaceTrait};
use crate::ut::UTTable;
use sel4::CPtr;
use core::mem::size_of;
use crate::mapping::map_frame;

type FrameData = [u8; BIT(sel4_sys::seL4_PageBits as usize)];

pub type FrameRef = u32;

// @alwin: C uses a packed struct but we don't have those in Rust.
// #[packed]
pub struct Frame {
    cap: sel4::cap::SmallPage,
    prev: Option<FrameRef>,
    next: Option<FrameRef>,
    list_id: ListID,
}

#[derive(Copy,Clone,Debug)]
pub struct FrameWrapper {
    frame: *mut Frame
}

impl FrameWrapper {
    fn get_prev(self: &Self) -> Option<FrameRef> {
        return unsafe {(*self.frame).prev};
    }

    fn set_prev(self: &Self, prev: Option<FrameRef>) {
        unsafe {(*self.frame).prev = prev};
    }

    pub fn get_cap(self: &Self) -> sel4::cap::SmallPage {
        return unsafe {(*self.frame).cap};
    }

    fn set_cap(self: &Self, cap: sel4::cap::SmallPage) {
        unsafe {(*self.frame).cap = cap};
    }

    fn get_next(self: &Self) -> Option<FrameRef> {
        return unsafe {(*self.frame).next};
    }

    fn set_next(self: &Self, next: Option<FrameRef>) {
        unsafe {(*self.frame).next = next};
    }

    fn get_list_id(self: &Self) -> ListID  {
        return unsafe {(*self.frame).list_id};
    }

    fn set_list_id(self: &Self, list_id: ListID) {
        unsafe {(*self.frame).list_id = list_id};
    }

    fn inner(self: &Self) -> *const Frame {
        return self.frame;
    }
}

#[derive(PartialEq, Copy, Clone)]
enum ListID {
    NoList,
    FreeList,
    AllocatedList
}

#[derive(Copy,Clone)]
struct FrameList {
    list_id: ListID,
    first: Option<FrameRef>,
    last: Option<FrameRef>,
    length: usize,
}

pub struct FrameTable {
    frames: *mut Frame,
    frame_data: *mut FrameData,
    capacity: usize,
    used: usize,
    byte_length: usize,
    free: FrameList,
    allocated: FrameList,
    vspace: sel4::cap::VSpace
}

impl FrameTable {
    pub fn frame_from_ref(self: &Self, frame_ref: FrameRef) -> FrameWrapper {
        assert!(frame_ref < self.capacity.try_into().unwrap());
        return FrameWrapper {frame: self.frames.wrapping_add(frame_ref.try_into().unwrap()) };
    }

    fn ref_from_frame(self: &Self, frame: &FrameWrapper) -> FrameRef {
        assert!(frame.inner() >= self.frames);
        assert!(frame.inner() < self.frames.wrapping_add(self.used));
        return unsafe { frame.inner().offset_from(self.frames).try_into().unwrap() };
    }

    pub fn frame_data(self: &Self, frame_ref: FrameRef) -> &mut FrameData {
        assert!(frame_ref < self.capacity.try_into().unwrap());
        return unsafe { &mut (*self.frame_data.wrapping_add(frame_ref.try_into().unwrap())) };
    }

    fn pop_front(self: &mut Self, list_id: ListID) -> Option<FrameWrapper> {
        let mut list = match list_id {
            ListID::FreeList => self.free,
            ListID::AllocatedList => self.allocated,
            _ => panic!("Invalid list type")
        };

        if list.first.is_none() {
            return None
        }

        let head = self.frame_from_ref(list.first.unwrap());
        if list.last == list.first {
            list.last = None;
            assert!(head.get_next() == None);
        } else {
            let next = self.frame_from_ref(head.get_next().unwrap());
            next.set_prev(None);
        }

        list.first = head.get_next();
        assert!(head.get_prev() == None);
        head.set_next(None);
        head.set_list_id(ListID::NoList);
        head.set_prev(None);
        list.length -= 1;

        match list_id {
            ListID::FreeList => self.free = list,
            ListID::AllocatedList => self.allocated = list,
            _ => panic!("Invalid list type")
        };

        return Some(head);
    }

    fn push_front(self: &mut Self, list_id: ListID, frame: FrameWrapper) {
        assert!(frame.get_list_id() == ListID::NoList);
        assert!(frame.get_next() == None);
        assert!(frame.get_prev() == None);

        let mut list = match list_id {
            ListID::FreeList => self.free,
            ListID::AllocatedList => self.allocated,
            _ => panic!("Invalid list type")
        };

        let frame_ref = self.ref_from_frame(&frame);

        if list.last == None {
            list.last = Some(frame_ref);
        }

        frame.set_next(list.first);
        if let Some(next_ref) = frame.get_next() {
            let next_frame = self.frame_from_ref(next_ref);
            next_frame.set_prev(Some(frame_ref));
        }

        list.first = Some(frame_ref);
        list.length += 1;
        frame.set_list_id(list.list_id);

        match list_id {
            ListID::FreeList => self.free = list,
            ListID::AllocatedList => self.allocated = list,
            _ => panic!("Invalid list type")
        };
    }

    fn push_back(self: &mut Self, list_id: ListID, frame: FrameWrapper) {
        assert!(frame.get_list_id() == ListID::NoList);
        assert!(frame.get_next() == None);
        assert!(frame.get_prev() == None);

        let mut list = match list_id {
            ListID::FreeList => self.free,
            ListID::AllocatedList => self.allocated,
            _ => panic!("Invalid list type")
        };

        let frame_ref = self.ref_from_frame(&frame);
        match list.last {
            Some(last_ref) => {
                let last_frame = self.frame_from_ref(last_ref);
                last_frame.set_next(Some(frame_ref));

                frame.set_prev(list.last);
                list.last = Some(frame_ref);

                frame.set_list_id(list.list_id);
                list.length += 1;

                match list_id {
                    ListID::FreeList => self.free = list,
                    ListID::AllocatedList => self.allocated = list,
                    _ => panic!("Invalid list type")
                };
            },
            None => self.push_front(list_id, frame) // This one pushes the list itself
        }
    }

    pub fn init(vspace: sel4::cap::VSpace) -> Self {
        return FrameTable {
            frames: vmem_layout::FRAME_TABLE as *mut Frame,
            frame_data: vmem_layout::FRAME_DATA as *mut FrameData,
            free: FrameList { list_id: ListID::FreeList, first: None, last: None, length: 0 },
            allocated: FrameList { list_id: ListID::AllocatedList, first: None, last: None, length: 0},
            capacity: 0,
            used: 0,
            byte_length: 0,
            vspace: vspace,
        }
    }

    pub fn alloc_frame(self: &mut Self, cspace: &mut CSpace, ut_table: &mut UTTable) -> Option<FrameRef> {
        let mut frame = self.pop_front(ListID::FreeList);

        if frame.is_none() {
            // @alwin: I really don't like this
            frame = Some(self.alloc_fresh_frame(cspace, ut_table).ok()?);
        }

        self.push_back(ListID::AllocatedList, frame.unwrap());
        return Some(self.ref_from_frame(&frame.unwrap()));

    }

    fn remove_frame(self: &mut Self, list_id: ListID, frame: &FrameWrapper) {
        let mut list = match list_id {
            ListID::FreeList => self.free,
            ListID::AllocatedList => self.allocated,
            _ => panic!("Invalid list type")
        };

        assert!(frame.get_list_id() == list.list_id);

        match frame.get_prev() {
            None => list.first = frame.get_next(),
            Some(prev_ref) => {
                let prev_frame = self.frame_from_ref(prev_ref);
                prev_frame.set_next(frame.get_next());
            }
        }

        match frame.get_next() {
            None => list.last = frame.get_prev(),
            Some(next_ref) => {
                let next_frame = self.frame_from_ref(next_ref);
                next_frame.set_prev(frame.get_prev());
            }
        }

        list.length -= 1;
        frame.set_list_id(ListID::NoList);
        frame.set_prev(None);
        frame.set_next(None);

        // @alwin: This is not nice but I had to do it because of multiple reference problems
        // Maybe use a refcell?
        match list_id {
            ListID::FreeList => self.free = list,
            ListID::AllocatedList => self.allocated = list,
            _ => panic!("Invalid list type")
        };
    }


    pub fn free_frame(self: &mut Self, frame_ref: FrameRef) {
        let frame = self.frame_from_ref(frame_ref);

        self.remove_frame(ListID::AllocatedList, &frame);
        self.push_front(ListID::FreeList, frame);
    }

    fn alloc_frame_at(self: &mut Self, cspace: &mut CSpace, ut_table: &mut UTTable, vaddr: usize)
                      -> Result<sel4::cap::SmallPage, sel4::Error> {

        /* Allocate an untyped for the frame. */
        let (_, ut) = ut_table.alloc_4k_untyped()?;

        /* Allocate a slot for the page capability. */
        let cptr = cspace.alloc_slot().map_err(|e| {
            ut_table.free(ut);
            e
        })?;

        /* Retype the untyped into a page. */
        cspace.untyped_retype(&ut.get_cap(), sel4::ObjectBlueprint::Arch(sel4::ObjectBlueprintArch::SmallPage),
                              cptr).map_err(|e| {

            cspace.free_slot(cptr);
            ut_table.free(ut);
            e
        })?;

        /* Map the frame in */
        let frame = CPtr::from_bits(cptr.try_into().unwrap()).cast::<sel4::cap_type::SmallPage>();
        map_frame(cspace, ut_table, frame.cast(), self.vspace, vaddr, sel4::CapRightsBuilder::all().build(),
                  sel4::VmAttributes::DEFAULT | sel4::VmAttributes::EXECUTE_NEVER, None).map_err(|e| {

            // @alwin: I don't like this error handling;
            let _ = cspace.delete(cptr);
            cspace.free_slot(cptr);
            ut_table.free(ut);
            e
        })?;

        return Ok(frame);
    }

    fn bump_capacity(self: &mut Self, cspace: &mut CSpace, ut_table: &mut UTTable) -> Result<(), sel4::Error> {
        // @alwin: There is some config frame limit thing here, probs unnecessary

        let vaddr = self.frames.wrapping_byte_add(self.byte_length);
        self.alloc_frame_at(cspace, ut_table, vaddr as usize)?;


        self.byte_length += BIT(sel4_sys::seL4_PageBits.try_into().unwrap());
        self.capacity = self.byte_length / size_of::<Frame>();

        return Ok(())
    }

    fn alloc_fresh_frame(self: &mut Self, cspace: &mut CSpace, ut_table: &mut UTTable)
                         -> Result<FrameWrapper, sel4::Error> {

        assert!(self.used <= self.capacity);
        // @alwin: There is a config frame limit thing here, probs unnecessary


        if self.used == self.capacity {
            self.bump_capacity(cspace, ut_table)?
        }

        assert!(self.used < self.capacity);

        let frame = self.frame_from_ref(self.used.try_into().unwrap());
        self.used += 1;

        // @alwin: Should we actually map everything into the root server?
        let vaddr = self.frame_data(self.ref_from_frame(&frame));
        return match self.alloc_frame_at(cspace, ut_table, vaddr as *const FrameData as usize) {
            Ok(frame_cap) => {
                frame.set_cap(frame_cap);
                frame.set_list_id(ListID::NoList);
                frame.set_prev(None);
                frame.set_next(None);
                Ok(frame)
            } Err(e) => {
                self.used -= 1;
                Err(e)
            }
        }
    }
}