use super::*;
use crate::endpoint::kernel::core::{SelectedRouteCommitRow, test_commit_delta_apply_permit};
use crate::global::role_program::{DenseLaneOrdinal, LaneWord, lane_word_count};
use core::mem::MaybeUninit;

struct RouteStateFixture {
    state: RouteState,
    storage: RouteStateFixtureStorage,
}

struct RouteStateFixtureStorage {
    route_arm: std::vec::Vec<RouteArmState>,
    lane_offer_state: std::vec::Vec<LaneOfferState>,
    scope_evidence_slots: std::vec::Vec<MaybeUninit<ScopeEvidenceSlot>>,
    scope_selected_arms: std::vec::Vec<RouteScopeSelectedArmSlot>,
    lane_dense_by_lane: std::vec::Vec<DenseLaneOrdinal>,
    lane_route_arm_lens: std::vec::Vec<u8>,
    lane_linger_counts: std::vec::Vec<u8>,
    lane_linger_words: std::vec::Vec<LaneWord>,
    lane_offer_linger_words: std::vec::Vec<LaneWord>,
    active_offer_lane_words: std::vec::Vec<LaneWord>,
}

impl RouteStateFixtureStorage {
    fn live_capacity_words(&self) -> usize {
        self.route_arm.len()
            + self.lane_offer_state.len()
            + self.scope_evidence_slots.len()
            + self.scope_selected_arms.len()
            + self.lane_dense_by_lane.len()
            + self.lane_route_arm_lens.len()
            + self.lane_linger_counts.len()
            + self.lane_linger_words.len()
            + self.lane_offer_linger_words.len()
            + self.active_offer_lane_words.len()
    }
}

impl Drop for RouteStateFixture {
    fn drop(&mut self) {
        core::hint::black_box(self.storage.live_capacity_words());
    }
}

fn route_state_fixture(lanes: usize, route_depth: usize, scope_count: usize) -> RouteStateFixture {
    let lane_words = lane_word_count(lanes);
    let mut lane_dense_by_lane: std::vec::Vec<DenseLaneOrdinal> = (0..lanes)
        .map(|lane| DenseLaneOrdinal::new(lane).expect("test lane dense ordinal"))
        .collect();
    let mut route_arm_storage = std::vec::Vec::with_capacity(lanes * route_depth);
    route_arm_storage.resize(lanes * route_depth, RouteArmState::EMPTY);
    let mut lane_offer_state_storage = std::vec::Vec::with_capacity(lanes);
    lane_offer_state_storage.resize(lanes, LaneOfferState::EMPTY);
    let mut scope_evidence_slots = std::vec::Vec::<MaybeUninit<ScopeEvidenceSlot>>::new();
    let mut scope_selected_arms = std::vec::Vec::with_capacity(scope_count);
    scope_selected_arms.resize(scope_count, RouteScopeSelectedArmSlot::EMPTY);
    let mut lane_route_arm_lens = std::vec::Vec::with_capacity(lanes);
    lane_route_arm_lens.resize(lanes, 0u8);
    let mut lane_linger_counts = std::vec::Vec::with_capacity(lanes);
    lane_linger_counts.resize(lanes, 0u8);
    let mut lane_linger_words = std::vec::Vec::with_capacity(lane_words);
    lane_linger_words.resize(lane_words, 0usize);
    let mut lane_offer_linger_words = std::vec::Vec::with_capacity(lane_words);
    lane_offer_linger_words.resize(lane_words, 0usize);
    let mut active_offer_lane_words = std::vec::Vec::with_capacity(lane_words);
    active_offer_lane_words.resize(lane_words, 0usize);
    let mut state = MaybeUninit::<RouteState>::uninit();
    unsafe {
        RouteState::init_empty(
            state.as_mut_ptr(),
            route_arm_storage.as_mut_ptr(),
            lane_offer_state_storage.as_mut_ptr(),
            scope_evidence_slots
                .as_mut_ptr()
                .cast::<ScopeEvidenceSlot>(),
            scope_selected_arms.as_mut_ptr(),
            lane_dense_by_lane.as_mut_ptr(),
            lanes,
            lane_route_arm_lens.as_mut_ptr(),
            lane_linger_counts.as_mut_ptr(),
            lane_linger_words.as_mut_ptr(),
            lane_offer_linger_words.as_mut_ptr(),
            active_offer_lane_words.as_mut_ptr(),
            lanes,
            lane_words,
            lanes,
            route_depth,
            0,
            scope_count,
        );
    }
    RouteStateFixture {
        state: unsafe { state.assume_init() },
        storage: RouteStateFixtureStorage {
            route_arm: route_arm_storage,
            lane_offer_state: lane_offer_state_storage,
            scope_evidence_slots,
            scope_selected_arms,
            lane_dense_by_lane,
            lane_route_arm_lens,
            lane_linger_counts,
            lane_linger_words,
            lane_offer_linger_words,
            active_offer_lane_words,
        },
    }
}

