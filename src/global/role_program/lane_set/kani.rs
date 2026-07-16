use super::{
    LANE_DOMAIN_SIZE, LANE_SET_VIEW_WORDS, LaneSet, LaneSetView, logical_lane_count_for_role,
};

#[kani::proof]
fn logical_lane_capacity_is_the_exact_descriptor_lane_span() {
    let active_lane_count: u16 = kani::any();
    let endpoint_lane_slot_count: u16 = kani::any();
    kani::assume(endpoint_lane_slot_count != 0);
    kani::assume(endpoint_lane_slot_count as usize <= LANE_DOMAIN_SIZE);
    kani::assume(active_lane_count <= endpoint_lane_slot_count);

    assert_eq!(
        logical_lane_count_for_role(
            active_lane_count as usize,
            endpoint_lane_slot_count as usize,
        ),
        endpoint_lane_slot_count as usize
    );
}

#[kani::proof]
fn lane_set_mutation_is_exact_over_the_complete_lane_domain() {
    let lane: u8 = kani::any();
    let mut words = [0usize; LANE_SET_VIEW_WORDS];
    let mut set = core::mem::MaybeUninit::<LaneSet>::uninit();
    /* SAFETY: `set` is writable uninitialized storage and `words` is one live,
    exclusively borrowed full-domain lane-word allocation. */
    unsafe {
        LaneSet::init_from_parts(set.as_mut_ptr(), words.as_mut_ptr(), words.len());
    }
    /* SAFETY: `init_from_parts` initialized every `LaneSet` field above. */
    let mut set = unsafe { set.assume_init() };

    set.insert(lane as usize);
    assert!(set.view().contains(lane as usize));
    set.remove(lane as usize);
    assert!(!set.view().contains(lane as usize));
}

#[kani::proof]
#[kani::unwind(6)]
fn lane_set_iteration_returns_the_first_set_lane_in_the_exact_domain() {
    let words: [usize; LANE_SET_VIEW_WORDS] = kani::any();
    let start: u16 = kani::any();
    let lane_limit: u16 = kani::any();
    let probe: u16 = kani::any();
    kani::assume(start as usize <= LANE_DOMAIN_SIZE);
    kani::assume(lane_limit as usize <= LANE_DOMAIN_SIZE);

    /* SAFETY: the symbolic word array remains live and immutable for the
    complete proof. */
    let view = unsafe { LaneSetView::from_parts(words.as_ptr(), words.len()) };
    let actual = view.next_set_from(start as usize, lane_limit as usize);
    match actual {
        Some(found) => {
            assert!(found >= start as usize);
            assert!(found < lane_limit as usize);
            assert!(view.contains(found));
            if probe as usize >= start as usize && (probe as usize) < found {
                assert!(!view.contains(probe as usize));
            }
        }
        None => {
            if probe as usize >= start as usize && (probe as usize) < lane_limit as usize {
                assert!(!view.contains(probe as usize));
            }
        }
    }
}

#[kani::proof]
#[kani::should_panic]
fn nonempty_lane_set_view_rejects_null_storage() {
    /* SAFETY: this deliberately violates the view contract to prove null is
    rejected before any read. */
    let _ = unsafe { LaneSetView::from_parts(core::ptr::null(), 1) };
}

#[kani::proof]
#[kani::should_panic]
fn nonempty_lane_set_owner_rejects_null_storage() {
    let mut set = core::mem::MaybeUninit::<LaneSet>::uninit();
    /* SAFETY: this deliberately violates the owner contract to prove null is
    rejected before initialization writes. */
    unsafe { LaneSet::init_from_parts(set.as_mut_ptr(), core::ptr::null_mut(), 1) };
}
