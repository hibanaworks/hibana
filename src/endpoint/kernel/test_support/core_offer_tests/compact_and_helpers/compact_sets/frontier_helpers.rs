use super::*;

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const fn frontier_max_usize(
    values: &[usize],
) -> usize {
    let mut idx = 0usize;
    let mut max = 0usize;
    while idx < values.len() {
        let value = values[idx];
        if value > max {
            max = value;
        }
        idx += 1;
    }
    max
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_lane_set_view<R>(
    lanes: &[usize],
    f: impl FnOnce(LaneSetView) -> R,
) -> R {
    let lane_limit = frontier_max_usize(lanes).saturating_add(1).max(1);
    let mut lane_words = std::vec![0 as LaneWord; lane_word_count(lane_limit)];
    let mut lane_set = LaneSet::from_parts(lane_words.as_mut_ptr(), lane_words.len());
    let mut idx = 0usize;
    while idx < lanes.len() {
        lane_set.insert(lanes[idx]);
        idx += 1;
    }
    f(lane_set.view())
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn assert_lane_set_eq(
    set: LaneSetView,
    lane_limit: usize,
    expected: &[u8],
) {
    let mut lanes = std::vec![u8::MAX; lane_limit.max(expected.len()).max(1)];
    let len = set.write_lane_indices(lane_limit, &mut lanes);
    assert_eq!(len, expected.len(), "lane-set length mismatch");
    assert_eq!(&lanes[..len], expected, "lane-set contents mismatch");
}

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_lane_set_view_keeps_sparse_high_lane_indices_exact()
 {
    with_lane_set_view(&[33], |set| {
        assert_lane_set_eq(set, 34, &[33]);
    });
    with_lane_set_view(&[0, 65], |set| {
        assert_lane_set_eq(set, 66, &[0, 65]);
    });
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn assert_buffered_lanes_eq(
    inbox: &BindingInbox,
    frame_label_mask: FrameLabelMask,
    expected: &[u8],
) {
    let mut lanes = std::vec![u8::MAX; expected.len().max(1)];
    let len = inbox.buffered_lanes_for_frame_labels(frame_label_mask, &mut lanes);
    assert_eq!(len, expected.len(), "buffered lane count mismatch");
    assert_eq!(&lanes[..len], expected, "buffered lanes mismatch");
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_test_binding_inbox<
    const ACTIVE_LANES: usize,
    R,
>(
    f: impl FnOnce(&mut BindingInbox) -> R,
) -> R {
    let mut lane_dense_by_lane: [DenseLaneOrdinal; ACTIVE_LANES] =
        core::array::from_fn(|lane_idx| {
            DenseLaneOrdinal::new(lane_idx).expect("test lane dense ordinal")
        });
    let mut slots = [[[0u32; 3]; BindingInbox::PER_LANE_CAPACITY]; ACTIVE_LANES];
    let mut len = [0u8; ACTIVE_LANES];
    let mut frame_label_masks = [FrameLabelMask::EMPTY; ACTIVE_LANES];
    let mut nonempty_lane_words = std::vec![0 as LaneWord; lane_word_count(ACTIVE_LANES)];
    let mut inbox = MaybeUninit::<BindingInbox>::uninit();
    unsafe {
        BindingInbox::init_empty(
            inbox.as_mut_ptr(),
            slots.as_mut_ptr().cast(),
            len.as_mut_ptr(),
            frame_label_masks.as_mut_ptr(),
            nonempty_lane_words.as_mut_ptr(),
            lane_dense_by_lane.as_mut_ptr(),
            ACTIVE_LANES,
            nonempty_lane_words.len(),
        );
        let mut inbox = inbox.assume_init();
        f(&mut inbox)
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn assert_nonempty_lanes_eq(
    inbox: &BindingInbox,
    lane_limit: usize,
    expected: &[u8],
) {
    let mut lanes = std::vec![u8::MAX; lane_limit.max(expected.len()).max(1)];
    let len = inbox
        .nonempty_lanes()
        .write_lane_indices(lane_limit, &mut lanes);
    assert_eq!(len, expected.len(), "nonempty lane count mismatch");
    assert_eq!(&lanes[..len], expected, "nonempty lane contents mismatch");
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_active_entry_set_storage<
    R,
>(
    capacity: usize,
    f: impl FnOnce(&mut ActiveEntrySet) -> R,
) -> R {
    let mut slots = std::vec![ActiveEntrySlot::EMPTY; capacity.max(1)];
    let mut entries = ActiveEntrySet::from_parts(slots.as_mut_ptr(), slots.len());
    entries.clear();
    f(&mut entries)
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn active_entry_set_storage(
    capacity: usize,
) -> (std::vec::Vec<ActiveEntrySlot>, ActiveEntrySet) {
    let mut slots = std::vec![ActiveEntrySlot::EMPTY; capacity.max(1)];
    let mut entries = ActiveEntrySet::from_parts(slots.as_mut_ptr(), slots.len());
    entries.clear();
    (slots, entries)
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn active_entry_set_from_pairs(
    entries: &[(usize, u8)],
) -> (std::vec::Vec<ActiveEntrySlot>, ActiveEntrySet) {
    let (slots, mut active_entries) = active_entry_set_storage(entries.len());
    for &(entry_idx, lane_idx) in entries {
        assert!(active_entries.insert_entry(entry_idx, lane_idx));
    }
    (slots, active_entries)
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn observed_entry_set_storage(
    capacity: usize,
) -> (std::vec::Vec<FrontierObservationSlot>, ObservedEntrySet) {
    let mut slots = std::vec![FrontierObservationSlot::EMPTY; capacity.max(1)];
    let mut entries = ObservedEntrySet::from_parts(slots.as_mut_ptr(), slots.len());
    entries.clear();
    (slots, entries)
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn observed_entry_set_from_states(
    entries: &[(usize, OfferEntryObservedState)],
) -> (std::vec::Vec<FrontierObservationSlot>, ObservedEntrySet) {
    let (slots, mut observed_entries) = observed_entry_set_storage(entries.len());
    for &(entry_idx, observed_state) in entries {
        let (entry_bit, inserted) = observed_entries
            .insert_entry(entry_idx)
            .expect("insert entry");
        assert!(inserted);
        observed_entries.observe(entry_bit, observed_state);
    }
    (slots, observed_entries)
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn frontier_observation_key_storage(
    slot_capacity: usize,
    lane_limit: usize,
) -> (
    std::vec::Vec<FrontierObservationSlot>,
    std::vec::Vec<LaneWord>,
    std::vec::Vec<LaneWord>,
    FrontierObservationKey,
) {
    let mut slots = std::vec![FrontierObservationSlot::EMPTY; slot_capacity.max(1)];
    let mut offer_lane_words = std::vec![0 as LaneWord; lane_word_count(lane_limit.max(1))];
    let mut binding_nonempty_lane_words =
        std::vec![0 as LaneWord; lane_word_count(lane_limit.max(1))];
    let mut key = FrontierObservationKey::from_parts(
        slots.as_mut_ptr(),
        slots.len(),
        offer_lane_words.as_mut_ptr(),
        binding_nonempty_lane_words.as_mut_ptr(),
        offer_lane_words.len(),
    );
    key.clear();
    (slots, offer_lane_words, binding_nonempty_lane_words, key)
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn copied_frontier_observation_key_storage(
    src: FrontierObservationKey,
    slot_capacity: usize,
    lane_limit: usize,
) -> (
    std::vec::Vec<FrontierObservationSlot>,
    std::vec::Vec<LaneWord>,
    std::vec::Vec<LaneWord>,
    FrontierObservationKey,
) {
    let (slots, offer_lane_words, binding_nonempty_lane_words, mut key) =
        frontier_observation_key_storage(slot_capacity, lane_limit);
    key.copy_from(src);
    (slots, offer_lane_words, binding_nonempty_lane_words, key)
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn with_frontier_observation_key_storage<
    R,
>(
    slot_capacity: usize,
    lane_limit: usize,
    f: impl FnOnce(&mut FrontierObservationKey) -> R,
) -> R {
    let mut slots = std::vec![FrontierObservationSlot::EMPTY; slot_capacity.max(1)];
    let mut offer_lane_words = std::vec![0 as LaneWord; lane_word_count(lane_limit.max(1))];
    let mut binding_nonempty_lane_words =
        std::vec![0 as LaneWord; lane_word_count(lane_limit.max(1))];
    let mut key = FrontierObservationKey::from_parts(
        slots.as_mut_ptr(),
        slots.len(),
        offer_lane_words.as_mut_ptr(),
        binding_nonempty_lane_words.as_mut_ptr(),
        offer_lane_words.len(),
    );
    key.clear();
    f(&mut key)
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn frontier_candidates<
    const N: usize,
>() -> [FrontierCandidate; N] {
    [FrontierCandidate::EMPTY; N]
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn frontier_visit_slots<
    const N: usize,
>() -> [ScopeId; N] {
    [ScopeId::none(); N]
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn frontier_snapshot_fixture<
    const N: usize,
>(
    current_scope: ScopeId,
    current_entry_idx: usize,
    current_parallel_root: ScopeId,
    current_frontier: FrontierKind,
    candidates: &mut [FrontierCandidate; N],
    candidate_len: usize,
) -> FrontierSnapshot {
    let source = *candidates;
    let len = core::cmp::min(candidate_len, N);
    let mut snapshot = unsafe {
        FrontierSnapshot::from_parts(
            current_scope,
            current_entry_idx,
            current_parallel_root,
            current_frontier,
            candidates.as_mut_ptr(),
            N,
        )
    };
    let mut idx = 0usize;
    while idx < len {
        assert!(snapshot.push_candidate(source[idx]));
        idx += 1;
    }
    snapshot
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn frontier_visit_set_fixture(
    slots: &mut [ScopeId],
) -> FrontierVisitSet {
    unsafe { FrontierVisitSet::from_parts(slots.as_mut_ptr(), slots.len()) }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn empty_frontier_visit_set()
-> FrontierVisitSet {
    unsafe { FrontierVisitSet::from_parts(core::ptr::null_mut(), 0) }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const fn offer_endpoint_slot_bytes<
    const ROLE: u8,
    T,
    B,
>(
    lane_capacity: usize,
) -> usize
where
    T: Transport + 'static,
    B: crate::binding::EndpointSlot + 'static,
{
    let header_bytes = size_of::<
        CursorEndpoint<
            'static,
            ROLE,
            T,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
            4,
            crate::control::cap::mint::MintConfig,
            B,
        >,
    >();
    let port_align = align_of::<
        Option<crate::rendezvous::port::Port<'static, T, crate::control::cap::mint::EpochTbl>>,
    >();
    let port_offset =
        (header_bytes + (port_align.saturating_sub(1))) & !(port_align.saturating_sub(1));
    let port_bytes = size_of::<
        Option<crate::rendezvous::port::Port<'static, T, crate::control::cap::mint::EpochTbl>>,
    >() * lane_capacity;
    let guard_align = align_of::<
        Option<crate::endpoint::affine::LaneGuard<'static, T, DefaultLabelUniverse, CounterClock>>,
    >();
    let guard_offset = (port_offset + port_bytes + (guard_align.saturating_sub(1)))
        & !(guard_align.saturating_sub(1));
    guard_offset
        + size_of::<
            Option<
                crate::endpoint::affine::LaneGuard<'static, T, DefaultLabelUniverse, CounterClock>,
            >,
        >() * lane_capacity
}