#[test]
fn route_state_keeps_lane_255_addressable_in_full_lane_domain() {
    const LANES: usize = 256;
    let lane_words = lane_word_count(LANES);
    let mut lane_dense_by_lane: std::vec::Vec<DenseLaneOrdinal> = (0..LANES)
        .map(|lane| DenseLaneOrdinal::new(lane).expect("test lane dense ordinal"))
        .collect();
    let mut route_arm_storage = std::vec::Vec::with_capacity(LANES);
    route_arm_storage.resize(LANES, RouteArmState::EMPTY);
    let mut lane_offer_state_storage = std::vec::Vec::with_capacity(LANES);
    lane_offer_state_storage.resize(LANES, LaneOfferState::EMPTY);
    let mut scope_evidence_slots = std::vec::Vec::<MaybeUninit<ScopeEvidenceSlot>>::new();
    let mut scope_selected_arms = std::vec::Vec::with_capacity(1);
    scope_selected_arms.resize(1, RouteScopeSelectedArmSlot::EMPTY);
    let mut lane_route_arm_lens = std::vec::Vec::with_capacity(LANES);
    lane_route_arm_lens.resize(LANES, 0u8);
    let mut lane_linger_counts = std::vec::Vec::with_capacity(LANES);
    lane_linger_counts.resize(LANES, 0u8);
    let mut lane_linger_words = std::vec::Vec::with_capacity(lane_words);
    lane_linger_words.resize(lane_words, 0usize);
    let mut lane_offer_linger_words = std::vec::Vec::with_capacity(lane_words);
    lane_offer_linger_words.resize(lane_words, 0usize);
    let mut active_offer_lane_words = std::vec::Vec::with_capacity(lane_words);
    active_offer_lane_words.resize(lane_words, 0usize);
    let mut state = MaybeUninit::<RouteState>::uninit();
    unsafe {
        RouteState::init_empty(
            state.as_mut_ptr(),
            route_arm_storage.as_mut_ptr(),
            lane_offer_state_storage.as_mut_ptr(),
            scope_evidence_slots
                .as_mut_ptr()
                .cast::<ScopeEvidenceSlot>(),
            scope_selected_arms.as_mut_ptr(),
            lane_dense_by_lane.as_mut_ptr(),
            LANES,
            lane_route_arm_lens.as_mut_ptr(),
            lane_linger_counts.as_mut_ptr(),
            lane_linger_words.as_mut_ptr(),
            lane_offer_linger_words.as_mut_ptr(),
            active_offer_lane_words.as_mut_ptr(),
            LANES,
            lane_words,
            LANES,
            1,
            0,
            1,
        );
    }
    let mut state = unsafe { state.assume_init() };
    let scope = ScopeId::route(1);

    assert_eq!(state.lane_route_arm_len(255), 0);
    let row = state
        .preflight_selected_route_commit(255, scope, 0, 1, false)
        .expect("high lane route arm should preflight");
    assert!(state.apply_prepared_route_selection(row, test_commit_delta_apply_permit()));
    assert_eq!(state.lane_route_arm_len(255), 1);
    assert_eq!(state.selected_arm_for_scope_slot(0), Some(1));
}

