#![allow(non_snake_case)]

use crate::bootstrap::INITIAL_TASK_CNODE_SIZE_BITS;
use crate::cspace::{CSpace, CSpaceTrait, BOT_LVL_PER_NODE, CNODE_SIZE_BITS, CNODE_SLOTS};
use crate::frame_table::{FrameRef, FrameTable};
use crate::ut::UTTable;
use alloc::boxed::Box;
use bitfield::{bf_clr_bit, bf_first_free, bf_get_bit, bf_set_bit, bitfield_init, bitfield_type};
use smos_common::util::BIT;

fn test_bf_bit(bit: usize) {
    let mut bf: bitfield_type!(128) = bitfield_init!(128);
    assert!(bf_first_free(&mut bf).unwrap() == 0);

    bf_set_bit(&mut bf, bit);
    assert!(bf_get_bit(&mut bf, bit));
    let ff = if bit == 0 { 1 } else { 0 };
    assert!(bf_first_free(&mut bf).unwrap() == ff);

    bf_clr_bit(&mut bf, bit);
    assert!(!bf_get_bit(&mut bf, bit));
}

const fn NSLOTS_MIN() -> usize {
    if CNODE_SLOTS(INITIAL_TASK_CNODE_SIZE_BITS) * CNODE_SLOTS(CNODE_SIZE_BITS) - 4
        < CNODE_SLOTS(CNODE_SIZE_BITS) * BOT_LVL_PER_NODE + 1
    {
        CNODE_SLOTS(INITIAL_TASK_CNODE_SIZE_BITS) * CNODE_SLOTS(CNODE_SIZE_BITS) - 4
    } else {
        CNODE_SLOTS(CNODE_SIZE_BITS) * BOT_LVL_PER_NODE + 1
    }
}

// @alwin: this is a hack!
const NSLOTS: usize = NSLOTS_MIN();

fn test_cspace(cspace: &mut CSpace) {
    let cptr = cspace.alloc_slot().unwrap();
    assert!(cptr != 0);

    cspace.free_slot(cptr);

    let cptr_new = cspace.alloc_slot().unwrap();
    assert!(cptr == cptr_new);

    cspace.free_slot(cptr_new);

    // @alwin: figure out NSLOTS for one-lvl cspace

    // @alwin: this test in C has dynamic allocation for some reason?

    let mut slots: [usize; NSLOTS] = [0; NSLOTS];
    let mut real_nslots = NSLOTS;

    for i in 0..NSLOTS {
        slots[i] = cspace.alloc_slot().unwrap();
        if slots[i] == 0 {
            real_nslots = i;
            break;
        }
    }

    log_rs!(
        "Allocated {} <-> {} slots",
        slots[0],
        slots[real_nslots - 1]
    );

    for i in 0..real_nslots {
        cspace.free_slot(slots[i]);
    }
}

fn test_bf() {
    test_bf_bit(0);
    test_bf_bit(1);
    test_bf_bit(63);
    test_bf_bit(64);
    test_bf_bit(65);
    test_bf_bit(127);

    let mut bf: bitfield_type!(128) = bitfield_init!(128);
    for i in 0..128 {
        assert!(!bf_get_bit(&mut bf, i));
        bf_set_bit(&mut bf, i);
        assert!(bf_get_bit(&mut bf, i));
        if i < 127 {
            assert!(bf_first_free(&mut bf).unwrap() == i + 1);
        } else {
            assert!(bf_first_free(&mut bf).is_err());
        }
    }
}

const TEST_FRAMES: usize = 10;

fn test_frame_table(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable) {
    let mut frames: [Option<FrameRef>; TEST_FRAMES] = [None; TEST_FRAMES];

    for f in 0..TEST_FRAMES {
        frames[f] = frame_table.alloc_frame(cspace, ut_table);
        assert!(frames[f].is_some());

        let data = frame_table.frame_data(frames[f].unwrap());
        data[0] = f.try_into().unwrap();
        data[BIT(sel4_sys::seL4_PageBits.try_into().unwrap()) - 1] = f.try_into().unwrap();
    }

    for f in 0..TEST_FRAMES {
        let data = frame_table.frame_data(frames[f].unwrap());
        assert!(usize::from(data[0]) == f);
        assert!(usize::from(data[BIT(sel4_sys::seL4_PageBits.try_into().unwrap()) - 1]) == f);
    }

    for f in 0..TEST_FRAMES {
        frame_table.free_frame(frames[f].unwrap());
    }

    let mut new_frames: [Option<FrameRef>; TEST_FRAMES] = [None; TEST_FRAMES];
    for f in 0..TEST_FRAMES {
        new_frames[f] = frame_table.alloc_frame(cspace, ut_table);
        assert!(new_frames[f].is_some());

        let mut o = 0;
        while o < TEST_FRAMES {
            if new_frames[f] == frames[o] {
                frames[o] = None;
                break;
            }
            o += 1;
        }

        assert!(o != TEST_FRAMES);
    }

    for f in 0..TEST_FRAMES {
        frame_table.free_frame(new_frames[f].unwrap());
    }
}

fn test_heap() {
    // Test simple heap allocation and free
    let t = Box::new(5);
    drop(t);
}

pub fn run_tests(cspace: &mut CSpace, ut_table: &mut UTTable, frame_table: &mut FrameTable) {
    test_bf();
    test_cspace(cspace);

    // @alwin: C also has some tests for children cspaces
    test_frame_table(cspace, ut_table, frame_table);
    test_heap();
}