#[test]
fn branch_commit_preflight_error_records_no_route_decisions() {
    let mut fixture = route_state_fixture(2, 1, 1);
    let state = &mut fixture.state;
    let scope = ScopeId::route(1);
    let row = state
        .preflight_selected_route_commit(0, scope, 0, 0, false)
        .expect("first route arm should preflight");
    assert!(state.apply_prepared_route_selection(row, test_commit_delta_apply_permit()));
    assert_eq!(state.selected_arm_for_scope_slot(0), Some(0));

    assert!(
        state
            .preflight_selected_route_commit(1, scope, 0, 1, false)
            .is_none(),
        "conflicting arm must fail in preflight"
    );
    assert_eq!(state.lane_route_arm_len(1), 0);
    assert_eq!(state.lane_route_arm_len(0), 1);
    assert_eq!(state.selected_arm_for_scope_slot(0), Some(0));
}

#[test]
fn branch_commit_publish_accepts_same_arm_after_preflight() {
    let mut fixture = route_state_fixture(2, 2, 1);
    let state = &mut fixture.state;
    let scope = ScopeId::route(1);
    let first = state
        .preflight_selected_route_commit(0, scope, 0, 1, false)
        .expect("first route arm should preflight");
    assert!(state.apply_prepared_route_selection(first, test_commit_delta_apply_permit()));
    let second = state
        .preflight_selected_route_commit(1, scope, 0, 1, false)
        .expect("same route arm should preflight");
    assert!(state.apply_prepared_route_selection(second, test_commit_delta_apply_permit()));
    assert_eq!(state.lane_route_arm_len(0), 1);
    assert_eq!(state.lane_route_arm_len(1), 1);
    assert_eq!(state.selected_arm_for_scope_slot(0), Some(1));
}

#[test]
fn non_linger_route_choice_is_not_blocked_by_lane_route_stack_depth() {
    let mut fixture = route_state_fixture(1, 1, 2);
    let state = &mut fixture.state;
    let first_scope = ScopeId::route(1);
    let second_scope = ScopeId::route(2);

    let first = state
        .preflight_selected_route_commit(0, first_scope, 0, 0, false)
        .expect("first non-linger route arm should preflight");
    assert!(state.apply_prepared_route_selection(first, test_commit_delta_apply_permit()));
    assert_eq!(state.lane_route_arm_len(0), 1);

    let second = state
        .preflight_selected_route_commit(0, second_scope, 1, 1, false)
        .expect("second non-linger route arm should not depend on lane stack depth");
    assert!(state.apply_prepared_route_selection(second, test_commit_delta_apply_permit()));
    assert_eq!(
        state.lane_route_arm_len(0),
        1,
        "non-linger conflict facts need not grow the resident lane stack"
    );
    assert_eq!(state.selected_arm_for_scope_slot(0), Some(0));
    assert_eq!(state.selected_arm_for_scope_slot(1), Some(1));

    assert!(
        state
            .preflight_selected_route_commit(0, ScopeId::route(3), 1, 0, true)
            .is_none(),
        "linger route choices still require lane tracking for epoch/rewind"
    );
}

#[test]
fn route_commit_row_workspace_accepts_more_than_64_route_scopes() {
    let mut storage = std::vec::Vec::new();
    storage.resize(71, SelectedRouteCommitRow::EMPTY);
    let mut workspace = MaybeUninit::<RouteCommitRowWorkspace>::uninit();
    unsafe {
        RouteCommitRowWorkspace::init(workspace.as_mut_ptr(), storage.as_mut_ptr(), 71);
    }
    let mut workspace = unsafe { workspace.assume_init() };
    let list = workspace
        .begin()
        .expect("route commit row workspace derives from route scope count");

    assert_eq!(list.len(), 0);
}

#[test]
fn decode_commit_row_workspace_accepts_more_than_64_route_scopes() {
    let mut storage = std::vec::Vec::new();
    storage.resize(71, SelectedRouteCommitRow::EMPTY);
    let mut workspace = MaybeUninit::<RouteCommitRowWorkspace>::uninit();
    unsafe {
        RouteCommitRowWorkspace::init(workspace.as_mut_ptr(), storage.as_mut_ptr(), 71);
    }
    let mut workspace = unsafe { workspace.assume_init() };
    let list = workspace
        .begin()
        .expect("decode commit plan uses shared route-scope row workspace");

    assert_eq!(list.len(), 0);
}
