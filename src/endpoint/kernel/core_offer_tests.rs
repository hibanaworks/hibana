//! Offer-path kernel regression tests.

mod abort_control_kind {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/abort_control.rs"
    ));
}
mod route_control_kinds {
    extern crate self as hibana;
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/route_control_kinds.rs"
    ));
}
mod snapshot_control_kind {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/snapshot_control.rs"
    ));
}

use super::super::offer::LaneIngressEvidence;
use super::*;
use crate::binding::{Channel, IngressEvidence, TransportOpsError};
use crate::control::cap::mint::{ControlOp, GenericCapToken, ResourceKind};
use crate::control::cap::resource_kinds::RouteDecisionKind;
use crate::control::cluster::core::SessionCluster;
use crate::g::{self, Msg, Role};
use crate::global::role_program::{
    DenseLaneOrdinal, LaneSet, LaneSetView, LaneWord, RoleProgram, lane_word_count, project,
};
use crate::global::steps::{PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
use crate::observe::core::TapEvent;
use crate::runtime::config::{Config, CounterClock};
use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse, RING_EVENTS};
use crate::transport::{FrameLabel, FrameLabelMask, Transport, TransportError, wire::Payload};
use abort_control_kind::{ABORT_CONTROL_LOGICAL, AbortControl};
use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    marker::PhantomData,
    mem::{MaybeUninit, align_of, size_of},
    pin::pin,
    task::{Context, Poll},
};
use futures::task::noop_waker_ref;
use route_control_kinds::RouteControl;
use snapshot_control_kind::{SNAPSHOT_CONTROL_LOGICAL, SnapshotControl};
use std::{task::Waker, thread_local};

type SendOnly<const LANE: u8, S, D, M> = StepCons<SendStep<S, D, M, LANE>, StepNil>;
type BranchSteps<L, R> = RouteSteps<L, R>;
const PICO_OFFER_FIXTURE_SLAB_CAPACITY: usize = 64 * 1024;
const LARGE_HOST_OFFER_FIXTURE_SLAB_CAPACITY: usize = 1_048_576;
const OFFER_FIXTURE_SLAB_CAPACITY: usize = LARGE_HOST_OFFER_FIXTURE_SLAB_CAPACITY;
const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;
const TEST_LOOP_BREAK_LOGICAL: u8 = 0xA2;
const TEST_ROUTE_DECISION_LOGICAL: u8 = 0xA3;
const TEST_LOOP_CONTINUE_FRAME: u8 = 2;
const TEST_LOOP_BREAK_FRAME: u8 = 3;
const ROUTE_HINT_RIGHT_LABEL: u8 = 122;
type RouteHintRightKind = RouteControl<0>;

fn frame_label_for_cursor_label(cursor: &PhaseCursor, label: u8) -> u8 {
    let idx = cursor
        .seek_label_index(label)
        .expect("logical label must exist in cursor typestate");
    if let Some(meta) = cursor.try_recv_meta_at(idx) {
        return meta.frame_label;
    }
    if let Some(meta) = cursor.try_send_meta_at(idx) {
        return meta.frame_label;
    }
    if let Some(meta) = cursor.try_local_meta_at(idx) {
        return meta.frame_label;
    }
    panic!("logical label must reference a local action");
}

fn overwrite_global_active_entries_fixture<
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B,
>(
    endpoint: &mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    src: ActiveEntrySet,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot,
{
    endpoint.init_global_frontier_scratch_if_needed();
    let mut dst = endpoint.global_active_entries();
    dst.copy_from(src);
}

fn overwrite_global_frontier_observed_fixture<
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B,
>(
    endpoint: &mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    src: ObservedEntrySet,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot,
{
    endpoint.init_global_frontier_scratch_if_needed();
    let mut key = endpoint.cached_global_frontier_observation_key();
    key.copy_slots_from_observed_entries(src);
    endpoint.frontier_state.global_frontier_observed = src.summary();
}

fn overwrite_global_frontier_observed_key_fixture<
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B,
>(
    endpoint: &mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    key: FrontierObservationKey,
) where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot,
{
    endpoint.init_global_frontier_scratch_if_needed();
    let mut dst = endpoint.cached_global_frontier_observation_key();
    dst.copy_from(key);
}

struct CursorSend<M>(PhantomData<M>);

impl<M> CursorSend<M>
where
    M: MessageSpec + SendableLabel,
{
    fn run<'a, 'r, A, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
        endpoint: &'a mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        arg: A,
    ) -> impl Future<Output = SendResult<()>> + 'a
    where
        M: 'a,
        M::Payload: crate::transport::wire::WireEncode + 'a,
        M::ControlKind: crate::global::ControlPayloadKind,
        A: crate::endpoint::flow::ErasedSendInput<'a, M>,
        T: Transport + 'r,
        U: LabelUniverse,
        C: crate::runtime::config::Clock,
        E: crate::control::cap::mint::EpochTable,
        Mint: crate::control::cap::mint::MintConfigMarker<
                Policy: crate::control::cap::mint::AllowsEndpointMint,
            >,
        B: crate::binding::BindingSlot + 'r,
        A: 'a,
        'r: 'a,
    {
        let desc = crate::endpoint::flow::send_desc::<M>();
        let mut preview = Some(endpoint.preview_flow_meta(desc.logical_label()));
        let mut payload = crate::endpoint::flow::ErasedSendInput::into_payload(arg)
            .map(crate::endpoint::kernel::RawSendPayload::from_typed::<M::Payload>);
        let mut state = None;

        core::future::poll_fn(move |cx| {
            if state.is_none() {
                let preview = preview.take().expect("cursor send polled after completion");
                let preview = match preview {
                    Ok(preview) => preview,
                    Err(err) => return Poll::Ready(Err(err)),
                };
                let (meta, preview_cursor_index) = preview.into_parts();
                state = Some(SendState::Init {
                    descriptor: desc.bind_frame_label(meta.frame_label),
                    meta,
                    preview_cursor_index: Some(preview_cursor_index),
                    payload: payload.take(),
                });
            }

            let state = state
                .as_mut()
                .expect("cursor send state must be initialized");
            match endpoint.poll_send_state(state, cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            }
        })
    }

    fn run_with_meta<'a, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
        endpoint: &'a mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        meta: SendMeta,
        payload: Option<&'a M::Payload>,
    ) -> impl Future<Output = SendResult<()>> + 'a
    where
        M: 'a,
        M::Payload: crate::transport::wire::WireEncode + 'a,
        M::ControlKind: crate::global::ControlPayloadKind,
        T: Transport + 'r,
        U: LabelUniverse,
        C: crate::runtime::config::Clock,
        E: crate::control::cap::mint::EpochTable,
        Mint: crate::control::cap::mint::MintConfigMarker<
                Policy: crate::control::cap::mint::AllowsEndpointMint,
            >,
        B: crate::binding::BindingSlot + 'r,
        'r: 'a,
    {
        let desc = crate::endpoint::flow::send_desc::<M>();
        let mut state = SendState::Init {
            descriptor: desc.bind_frame_label(meta.frame_label),
            meta,
            preview_cursor_index: None,
            payload: payload.map(crate::endpoint::kernel::RawSendPayload::from_typed::<M::Payload>),
        };

        core::future::poll_fn(move |cx| match endpoint.poll_send_state(&mut state, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        })
    }
}

struct CursorOffer<'a, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot + 'r,
{
    endpoint: &'a mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    state: OfferState<'r>,
}

impl<'a, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B> Future
    for CursorOffer<'a, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot + 'r,
    'r: 'a,
{
    type Output = RecvResult<MaterializedRouteBranch<'r>>;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.endpoint.poll_offer_state(&mut this.state, cx)
    }
}

fn cursor_offer<'a, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    endpoint: &'a mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
) -> CursorOffer<'a, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot + 'r,
    'r: 'a,
{
    CursorOffer {
        endpoint,
        state: OfferState::new(),
    }
}

fn branch_label(branch: &MaterializedRouteBranch<'_>) -> u8 {
    branch.label
}

fn branch_scope(branch: &MaterializedRouteBranch<'_>) -> ScopeId {
    branch.branch_meta.scope_id
}

fn branch_has_staged_payload(branch: &MaterializedRouteBranch<'_>) -> bool {
    branch.staged_payload.is_some()
}

fn branch_has_transport_payload(branch: &MaterializedRouteBranch<'_>) -> bool {
    matches!(branch.staged_payload, Some(StagedPayload::Transport { .. }))
}

struct CursorDecode<M>(PhantomData<M>);

impl<M> CursorDecode<M>
where
    M: MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    fn run<'a, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
        endpoint: &'a mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        branch: MaterializedRouteBranch<'r>,
    ) -> CursorDecodeFuture<'a, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B, M>
    where
        T: Transport + 'r,
        U: LabelUniverse,
        C: crate::runtime::config::Clock,
        E: crate::control::cap::mint::EpochTable,
        Mint: crate::control::cap::mint::MintConfigMarker,
        B: crate::binding::BindingSlot + 'r,
    {
        CursorDecodeFuture {
            endpoint: core::ptr::from_mut(endpoint),
            state: super::super::decode::DecodeState::new(branch),
            _borrow: PhantomData,
            _msg: PhantomData,
        }
    }
}

struct CursorDecodeFuture<'a, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B, M>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot + 'r,
    M: MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    endpoint: *mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    state: super::super::decode::DecodeState<'r>,
    _borrow: PhantomData<&'a mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>>,
    _msg: PhantomData<M>,
}

impl<'a, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B, M> Future
    for CursorDecodeFuture<'a, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B, M>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot + 'r,
    M: MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    type Output = RecvResult<<M::Payload as crate::transport::wire::WirePayload>::Decoded<'a>>;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let endpoint = unsafe { &mut *this.endpoint };
        let desc = DecodeRuntimeSpec::new(
            <M as MessageSpec>::LOGICAL_LABEL,
            <M::ControlKind as crate::global::ControlPayloadKind>::IS_CONTROL,
            |payload| {
                <M::Payload as crate::transport::wire::WirePayload>::decode_payload(payload)
                    .map(|_| ())
            },
            |scratch| {
                <M::Payload as crate::transport::wire::WirePayload>::synthetic_payload(scratch)
            },
        );
        match endpoint.poll_decode_state(desc, &mut this.state, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                let payload = super::super::lane_port::shrink_payload(payload);
                Poll::Ready(
                    <M::Payload as crate::transport::wire::WirePayload>::decode_payload(payload)
                        .map_err(RecvError::Codec),
                )
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }
}

impl<'a, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B, M> Drop
    for CursorDecodeFuture<'a, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B, M>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::BindingSlot + 'r,
    M: MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    fn drop(&mut self) {
        if self.state.restore_on_drop
            && let Some(branch) = self.state.branch.take()
        {
            unsafe {
                (&mut *self.endpoint).restore_materialized_route_branch(branch.into());
            }
        }
    }
}

const fn max_usize(values: &[usize]) -> usize {
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

fn with_lane_set_view<R>(lanes: &[usize], f: impl FnOnce(LaneSetView) -> R) -> R {
    let lane_limit = max_usize(lanes).saturating_add(1).max(1);
    let mut lane_words = std::vec![0 as LaneWord; lane_word_count(lane_limit)];
    let mut lane_set = LaneSet::from_parts(lane_words.as_mut_ptr(), lane_words.len());
    let mut idx = 0usize;
    while idx < lanes.len() {
        lane_set.insert(lanes[idx]);
        idx += 1;
    }
    f(lane_set.view())
}

fn assert_lane_set_eq(set: LaneSetView, lane_limit: usize, expected: &[u8]) {
    let mut lanes = std::vec![u8::MAX; lane_limit.max(expected.len()).max(1)];
    let len = set.write_lane_indices(lane_limit, &mut lanes);
    assert_eq!(len, expected.len(), "lane-set length mismatch");
    assert_eq!(&lanes[..len], expected, "lane-set contents mismatch");
}

#[test]
fn with_lane_set_view_keeps_sparse_high_lane_indices_exact() {
    with_lane_set_view(&[33], |set| {
        assert_lane_set_eq(set, 34, &[33]);
    });
    with_lane_set_view(&[0, 65], |set| {
        assert_lane_set_eq(set, 66, &[0, 65]);
    });
}

fn assert_buffered_lanes_eq(
    inbox: &BindingInbox,
    frame_label_mask: FrameLabelMask,
    expected: &[u8],
) {
    let mut lanes = std::vec![u8::MAX; expected.len().max(1)];
    let len = inbox.buffered_lanes_for_frame_labels(frame_label_mask, &mut lanes);
    assert_eq!(len, expected.len(), "buffered lane count mismatch");
    assert_eq!(&lanes[..len], expected, "buffered lanes mismatch");
}

fn with_test_binding_inbox<const ACTIVE_LANES: usize, R>(
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

fn assert_nonempty_lanes_eq(inbox: &BindingInbox, lane_limit: usize, expected: &[u8]) {
    let mut lanes = std::vec![u8::MAX; lane_limit.max(expected.len()).max(1)];
    let len = inbox
        .nonempty_lanes()
        .write_lane_indices(lane_limit, &mut lanes);
    assert_eq!(len, expected.len(), "nonempty lane count mismatch");
    assert_eq!(&lanes[..len], expected, "nonempty lane contents mismatch");
}

fn with_active_entry_set_storage<R>(
    capacity: usize,
    f: impl FnOnce(&mut ActiveEntrySet) -> R,
) -> R {
    let mut slots = std::vec![ActiveEntrySlot::EMPTY; capacity.max(1)];
    let mut entries = ActiveEntrySet::from_parts(slots.as_mut_ptr(), slots.len());
    entries.clear();
    f(&mut entries)
}

fn active_entry_set_storage(capacity: usize) -> (std::vec::Vec<ActiveEntrySlot>, ActiveEntrySet) {
    let mut slots = std::vec![ActiveEntrySlot::EMPTY; capacity.max(1)];
    let mut entries = ActiveEntrySet::from_parts(slots.as_mut_ptr(), slots.len());
    entries.clear();
    (slots, entries)
}

fn active_entry_set_from_pairs(
    entries: &[(usize, u8)],
) -> (std::vec::Vec<ActiveEntrySlot>, ActiveEntrySet) {
    let (slots, mut active_entries) = active_entry_set_storage(entries.len());
    for &(entry_idx, lane_idx) in entries {
        assert!(active_entries.insert_entry(entry_idx, lane_idx));
    }
    (slots, active_entries)
}

fn observed_entry_set_storage(
    capacity: usize,
) -> (std::vec::Vec<FrontierObservationSlot>, ObservedEntrySet) {
    let mut slots = std::vec![FrontierObservationSlot::EMPTY; capacity.max(1)];
    let mut entries = ObservedEntrySet::from_parts(slots.as_mut_ptr(), slots.len());
    entries.clear();
    (slots, entries)
}

fn observed_entry_set_from_states(
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

fn frontier_observation_key_storage(
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

fn copied_frontier_observation_key_storage(
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

fn with_frontier_observation_key_storage<R>(
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

fn frontier_candidates<const N: usize>() -> [FrontierCandidate; N] {
    [FrontierCandidate::EMPTY; N]
}

fn frontier_visit_slots<const N: usize>() -> [ScopeId; N] {
    [ScopeId::none(); N]
}

fn frontier_snapshot_fixture<const N: usize>(
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

fn frontier_visit_set_fixture(slots: &mut [ScopeId]) -> FrontierVisitSet {
    unsafe { FrontierVisitSet::from_parts(slots.as_mut_ptr(), slots.len()) }
}

fn empty_frontier_visit_set() -> FrontierVisitSet {
    unsafe { FrontierVisitSet::from_parts(core::ptr::null_mut(), 0) }
}

const fn offer_endpoint_slot_bytes<const ROLE: u8, T, B>(lane_capacity: usize) -> usize
where
    T: Transport + 'static,
    B: crate::binding::BindingSlot + 'static,
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

type OfferHintCluster =
    SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>;
type OfferHintControllerEndpoint = CursorEndpoint<
    'static,
    0,
    HintOnlyTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
type OfferHintWorkerEndpoint = CursorEndpoint<
    'static,
    1,
    HintOnlyTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
type OfferHintWorkerBindingEndpoint = CursorEndpoint<
    'static,
    1,
    HintOnlyTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    TestBinding,
>;
type OfferHintLaneAwareWorkerEndpoint = CursorEndpoint<
    'static,
    1,
    HintOnlyTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    LaneAwareTestBinding,
>;
type DeepRightOuterLeftMsg = Msg<0x50, u8>;
type DeepRightMiddleLeftMsg = Msg<0x51, u8>;
type DeepRightThirdLeftMsg = Msg<0x52, u8>;
type DeepRightFinalLeftMsg = Msg<0x53, u8>;
type DeepRightFinalRightMsg = Msg<0x55, u8>;
const DEEP_RIGHT_FINAL_RIGHT_FRAME: u8 = 4;
type DeepRightStaticRouteLeftMsg =
    Msg<{ TEST_ROUTE_DECISION_LOGICAL }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>;
type DeepRightStaticRouteRightMsg =
    Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>;
type DeepRightFinalDecisionLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, DeepRightFinalLeftMsg>,
>;
type DeepRightFinalDecisionRightSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteRightMsg>,
    SendOnly<0, Role<0>, Role<1>, DeepRightFinalRightMsg>,
>;
type DeepRightFinalDecisionSteps =
    BranchSteps<DeepRightFinalDecisionLeftSteps, DeepRightFinalDecisionRightSteps>;
type DeepRightThirdLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, DeepRightThirdLeftMsg>,
>;
type DeepRightThirdRightSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteRightMsg>,
    DeepRightFinalDecisionSteps,
>;
type DeepRightThirdSteps = BranchSteps<DeepRightThirdLeftSteps, DeepRightThirdRightSteps>;
type DeepRightMiddleLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, DeepRightMiddleLeftMsg>,
>;
type DeepRightMiddleRightSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteRightMsg>, DeepRightThirdSteps>;
type DeepRightMiddleSteps = BranchSteps<DeepRightMiddleLeftSteps, DeepRightMiddleRightSteps>;
type DeepRightOuterLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, DeepRightOuterLeftMsg>,
>;
type DeepRightOuterRightSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, DeepRightStaticRouteRightMsg>, DeepRightMiddleSteps>;
type DeepRightProgramSteps = BranchSteps<DeepRightOuterLeftSteps, DeepRightOuterRightSteps>;
#[allow(non_snake_case)]
fn DEEP_RIGHT_FINAL_DECISION() -> g::Program<DeepRightFinalDecisionSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, DeepRightFinalLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, DeepRightFinalRightMsg, 0>(),
        ),
    )
}

#[allow(non_snake_case)]
fn DEEP_RIGHT_THIRD() -> g::Program<DeepRightThirdSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, DeepRightThirdLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteRightMsg, 0>(),
            DEEP_RIGHT_FINAL_DECISION(),
        ),
    )
}

#[allow(non_snake_case)]
fn DEEP_RIGHT_MIDDLE() -> g::Program<DeepRightMiddleSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, DeepRightMiddleLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteRightMsg, 0>(),
            DEEP_RIGHT_THIRD(),
        ),
    )
}

#[allow(non_snake_case)]
fn DEEP_RIGHT_PROGRAM() -> g::Program<DeepRightProgramSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, DeepRightOuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, DeepRightStaticRouteRightMsg, 0>(),
            DEEP_RIGHT_MIDDLE(),
        ),
    )
}
type NestedStaticOuterLeftMsg = Msg<0x50, u8>;
type NestedStaticLeafLeftMsg = Msg<0x51, u8>;
type NestedStaticLeafRightMsg = Msg<0x52, u8>;
type NestedStaticMiddleRightMsg = Msg<0x53, u8>;
type NestedStaticRouteLeftMsg =
    Msg<{ TEST_ROUTE_DECISION_LOGICAL }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>;
type NestedStaticRouteRightMsg =
    Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>;
type NestedStaticInnerLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, NestedStaticRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, NestedStaticLeafLeftMsg>,
>;
type NestedStaticInnerRightSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, NestedStaticRouteRightMsg>,
    SendOnly<0, Role<0>, Role<1>, NestedStaticLeafRightMsg>,
>;
type NestedStaticInnerSteps = BranchSteps<NestedStaticInnerLeftSteps, NestedStaticInnerRightSteps>;
type NestedStaticMiddleLeftSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, NestedStaticRouteLeftMsg>, NestedStaticInnerSteps>;
type NestedStaticMiddleRightSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, NestedStaticRouteRightMsg>,
    SendOnly<0, Role<0>, Role<1>, NestedStaticMiddleRightMsg>,
>;
type NestedStaticMiddleSteps =
    BranchSteps<NestedStaticMiddleLeftSteps, NestedStaticMiddleRightSteps>;
type NestedStaticOuterLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, NestedStaticRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, NestedStaticOuterLeftMsg>,
>;
type NestedStaticOuterRightSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, NestedStaticRouteRightMsg>, NestedStaticMiddleSteps>;
type NestedStaticProgramSteps =
    BranchSteps<NestedStaticOuterLeftSteps, NestedStaticOuterRightSteps>;
#[allow(non_snake_case)]
fn NESTED_STATIC_INNER() -> g::Program<NestedStaticInnerSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, NestedStaticLeafLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, NestedStaticLeafRightMsg, 0>(),
        ),
    )
}

#[allow(non_snake_case)]
fn NESTED_STATIC_MIDDLE() -> g::Program<NestedStaticMiddleSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteLeftMsg, 0>(),
            NESTED_STATIC_INNER(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, NestedStaticMiddleRightMsg, 0>(),
        ),
    )
}

#[allow(non_snake_case)]
fn NESTED_STATIC_PROGRAM() -> g::Program<NestedStaticProgramSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, NestedStaticOuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, NestedStaticRouteRightMsg, 0>(),
            NESTED_STATIC_MIDDLE(),
        ),
    )
}

#[allow(non_snake_case)]
fn NESTED_STATIC_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&NESTED_STATIC_PROGRAM())
}

#[allow(non_snake_case)]
fn NESTED_STATIC_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&NESTED_STATIC_PROGRAM())
}
type LoopContinueScopedContinueMsg = Msg<
    { TEST_LOOP_CONTINUE_LOGICAL },
    GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
    crate::control::cap::resource_kinds::LoopContinueKind,
>;
type LoopContinueScopedBreakMsg = Msg<
    { TEST_LOOP_BREAK_LOGICAL },
    GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
    crate::control::cap::resource_kinds::LoopBreakKind,
>;
type LoopContinueScopedRouteLeftMsg =
    Msg<{ TEST_ROUTE_DECISION_LOGICAL }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>;
type LoopContinueScopedRouteRightMsg =
    Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>;
type LoopContinueScopedInnerLeftMsg = Msg<90, u8>;
type LoopContinueScopedInnerRightMsg = Msg<91, u8>;
type LoopContinueScopedInnerLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, LoopContinueScopedInnerLeftMsg>,
>;
type LoopContinueScopedInnerRightSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteRightMsg>,
    SendOnly<0, Role<0>, Role<1>, LoopContinueScopedInnerRightMsg>,
>;
type LoopContinueScopedInnerRouteSteps =
    BranchSteps<LoopContinueScopedInnerLeftSteps, LoopContinueScopedInnerRightSteps>;
type LoopContinueScopedContinueArmSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedContinueMsg>,
    LoopContinueScopedInnerRouteSteps,
>;
type LoopContinueScopedProgramSteps = BranchSteps<
    LoopContinueScopedContinueArmSteps,
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedBreakMsg>,
>;
type LoopSemanticsProgramSteps = BranchSteps<
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedContinueMsg>,
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedBreakMsg>,
>;
#[allow(non_snake_case)]
fn LOOP_SEMANTICS_PROGRAM() -> g::Program<LoopSemanticsProgramSteps> {
    g::route(
        g::send::<Role<0>, Role<0>, LoopContinueScopedContinueMsg, 0>(),
        g::send::<Role<0>, Role<0>, LoopContinueScopedBreakMsg, 0>(),
    )
}

#[allow(non_snake_case)]
fn LOOP_SEMANTICS_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&LOOP_SEMANTICS_PROGRAM())
}

#[allow(non_snake_case)]
fn LOOP_CONTINUE_SCOPED_PROGRAM() -> g::Program<LoopContinueScopedProgramSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueScopedContinueMsg, 0>(),
            g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg, 0>(),
                    g::send::<Role<0>, Role<1>, LoopContinueScopedInnerLeftMsg, 0>(),
                ),
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteRightMsg, 0>(),
                    g::send::<Role<0>, Role<1>, LoopContinueScopedInnerRightMsg, 0>(),
                ),
            ),
        ),
        g::send::<Role<0>, Role<0>, LoopContinueScopedBreakMsg, 0>(),
    )
}

#[allow(non_snake_case)]
fn LOOP_CONTINUE_SCOPED_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&LOOP_CONTINUE_SCOPED_PROGRAM())
}
const LOOP_CONTINUE_PASSIVE_RIGHT_REPLY_LABEL: u8 = 0x51;
type LoopContinuePassiveOuterLeftMsg = Msg<90, u8>;
type LoopContinuePassiveRightReplyMsg = Msg<{ LOOP_CONTINUE_PASSIVE_RIGHT_REPLY_LABEL }, u8>;
type LoopContinuePassiveInnerLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, LoopContinuePassiveOuterLeftMsg>,
>;
type LoopContinuePassiveInnerRightSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteRightMsg>,
    SendOnly<0, Role<0>, Role<1>, LoopContinuePassiveRightReplyMsg>,
>;
type LoopContinuePassiveInnerRouteSteps =
    BranchSteps<LoopContinuePassiveInnerLeftSteps, LoopContinuePassiveInnerRightSteps>;
type LoopContinuePassiveProgramSteps = BranchSteps<
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedContinueMsg>,
        LoopContinuePassiveInnerRouteSteps,
    >,
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedBreakMsg>,
>;
#[allow(non_snake_case)]
fn LOOP_CONTINUE_PASSIVE_PROGRAM() -> g::Program<LoopContinuePassiveProgramSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueScopedContinueMsg, 0>(),
            g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg, 0>(),
                    g::send::<Role<0>, Role<1>, LoopContinuePassiveOuterLeftMsg, 0>(),
                ),
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteRightMsg, 0>(),
                    g::send::<Role<0>, Role<1>, LoopContinuePassiveRightReplyMsg, 0>(),
                ),
            ),
        ),
        g::send::<Role<0>, Role<0>, LoopContinueScopedBreakMsg, 0>(),
    )
}

#[allow(non_snake_case)]
fn LOOP_CONTINUE_PASSIVE_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&LOOP_CONTINUE_PASSIVE_PROGRAM())
}

#[allow(non_snake_case)]
fn LOOP_CONTINUE_PASSIVE_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&LOOP_CONTINUE_PASSIVE_PROGRAM())
}
type NestedDispatchOuterLeftMsg = Msg<0x10, u8>;
type NestedDispatchLeafLeftMsg = Msg<0x51, u8>;
type NestedDispatchLeafRightMsg = Msg<0x52, u8>;
type NestedDispatchInnerLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, NestedDispatchLeafLeftMsg>,
>;
type NestedDispatchInnerRightSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteRightMsg>,
    SendOnly<0, Role<0>, Role<1>, NestedDispatchLeafRightMsg>,
>;
type NestedDispatchInnerSteps =
    BranchSteps<NestedDispatchInnerLeftSteps, NestedDispatchInnerRightSteps>;
type NestedDispatchOuterLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, NestedDispatchOuterLeftMsg>,
>;
type NestedDispatchProgramSteps = BranchSteps<
    NestedDispatchOuterLeftSteps,
    SeqSteps<
        SendOnly<0, Role<0>, Role<0>, LoopContinueScopedRouteRightMsg>,
        NestedDispatchInnerSteps,
    >,
>;
#[allow(non_snake_case)]
fn NESTED_DISPATCH_PROGRAM() -> g::Program<NestedDispatchProgramSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, NestedDispatchOuterLeftMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueScopedRouteRightMsg, 0>(),
            g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteLeftMsg, 0>(),
                    g::send::<Role<0>, Role<1>, NestedDispatchLeafLeftMsg, 0>(),
                ),
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueScopedRouteRightMsg, 0>(),
                    g::send::<Role<0>, Role<1>, NestedDispatchLeafRightMsg, 0>(),
                ),
            ),
        ),
    )
}

#[allow(non_snake_case)]
fn NESTED_DISPATCH_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&NESTED_DISPATCH_PROGRAM())
}

#[allow(non_snake_case)]
fn NESTED_DISPATCH_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&NESTED_DISPATCH_PROGRAM())
}
type PendingOfferCluster =
    SessionCluster<'static, PendingTransport, DefaultLabelUniverse, CounterClock, 4>;
type HintPendingOfferCluster =
    SessionCluster<'static, HintPendingTransport, DefaultLabelUniverse, CounterClock, 4>;
type PendingControllerEndpoint = CursorEndpoint<
    'static,
    0,
    PendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
type PendingControllerBindingEndpoint = CursorEndpoint<
    'static,
    0,
    PendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    TestBinding,
>;
type HintPendingControllerEndpoint = CursorEndpoint<
    'static,
    0,
    HintPendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
type HintPendingWorkerEndpoint = CursorEndpoint<
    'static,
    1,
    HintPendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
const OFFER_CLUSTER_SLOT_BYTES: usize = max_usize(&[
    size_of::<OfferHintCluster>(),
    size_of::<PendingOfferCluster>(),
    size_of::<HintPendingOfferCluster>(),
    size_of::<
        SessionCluster<'static, DeferredIngressTransport, DefaultLabelUniverse, CounterClock, 4>,
    >(),
]);
const OFFER_VALUE_SLOT_BYTES: usize = max_usize(&[
    offer_endpoint_slot_bytes::<0, HintOnlyTransport, NoBinding>(1),
    offer_endpoint_slot_bytes::<1, HintOnlyTransport, NoBinding>(4),
    offer_endpoint_slot_bytes::<0, HintOnlyTransport, TestBinding>(4),
    offer_endpoint_slot_bytes::<1, HintOnlyTransport, TestBinding>(4),
    offer_endpoint_slot_bytes::<1, HintOnlyTransport, LaneAwareTestBinding>(3),
    offer_endpoint_slot_bytes::<0, PendingTransport, NoBinding>(1),
    offer_endpoint_slot_bytes::<1, PendingTransport, NoBinding>(1),
    offer_endpoint_slot_bytes::<0, PendingTransport, TestBinding>(1),
    offer_endpoint_slot_bytes::<1, PendingTransport, TestBinding>(1),
    offer_endpoint_slot_bytes::<0, HintPendingTransport, NoBinding>(1),
    offer_endpoint_slot_bytes::<1, HintPendingTransport, NoBinding>(1),
    size_of::<PendingTransportState>(),
    size_of::<DeferredIngressState>(),
    offer_endpoint_slot_bytes::<0, DeferredIngressTransport, NoBinding>(1),
    offer_endpoint_slot_bytes::<1, DeferredIngressTransport, DeferredIngressBinding>(1),
]);
type PendingWorkerEndpoint = CursorEndpoint<
    'static,
    1,
    PendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    NoBinding,
>;
type PendingWorkerBindingEndpoint = CursorEndpoint<
    'static,
    1,
    PendingTransport,
    DefaultLabelUniverse,
    CounterClock,
    crate::control::cap::mint::EpochTbl,
    4,
    crate::control::cap::mint::MintConfig,
    TestBinding,
>;

struct OfferTestFixtureGuard<const N: usize> {
    tap: *mut [TapEvent; RING_EVENTS],
    slab: *mut [u8; OFFER_FIXTURE_SLAB_CAPACITY],
    clock: *const CounterClock,
}

thread_local! {
    static OFFER_TEST_TAP: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    static OFFER_TEST_SLAB: UnsafeCell<[u8; OFFER_FIXTURE_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; OFFER_FIXTURE_SLAB_CAPACITY]) };
    static OFFER_TEST_CLOCK: CounterClock = const { CounterClock::new() };
}

fn acquire_offer_fixture<const N: usize>() -> OfferTestFixtureGuard<N> {
    assert!(
        N <= OFFER_FIXTURE_SLAB_CAPACITY,
        "offer fixture slab too small"
    );
    OFFER_TEST_TAP.with(|tap| {
        OFFER_TEST_SLAB.with(|slab| unsafe {
            OFFER_TEST_CLOCK.with(|clock| {
                let tap_ptr = tap.get();
                (*tap_ptr).fill(TapEvent::zero());
                let slab_ptr = slab.get();
                (*slab_ptr).fill(0);
                OfferTestFixtureGuard {
                    tap: tap_ptr,
                    slab: slab_ptr,
                    clock: clock as *const CounterClock,
                }
            })
        })
    })
}

impl<const N: usize> OfferTestFixtureGuard<N> {
    fn config(&mut self) -> Config<'static, DefaultLabelUniverse, CounterClock> {
        let tap = unsafe { &mut *self.tap };
        let slab = unsafe { &mut *self.slab };
        Config::new(tap, slab)
    }

    fn clock(&self) -> &'static CounterClock {
        unsafe { &*self.clock }
    }
}

#[repr(C, align(16))]
struct OfferClusterStorage {
    bytes: [u8; OFFER_CLUSTER_SLOT_BYTES],
}

#[repr(C, align(16))]
struct OfferValueStorage {
    bytes: [u8; OFFER_VALUE_SLOT_BYTES],
}

trait OfferClusterInit {
    unsafe fn init_empty(dst: *mut Self, clock: &'static CounterClock);
}

impl<T, U, const MAX_RV: usize> OfferClusterInit
    for SessionCluster<'static, T, U, CounterClock, MAX_RV>
where
    T: Transport + 'static,
    U: LabelUniverse + 'static,
{
    unsafe fn init_empty(dst: *mut Self, clock: &'static CounterClock) {
        unsafe { SessionCluster::init_empty(dst, clock) };
    }
}

thread_local! {
    static OFFER_CLUSTER_STORAGE: UnsafeCell<MaybeUninit<OfferClusterStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_CONTROLLER_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_CONTROLLER_OCCUPIED: Cell<bool> = const { Cell::new(false) };
    static OFFER_WORKER_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_WORKER_OCCUPIED: Cell<bool> = const { Cell::new(false) };
    static OFFER_CLIENT_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_CLIENT_OCCUPIED: Cell<bool> = const { Cell::new(false) };
    static OFFER_SERVER_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_SERVER_OCCUPIED: Cell<bool> = const { Cell::new(false) };
    static OFFER_PENDING_STATE_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_PENDING_STATE_OCCUPIED: Cell<bool> = const { Cell::new(false) };
    static OFFER_DEFERRED_STATE_STORAGE: UnsafeCell<MaybeUninit<OfferValueStorage>> =
        const { UnsafeCell::new(MaybeUninit::uninit()) };
    static OFFER_DEFERRED_STATE_OCCUPIED: Cell<bool> = const { Cell::new(false) };
}

fn with_offer_cluster_slot<T, R>(clock: &'static CounterClock, f: impl FnOnce(&'static T) -> R) -> R
where
    T: OfferClusterInit + 'static,
{
    assert!(
        size_of::<T>() <= OFFER_CLUSTER_SLOT_BYTES,
        "offer cluster slot too small"
    );
    assert!(
        align_of::<T>() <= align_of::<OfferClusterStorage>(),
        "offer cluster slot alignment too small"
    );
    OFFER_CLUSTER_STORAGE.with(|storage| unsafe {
        let ptr = (*storage.get()).as_mut_ptr().cast::<T>();
        T::init_empty(ptr, clock);
        let result = f(&*ptr);
        core::ptr::drop_in_place(ptr);
        result
    })
}

struct OfferValueSlotGuard<'a, T> {
    value: *mut T,
    occupied: *const Cell<bool>,
    _marker: PhantomData<&'a mut T>,
}

fn with_offer_value_storage<'a, T: 'a, R>(
    storage: &UnsafeCell<MaybeUninit<OfferValueStorage>>,
    occupied: &Cell<bool>,
    f: impl FnOnce(&mut OfferValueSlotGuard<'a, T>) -> R,
) -> R {
    assert!(
        size_of::<T>() <= OFFER_VALUE_SLOT_BYTES,
        "offer value slot too small"
    );
    assert!(
        align_of::<T>() <= align_of::<OfferValueStorage>(),
        "offer value slot alignment too small"
    );
    occupied.set(false);
    let mut slot = OfferValueSlotGuard {
        value: unsafe { (*storage.get()).as_mut_ptr().cast::<T>() },
        occupied: occupied as *const Cell<bool>,
        _marker: PhantomData,
    };
    f(&mut slot)
}

fn with_offer_value_slot_storage<R>(
    slot_name: &str,
    f: impl FnOnce(&UnsafeCell<MaybeUninit<OfferValueStorage>>, &Cell<bool>) -> R,
) -> R {
    match slot_name {
        "controller_slot" => OFFER_CONTROLLER_STORAGE
            .with(|storage| OFFER_CONTROLLER_OCCUPIED.with(|occupied| f(storage, occupied))),
        "worker_slot" => OFFER_WORKER_STORAGE
            .with(|storage| OFFER_WORKER_OCCUPIED.with(|occupied| f(storage, occupied))),
        "client_slot" => OFFER_CLIENT_STORAGE
            .with(|storage| OFFER_CLIENT_OCCUPIED.with(|occupied| f(storage, occupied))),
        "server_slot" => OFFER_SERVER_STORAGE
            .with(|storage| OFFER_SERVER_OCCUPIED.with(|occupied| f(storage, occupied))),
        "pending_state_slot" => OFFER_PENDING_STATE_STORAGE
            .with(|storage| OFFER_PENDING_STATE_OCCUPIED.with(|occupied| f(storage, occupied))),
        "deferred_state_slot" => OFFER_DEFERRED_STATE_STORAGE
            .with(|storage| OFFER_DEFERRED_STATE_OCCUPIED.with(|occupied| f(storage, occupied))),
        _ => panic!("unknown offer value slot"),
    }
}

impl<T> OfferValueSlotGuard<'_, T> {
    fn occupied(&self) -> &Cell<bool> {
        unsafe { &*self.occupied }
    }

    fn ptr(&self) -> *mut T {
        self.occupied().set(true);
        self.value
    }

    fn store(&self, value: T) {
        unsafe {
            self.value.write(value);
        }
        self.occupied().set(true);
    }

    fn borrow_mut(&mut self) -> &mut T {
        assert!(self.occupied().get(), "offer value slot is empty");
        unsafe { &mut *self.value }
    }
}

impl<T> Drop for OfferValueSlotGuard<'_, T> {
    fn drop(&mut self) {
        if self.occupied().replace(false) {
            unsafe {
                core::ptr::drop_in_place(self.value);
            }
        }
    }
}

macro_rules! offer_fixture {
    ($size:expr, $clock:ident, $config:ident) => {
        let mut __offer_fixture = acquire_offer_fixture::<$size>();
        let $clock = __offer_fixture.clock();
        let $config = __offer_fixture.config();
    };
}

macro_rules! with_offer_cluster {
    ($clock:expr, $cluster_ty:ty, $cluster_ref:ident, $body:block) => {{ with_offer_cluster_slot::<$cluster_ty, _>($clock, |$cluster_ref| $body) }};
}

macro_rules! with_offer_value_slot {
    ($value_ty:ty, $slot:ident, $body:block) => {{
        with_offer_value_slot_storage(stringify!($slot), |storage, occupied| {
            with_offer_value_storage::<$value_ty, _>(storage, occupied, |$slot| $body)
        })
    }};
}

fn poll_ready_ok<F, T, E>(cx: &mut Context<'_>, mut fut: core::pin::Pin<&mut F>, context: &str) -> T
where
    F: Future<Output = Result<T, E>>,
    E: core::fmt::Debug,
{
    let mut spins = 0usize;
    loop {
        match fut.as_mut().poll(cx) {
            Poll::Ready(Ok(value)) => return value,
            Poll::Ready(Err(err)) => panic!("{context} failed: {err:?}"),
            Poll::Pending => {
                spins += 1;
                if spins > 8 {
                    panic!("{context} unexpectedly pending");
                }
                cx.waker().wake_by_ref();
            }
        }
    }
}

fn run_offer_regression_test<F>(name: &'static str, test: F)
where
    F: FnOnce() + Send + 'static,
{
    let _ = name;
    test();
}

const TEST_BINDING_QUEUE_CAPACITY: usize = 8;
const TEST_BINDING_PAYLOAD_CAPACITY: usize = 64;

struct FixedQueue<T, const N: usize> {
    items: [Option<T>; N],
    head: usize,
    len: usize,
}

impl<T, const N: usize> FixedQueue<T, N> {
    fn new() -> Self {
        Self {
            items: core::array::from_fn(|_| None),
            head: 0,
            len: 0,
        }
    }

    fn push_back(&mut self, item: T) {
        assert!(self.len < N, "fixed queue capacity exceeded");
        let idx = (self.head + self.len) % N;
        self.items[idx] = Some(item);
        self.len += 1;
    }

    fn pop_front(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let idx = self.head;
        self.head = (self.head + 1) % N;
        self.len -= 1;
        self.items[idx].take()
    }
}

struct FixedPayload {
    len: usize,
    bytes: [u8; TEST_BINDING_PAYLOAD_CAPACITY],
}

impl FixedPayload {
    fn from_bytes(payload: &[u8]) -> Self {
        assert!(
            payload.len() <= TEST_BINDING_PAYLOAD_CAPACITY,
            "test binding payload exceeds fixed capacity"
        );
        let mut bytes = [0u8; TEST_BINDING_PAYLOAD_CAPACITY];
        bytes[..payload.len()].copy_from_slice(payload);
        Self {
            len: payload.len(),
            bytes,
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

struct TestBinding {
    incoming: FixedQueue<IngressEvidence, TEST_BINDING_QUEUE_CAPACITY>,
    recv_payloads: FixedQueue<FixedPayload, TEST_BINDING_QUEUE_CAPACITY>,
    polls: Cell<usize>,
    last_recv_channel: Cell<Option<Channel>>,
}

impl TestBinding {
    fn with_incoming(incoming: &[IngressEvidence]) -> Self {
        let mut binding = Self::default();
        for evidence in incoming.iter().copied() {
            binding.incoming.push_back(evidence);
        }
        binding
    }

    fn with_incoming_and_payloads(incoming: &[IngressEvidence], recv_payloads: &[&[u8]]) -> Self {
        let mut binding = Self::with_incoming(incoming);
        for payload in recv_payloads {
            binding
                .recv_payloads
                .push_back(FixedPayload::from_bytes(payload));
        }
        binding
    }

    fn poll_count(&self) -> usize {
        self.polls.get()
    }

    fn last_recv_channel(&self) -> Option<Channel> {
        self.last_recv_channel.get()
    }
}

impl Default for TestBinding {
    fn default() -> Self {
        Self {
            incoming: FixedQueue::new(),
            recv_payloads: FixedQueue::new(),
            polls: Cell::new(0),
            last_recv_channel: Cell::new(None),
        }
    }
}

struct LaneAwareTestBinding {
    incoming: std::vec::Vec<FixedQueue<IngressEvidence, TEST_BINDING_QUEUE_CAPACITY>>,
    polls: std::vec::Vec<usize>,
}

impl LaneAwareTestBinding {
    fn with_lane_incoming(incoming: &[(u8, IngressEvidence)]) -> Self {
        let lane_capacity = incoming
            .iter()
            .map(|(lane, _)| usize::from(*lane).saturating_add(1))
            .max()
            .unwrap_or(1);
        let mut binding = Self {
            incoming: std::iter::repeat_with(FixedQueue::new)
                .take(lane_capacity)
                .collect(),
            polls: std::vec![0; lane_capacity],
        };
        for (lane, evidence) in incoming.iter().copied() {
            let lane_idx = lane as usize;
            if lane_idx < binding.incoming.len() {
                binding.incoming[lane_idx].push_back(evidence);
            }
        }
        binding
    }

    fn poll_count_for_lane(&self, lane_idx: usize) -> usize {
        self.polls.get(lane_idx).copied().unwrap_or(0)
    }
}

impl BindingSlot for LaneAwareTestBinding {
    fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IngressEvidence> {
        let lane_idx = logical_lane as usize;
        if lane_idx >= self.incoming.len() {
            return None;
        }
        self.polls[lane_idx] = self.polls[lane_idx].saturating_add(1);
        self.incoming[lane_idx].pop_front()
    }

    fn on_recv<'a>(
        &'a mut self,
        _channel: Channel,
        _buf: &'a mut [u8],
    ) -> Result<Payload<'a>, TransportOpsError> {
        Ok(Payload::new(&[]))
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
        None
    }
}

impl BindingSlot for TestBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IngressEvidence> {
        self.polls.set(self.polls.get().saturating_add(1));
        self.incoming.pop_front()
    }

    fn on_recv<'a>(
        &'a mut self,
        channel: Channel,
        buf: &'a mut [u8],
    ) -> Result<Payload<'a>, TransportOpsError> {
        self.last_recv_channel.set(Some(channel));
        let Some(payload) = self.recv_payloads.pop_front() else {
            return Ok(Payload::new(&[]));
        };
        let payload = payload.as_slice();
        let len = core::cmp::min(buf.len(), payload.len());
        buf[..len].copy_from_slice(&payload[..len]);
        Ok(Payload::new(&buf[..len]))
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
        None
    }
}

const HINT_NONE: u8 = u8::MAX;

#[derive(Clone, Copy)]
struct HintOnlyTransport {
    worker_hint: u8,
}

impl HintOnlyTransport {
    const fn new(worker_hint: u8) -> Self {
        Self { worker_hint }
    }
}

struct HintOnlyRx {
    hint: Cell<u8>,
}

#[derive(Clone, Copy)]
struct HintPendingTransport {
    state: &'static PendingTransportState,
    worker_hint: u8,
}

impl HintPendingTransport {
    const fn new(state: &'static PendingTransportState, worker_hint: u8) -> Self {
        Self { state, worker_hint }
    }

    fn poll_count(&self) -> usize {
        self.state.polls.get()
    }

    fn assert_no_hint_drain_while_recv_parked(&self) {
        assert_eq!(
            self.state.hint_drains_while_recv_parked.get(),
            0,
            "offer must not drain route hints from a lane whose recv future is parked"
        );
    }
}

struct HintPendingRx {
    hint: Cell<u8>,
}

impl Transport for HintOnlyTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = HintOnlyRx
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let hint = if local_role == 1 {
            self.worker_hint
        } else {
            HINT_NONE
        };
        (
            (),
            HintOnlyRx {
                hint: Cell::new(hint),
            },
        )
    }

    fn poll_send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        Poll::Ready(Ok(Payload::new(&[0u8; 1])))
    }

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        let hint = rx.hint.get();
        if hint == HINT_NONE {
            None
        } else {
            rx.hint.set(HINT_NONE);
            Some(FrameLabel::new(hint))
        }
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }

    fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
}

impl Transport for HintPendingTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = HintPendingRx
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        let hint = if local_role == 1 {
            self.worker_hint
        } else {
            HINT_NONE
        };
        (
            (),
            HintPendingRx {
                hint: Cell::new(hint),
            },
        )
    }

    fn poll_send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.state.polls.set(self.state.polls.get().wrapping_add(1));
        if self.state.ready.get() {
            self.state.recv_parked.set(false);
            Poll::Ready(Ok(Payload::new(&[])))
        } else {
            self.state.recv_parked.set(true);
            unsafe {
                *self.state.waker.get() = Some(cx.waker().clone());
            }
            Poll::Pending
        }
    }

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<crate::transport::FrameLabel> {
        if self.state.recv_parked.get() {
            self.state.hint_drains_while_recv_parked.set(
                self.state
                    .hint_drains_while_recv_parked
                    .get()
                    .wrapping_add(1),
            );
            assert!(
                !self.state.panic_on_hint_drain_while_recv_parked.get(),
                "transport hint drain must not touch rx while recv future is parked"
            );
        }
        let hint = rx.hint.get();
        if hint == HINT_NONE {
            None
        } else {
            Some(FrameLabel::new(hint))
        }
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }

    fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
}

#[derive(Clone, Copy)]
struct PendingTransport {
    state: &'static PendingTransportState,
}

impl PendingTransport {
    fn new(state: &'static PendingTransportState) -> Self {
        Self { state }
    }

    fn poll_count(&self) -> usize {
        self.state.polls.get()
    }

    fn requeue_count(&self) -> usize {
        self.state.requeues.get()
    }
}

#[derive(Default)]
struct PendingTransportState {
    polls: Cell<usize>,
    requeues: Cell<usize>,
    ready: Cell<bool>,
    recv_parked: Cell<bool>,
    hint_drains_while_recv_parked: Cell<usize>,
    panic_on_hint_drain_while_recv_parked: Cell<bool>,
    waker: UnsafeCell<Option<Waker>>,
}

struct DeferredIngressState {
    incoming: UnsafeCell<FixedQueue<IngressEvidence, TEST_BINDING_QUEUE_CAPACITY>>,
    recv_payloads: UnsafeCell<FixedQueue<FixedPayload, TEST_BINDING_QUEUE_CAPACITY>>,
    available: Cell<usize>,
}

impl DeferredIngressState {
    fn new() -> Self {
        Self {
            incoming: UnsafeCell::new(FixedQueue::new()),
            recv_payloads: UnsafeCell::new(FixedQueue::new()),
            available: Cell::new(0),
        }
    }

    fn push_incoming(&self, evidence: IngressEvidence) {
        unsafe {
            (&mut *self.incoming.get()).push_back(evidence);
        }
    }

    fn push_recv_payload(&self, payload: FixedPayload) {
        unsafe {
            (&mut *self.recv_payloads.get()).push_back(payload);
        }
    }

    fn pop_incoming(&self) -> Option<IngressEvidence> {
        unsafe { (&mut *self.incoming.get()).pop_front() }
    }

    fn pop_recv_payload(&self) -> Option<FixedPayload> {
        unsafe { (&mut *self.recv_payloads.get()).pop_front() }
    }
}

struct DeferredIngressBinding {
    state: &'static DeferredIngressState,
    polls: Cell<usize>,
}

impl DeferredIngressBinding {
    fn with_incoming_and_payloads(
        state: &'static DeferredIngressState,
        incoming: &[IngressEvidence],
        recv_payloads: &[&[u8]],
    ) -> Self {
        for evidence in incoming.iter().copied() {
            state.push_incoming(evidence);
        }
        for payload in recv_payloads {
            state.push_recv_payload(FixedPayload::from_bytes(payload));
        }
        Self {
            state,
            polls: Cell::new(0),
        }
    }
}

impl BindingSlot for DeferredIngressBinding {
    fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IngressEvidence> {
        self.polls.set(self.polls.get().saturating_add(1));
        if self.state.available.get() == 0 {
            return None;
        }
        let evidence = self.state.pop_incoming()?;
        self.state
            .available
            .set(self.state.available.get().saturating_sub(1));
        Some(evidence)
    }

    fn on_recv<'a>(
        &'a mut self,
        _channel: Channel,
        buf: &'a mut [u8],
    ) -> Result<Payload<'a>, TransportOpsError> {
        let Some(payload) = self.state.pop_recv_payload() else {
            return Ok(Payload::new(&[]));
        };
        let payload = payload.as_slice();
        let len = core::cmp::min(buf.len(), payload.len());
        buf[..len].copy_from_slice(&payload[..len]);
        Ok(Payload::new(&buf[..len]))
    }

    fn policy_signals_provider(
        &self,
    ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
        None
    }
}

struct DeferredIngressTransport {
    state: &'static DeferredIngressState,
}

impl DeferredIngressTransport {
    fn new(state: &'static DeferredIngressState) -> Self {
        Self { state }
    }
}

struct DeferredIngressRx;

struct PendingRx;

impl Transport for PendingTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = PendingRx
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), PendingRx)
    }

    fn poll_send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.state.polls.set(self.state.polls.get().wrapping_add(1));
        if self.state.ready.get() {
            self.state.recv_parked.set(false);
            Poll::Ready(Ok(Payload::new(&[0x5a])))
        } else {
            self.state.recv_parked.set(true);
            unsafe {
                *self.state.waker.get() = Some(cx.waker().clone());
            }
            Poll::Pending
        }
    }

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {
        self.state
            .requeues
            .set(self.state.requeues.get().wrapping_add(1));
    }

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(
        &'a self,
        _rx: &'a Self::Rx<'a>,
    ) -> Option<crate::transport::FrameLabel> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }

    fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
}

impl Transport for DeferredIngressTransport {
    type Error = TransportError;
    type Tx<'a>
        = ()
    where
        Self: 'a;
    type Rx<'a>
        = DeferredIngressRx
    where
        Self: 'a;
    type Metrics = ();

    fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
        ((), DeferredIngressRx)
    }

    fn poll_send<'a, 'f>(
        &'a self,
        _tx: &'a mut Self::Tx<'a>,
        _outgoing: crate::transport::Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>
    where
        'a: 'f,
    {
        Poll::Ready(Ok(()))
    }

    fn poll_recv<'a>(
        &'a self,
        _rx: &'a mut Self::Rx<'a>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Payload<'a>, Self::Error>> {
        self.state
            .available
            .set(self.state.available.get().wrapping_add(1));
        Poll::Ready(Ok(Payload::new(&[])))
    }

    fn cancel_send<'a>(&'a self, _tx: &'a mut Self::Tx<'a>) {}

    fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

    fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

    fn recv_frame_hint<'a>(
        &'a self,
        _rx: &'a Self::Rx<'a>,
    ) -> Option<crate::transport::FrameLabel> {
        None
    }

    fn metrics(&self) -> Self::Metrics {
        ()
    }

    fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
}

const HINT_ROUTE_POLICY_ID: u16 = 601;
type HintLeftHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
        >,
        StepNil,
    >,
    HINT_ROUTE_POLICY_ID,
>;
type HintRightHead = PolicySteps<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>,
        >,
        StepNil,
    >,
    HINT_ROUTE_POLICY_ID,
>;
#[allow(non_snake_case)]
fn HINT_LEFT_ARM()
-> g::Program<SeqSteps<HintLeftHead, StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>>> {
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<100, u8>, 0>(),
    )
}

#[allow(non_snake_case)]
fn HINT_RIGHT_ARM()
-> g::Program<SeqSteps<HintRightHead, StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>>>
{
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<101, u8>, 0>(),
    )
}
type HintRouteSteps = RouteSteps<
    SeqSteps<HintLeftHead, StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>>,
    SeqSteps<HintRightHead, StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>>,
>;
#[allow(non_snake_case)]
fn HINT_ROUTE_PROGRAM() -> g::Program<HintRouteSteps> {
    g::route(HINT_LEFT_ARM(), HINT_RIGHT_ARM())
}

#[allow(non_snake_case)]
fn HINT_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&HINT_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
fn HINT_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&HINT_ROUTE_PROGRAM())
}
type HintSplitLeftSteps = SeqSteps<HintLeftHead, SendOnly<0, Role<0>, Role<1>, Msg<100, u8>>>;
type HintSplitRightSteps = SeqSteps<HintRightHead, SendOnly<2, Role<0>, Role<1>, Msg<101, u8>>>;
type HintSplitRouteSteps = RouteSteps<HintSplitLeftSteps, HintSplitRightSteps>;
#[allow(non_snake_case)]
fn HINT_SPLIT_LEFT_ARM() -> g::Program<HintSplitLeftSteps> {
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<100, u8>, 0>(),
    )
}

#[allow(non_snake_case)]
fn HINT_SPLIT_RIGHT_ARM() -> g::Program<HintSplitRightSteps> {
    g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<101, u8>, 2>(),
    )
}

#[allow(non_snake_case)]
fn HINT_SPLIT_ROUTE_PROGRAM() -> g::Program<HintSplitRouteSteps> {
    g::route(HINT_SPLIT_LEFT_ARM(), HINT_SPLIT_RIGHT_ARM())
}

#[allow(non_snake_case)]
fn HINT_SPLIT_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&HINT_SPLIT_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
fn HINT_SPLIT_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&HINT_SPLIT_ROUTE_PROGRAM())
}
const HINT_LEFT_DATA_LABEL: u8 = 100;
const HINT_RIGHT_DATA_LABEL: u8 = 101;
const HINT_LEFT_DATA_FRAME: u8 = 0;
const HINT_RIGHT_DATA_FRAME: u8 = 1;
type MultiSendRouteLeftMsg =
    Msg<{ TEST_ROUTE_DECISION_LOGICAL }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>;
type MultiSendRouteRightMsg =
    Msg<ROUTE_HINT_RIGHT_LABEL, GenericCapToken<RouteHintRightKind>, RouteHintRightKind>;
type MultiSendLeftPayloadMsg = Msg<0x59, u8>;
type MultiSendRightFirstMsg = Msg<0x5a, u8>;
type MultiSendRightSecondMsg = Msg<0x5b, u8>;
type MultiSendRightPayloadSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<1>, MultiSendRightFirstMsg>,
    SendOnly<0, Role<0>, Role<1>, MultiSendRightSecondMsg>,
>;
type MultiSendLeftSteps = SeqSteps<
    SendOnly<0, Role<0>, Role<0>, MultiSendRouteLeftMsg>,
    SendOnly<0, Role<0>, Role<1>, MultiSendLeftPayloadMsg>,
>;
type MultiSendRightSteps =
    SeqSteps<SendOnly<0, Role<0>, Role<0>, MultiSendRouteRightMsg>, MultiSendRightPayloadSteps>;
type MultiSendRouteSteps = BranchSteps<MultiSendLeftSteps, MultiSendRightSteps>;
#[allow(non_snake_case)]
fn MULTI_SEND_ROUTE_PROGRAM() -> g::Program<MultiSendRouteSteps> {
    g::route(
        g::seq(
            g::send::<Role<0>, Role<0>, MultiSendRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, MultiSendLeftPayloadMsg, 0>(),
        ),
        g::seq(
            g::send::<Role<0>, Role<0>, MultiSendRouteRightMsg, 0>(),
            g::seq(
                g::send::<Role<0>, Role<1>, MultiSendRightFirstMsg, 0>(),
                g::send::<Role<0>, Role<1>, MultiSendRightSecondMsg, 0>(),
            ),
        ),
    )
}

#[allow(non_snake_case)]
fn MULTI_SEND_ROUTE_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&MULTI_SEND_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
fn MULTI_SEND_ROUTE_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&MULTI_SEND_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
fn ENTRY_ARM0_PROGRAM() -> g::Program<
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
        >,
    >,
> {
    g::seq(
        g::send::<Role<0>, Role<0>, Msg<102, u8>, 0>(),
        g::seq(
            g::send::<Role<0>, Role<1>, Msg<103, u8>, 0>(),
            g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>(),
        ),
    )
}

#[allow(non_snake_case)]
fn ENTRY_ARM1_PROGRAM() -> g::Program<
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<86, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<87, u8>>, StepNil>,
        >,
    >,
> {
    g::seq(
        g::send::<Role<0>, Role<0>, Msg<105, u8>, 0>(),
        g::seq(
            g::send::<Role<0>, Role<1>, Msg<86, u8>, 0>(),
            g::send::<Role<1>, Role<0>, Msg<87, u8>, 0>(),
        ),
    )
}
type EntryRouteSteps = RouteSteps<
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
        >,
    >,
    SeqSteps<
        StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<86, u8>>, StepNil>,
            StepCons<SendStep<Role<1>, Role<0>, Msg<87, u8>>, StepNil>,
        >,
    >,
>;
#[allow(non_snake_case)]
fn ENTRY_ROUTE_PROGRAM() -> g::Program<EntryRouteSteps> {
    g::route(ENTRY_ARM0_PROGRAM(), ENTRY_ARM1_PROGRAM())
}

#[allow(non_snake_case)]
fn ENTRY_CONTROLLER_PROGRAM() -> RoleProgram<0> {
    project(&ENTRY_ROUTE_PROGRAM())
}

#[allow(non_snake_case)]
fn ENTRY_WORKER_PROGRAM() -> RoleProgram<1> {
    project(&ENTRY_ROUTE_PROGRAM())
}

type NestedRouteSteps = RouteSteps<HintRouteSteps, EntryRouteSteps>;
#[allow(non_snake_case)]
fn NESTED_ROUTE_PROGRAM() -> g::Program<NestedRouteSteps> {
    g::route(HINT_ROUTE_PROGRAM(), ENTRY_ROUTE_PROGRAM())
}
const ENTRY_ARM0_SIGNAL_LABEL: u8 = 103;
const ENTRY_ARM0_SIGNAL_FRAME: u8 = 0;
const ENTRY_ARM1_SIGNAL_FRAME: u8 = 1;

#[test]
fn binding_inbox_take_is_one_shot() {
    let evidence = IngressEvidence {
        frame_label: FrameLabel::new(7),
        instance: 3,
        has_fin: false,
        channel: Channel::new(1),
    };
    let mut binding = TestBinding::with_incoming(&[evidence]);
    with_test_binding_inbox::<1, _>(|inbox| {
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(evidence));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), None);

        inbox.put_back(0, evidence);
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(evidence));
    });
}

#[test]
fn binding_inbox_take_matching_skips_head_mismatch() {
    let head = IngressEvidence {
        frame_label: FrameLabel::new(7),
        instance: 3,
        has_fin: false,
        channel: Channel::new(1),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(9),
        instance: 4,
        has_fin: false,
        channel: Channel::new(2),
    };
    let mut binding = TestBinding::with_incoming(&[head, expected]);
    with_test_binding_inbox::<1, _>(|inbox| {
        let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.frame_label.raw());
        assert_eq!(picked, Some(expected));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(head));
    });
}

#[test]
fn binding_inbox_take_matching_scans_buffered_entries() {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(3),
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(4),
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(5),
        instance: 3,
        has_fin: false,
        channel: Channel::new(13),
    };
    let mut binding = TestBinding::default();
    with_test_binding_inbox::<1, _>(|inbox| {
        assert!(inbox.push_back(0, first));
        assert!(inbox.push_back(0, second));
        assert!(inbox.push_back(0, expected));

        let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.frame_label.raw());
        assert_eq!(picked, Some(expected));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(second));
    });
}

#[test]
fn binding_inbox_nonempty_mask_tracks_buffered_lanes() {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(3),
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(4),
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let mut binding = TestBinding::default();
    with_test_binding_inbox::<3, _>(|inbox| {
        assert_nonempty_lanes_eq(inbox, 3, &[]);

        assert!(inbox.push_back(0, first));
        assert_nonempty_lanes_eq(inbox, 3, &[0]);

        assert!(inbox.push_back(2, second));
        assert_nonempty_lanes_eq(inbox, 3, &[0, 2]);

        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_nonempty_lanes_eq(inbox, 3, &[2]);

        assert_eq!(
            inbox.take_matching_or_poll(&mut binding, 2, second.frame_label.raw()),
            Some(second)
        );
        assert_nonempty_lanes_eq(inbox, 3, &[]);
    });
}

#[test]
fn binding_inbox_frame_label_masks_track_buffered_frame_labels_exactly() {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(3),
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(4),
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let third = IngressEvidence {
        frame_label: FrameLabel::new(207),
        instance: 3,
        has_fin: false,
        channel: Channel::new(13),
    };
    let mut binding = TestBinding::default();
    with_test_binding_inbox::<3, _>(|inbox| {
        assert!(inbox.push_back(0, first));
        assert!(inbox.push_back(0, second));
        assert!(inbox.push_back(2, third));
        assert_eq!(
            inbox.buffered_frame_label_mask_for_lane(0),
            FrameLabelMask::from_frame_label(first.frame_label.raw())
                | FrameLabelMask::from_frame_label(second.frame_label.raw())
        );
        assert_eq!(
            inbox.buffered_frame_label_mask_for_lane(2),
            FrameLabelMask::from_frame_label(third.frame_label.raw())
        );
        assert_buffered_lanes_eq(
            inbox,
            FrameLabelMask::from_frame_label(first.frame_label.raw()),
            &[0],
        );
        assert_buffered_lanes_eq(
            inbox,
            FrameLabelMask::from_frame_label(second.frame_label.raw()),
            &[0],
        );
        assert_buffered_lanes_eq(
            inbox,
            FrameLabelMask::from_frame_label(third.frame_label.raw()),
            &[2],
        );

        assert_eq!(
            inbox.take_matching_or_poll(&mut binding, 0, second.frame_label.raw()),
            Some(second)
        );
        assert_eq!(
            inbox.buffered_frame_label_mask_for_lane(0),
            FrameLabelMask::from_frame_label(first.frame_label.raw())
        );
        assert_buffered_lanes_eq(
            inbox,
            FrameLabelMask::from_frame_label(second.frame_label.raw()),
            &[],
        );
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_eq!(
            inbox.buffered_frame_label_mask_for_lane(0),
            FrameLabelMask::EMPTY
        );
        assert_buffered_lanes_eq(
            inbox,
            FrameLabelMask::from_frame_label(first.frame_label.raw()),
            &[],
        );
    });
}

#[test]
fn binding_inbox_take_matching_mask_drops_buffered_loop_control_frames() {
    let loop_control = IngressEvidence {
        frame_label: FrameLabel::new(TEST_LOOP_CONTINUE_FRAME),
        instance: 1,
        has_fin: false,
        channel: Channel::new(11),
    };
    let deferred = IngressEvidence {
        frame_label: FrameLabel::new(33),
        instance: 2,
        has_fin: false,
        channel: Channel::new(12),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(55),
        instance: 3,
        has_fin: false,
        channel: Channel::new(13),
    };
    let mut binding = TestBinding::with_incoming(&[expected]);
    with_test_binding_inbox::<1, _>(|inbox| {
        assert!(inbox.push_back(0, loop_control));
        assert!(inbox.push_back(0, deferred));

        let picked = inbox.take_matching_mask_or_poll(
            &mut binding,
            0,
            FrameLabelMask::from_frame_label(expected.frame_label.raw()),
            FrameLabelMask::from_frame_label(TEST_LOOP_CONTINUE_FRAME)
                | FrameLabelMask::from_frame_label(TEST_LOOP_BREAK_FRAME),
            |frame_label| {
                matches!(
                    frame_label,
                    TEST_LOOP_CONTINUE_FRAME | TEST_LOOP_BREAK_FRAME
                )
            },
        );
        assert_eq!(picked, Some(expected));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(deferred));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), None);
    });
}

#[test]
fn binding_frame_mismatch_finds_later_matching_frame_label() {
    let first = IngressEvidence {
        frame_label: FrameLabel::new(11),
        instance: 1,
        has_fin: false,
        channel: Channel::new(21),
    };
    let second = IngressEvidence {
        frame_label: FrameLabel::new(12),
        instance: 2,
        has_fin: false,
        channel: Channel::new(22),
    };
    let expected = IngressEvidence {
        frame_label: FrameLabel::new(13),
        instance: 3,
        has_fin: false,
        channel: Channel::new(23),
    };
    let mut binding = TestBinding::with_incoming(&[first, second, expected]);
    with_test_binding_inbox::<1, _>(|inbox| {
        let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.frame_label.raw());
        assert_eq!(
            picked,
            Some(expected),
            "scan must continue past mismatched head entries"
        );
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(second));
    });
}

#[test]
fn stage_transport_payload_copies_bytes() {
    let mut scratch = [0u8; 8];
    let src = [1u8, 2, 3, 4];
    let len = stage_transport_payload(&mut scratch, &src).expect("stage payload");
    assert_eq!(len, src.len());
    assert_eq!(&scratch[..len], &src);
}

#[test]
fn stage_transport_payload_rejects_oversize() {
    let mut scratch = [0u8; 2];
    let src = [1u8, 2, 3];
    let err = stage_transport_payload(&mut scratch, &src).expect_err("oversize");
    assert!(matches!(err, RecvError::PhaseInvariant));
}

#[test]
fn offer_select_priority_is_deterministic() {
    assert_eq!(
        choose_offer_priority(true, 1, 1, 2),
        Some(OfferSelectPriority::CurrentOfferEntry)
    );
    assert_eq!(
        choose_offer_priority(false, 1, 2, 2),
        Some(OfferSelectPriority::DynamicControllerUnique)
    );
    assert_eq!(
        choose_offer_priority(false, 0, 1, 2),
        Some(OfferSelectPriority::ControllerUnique)
    );
    assert_eq!(
        choose_offer_priority(false, 0, 2, 1),
        Some(OfferSelectPriority::CandidateUnique)
    );
    assert_eq!(choose_offer_priority(false, 0, 2, 2), None);
}

#[test]
fn static_controller_current_is_not_preempted() {
    let selected = choose_offer_priority(true, 1, 1, 2);
    assert_eq!(selected, Some(OfferSelectPriority::CurrentOfferEntry));
}

#[test]
fn hint_filter_does_not_override_priority() {
    // Stage A applies filter; Stage B ordering is still fixed.
    let current_is_candidate_after_filter = true;
    let selected = choose_offer_priority(current_is_candidate_after_filter, 1, 1, 1);
    assert_eq!(selected, Some(OfferSelectPriority::CurrentOfferEntry));
}

#[test]
fn offer_priority_has_no_liveness_override() {
    // Stage B priority is fixed and independent from liveness signals.
    assert_eq!(
        choose_offer_priority(false, 1, 1, 1),
        Some(OfferSelectPriority::DynamicControllerUnique)
    );
    assert_eq!(
        choose_offer_priority(false, 0, 1, 1),
        Some(OfferSelectPriority::ControllerUnique)
    );
}

#[test]
fn current_scope_selection_meta_non_route_defaults_do_not_block_current() {
    let meta = CurrentScopeSelectionMeta::EMPTY;
    assert!(!meta.is_route_entry());
    assert!(meta.has_offer_lanes());
    assert!(!meta.is_controller());
}

#[test]
fn current_scope_selection_meta_route_entry_flags_roundtrip() {
    let meta = CurrentScopeSelectionMeta {
        flags: CurrentScopeSelectionMeta::FLAG_ROUTE_ENTRY
            | CurrentScopeSelectionMeta::FLAG_HAS_OFFER_LANES
            | CurrentScopeSelectionMeta::FLAG_CONTROLLER,
    };
    assert!(meta.is_route_entry());
    assert!(meta.has_offer_lanes());
    assert!(meta.is_controller());
}

#[test]
fn current_frontier_selection_state_loop_controller_without_evidence_is_exact() {
    let base = CurrentFrontierSelectionState {
        frontier: FrontierKind::Loop,
        parallel_root: ScopeId::none(),
        ready: true,
        has_progress_evidence: false,
        flags: CurrentFrontierSelectionState::FLAG_CONTROLLER,
    };
    assert!(base.loop_controller_without_evidence());
    assert!(
        !CurrentFrontierSelectionState {
            ready: false,
            ..base
        }
        .loop_controller_without_evidence()
    );
    assert!(
        !CurrentFrontierSelectionState {
            has_progress_evidence: true,
            ..base
        }
        .loop_controller_without_evidence()
    );
    assert!(!CurrentFrontierSelectionState { flags: 0, ..base }.loop_controller_without_evidence());
}

#[test]
fn current_frontier_selection_state_updates_only_current_candidate() {
    let mut state = CurrentFrontierSelectionState {
        frontier: FrontierKind::Parallel,
        parallel_root: ScopeId::generic(3),
        ready: false,
        has_progress_evidence: false,
        flags: 0,
    };
    state.observe_candidate(
        ScopeId::generic(11),
        7,
        FrontierCandidate {
            scope_id: ScopeId::generic(12),
            entry_idx: 9,
            parallel_root: ScopeId::generic(3),
            frontier: FrontierKind::Parallel,
            flags: FrontierCandidate::pack_flags(false, false, true, true),
        },
    );
    assert!(!state.ready);
    assert!(!state.has_progress_evidence);

    state.observe_candidate(
        ScopeId::generic(11),
        7,
        FrontierCandidate {
            scope_id: ScopeId::generic(11),
            entry_idx: 7,
            parallel_root: ScopeId::generic(3),
            frontier: FrontierKind::Parallel,
            flags: FrontierCandidate::pack_flags(false, false, true, true),
        },
    );
    assert!(state.ready);
    assert!(state.has_progress_evidence);
}

#[test]
fn scope_loop_meta_recvless_ready_requires_active_or_linger() {
    assert!(!ScopeLoopMeta::EMPTY.recvless_ready());
    assert!(
        ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_SCOPE_ACTIVE,
        }
        .recvless_ready()
    );
    assert!(
        ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_SCOPE_LINGER | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
        }
        .recvless_ready()
    );
    assert!(
        !ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_SCOPE_ACTIVE
                | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV
                | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
        }
        .recvless_ready()
    );
}

#[test]
fn scope_loop_meta_loop_label_scope_and_arm_recv_bits_are_exact() {
    let meta = ScopeLoopMeta {
        flags: ScopeLoopMeta::FLAG_CONTROL_SCOPE | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
    };
    assert!(meta.loop_label_scope());
    assert!(!meta.arm_has_recv(0));
    assert!(meta.arm_has_recv(1));

    let linger = ScopeLoopMeta {
        flags: ScopeLoopMeta::FLAG_SCOPE_LINGER | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV,
    };
    assert!(linger.loop_label_scope());
    assert!(linger.arm_has_recv(0));
    assert!(!linger.arm_has_recv(1));
    assert!(!ScopeLoopMeta::EMPTY.loop_label_scope());
}

#[test]
fn scope_frame_label_meta_current_recv_frame_label_and_arm_bits_are_exact() {
    let no_arm = ScopeFrameLabelMeta {
        recv_frame_label: 7,
        recv_arm: 1,
        flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL,
        ..ScopeFrameLabelMeta::EMPTY
    };
    assert!(no_arm.matches_current_recv_frame_label(7));
    assert!(no_arm.matches_frame_hint(7));
    assert_eq!(no_arm.current_recv_arm_for_frame_label(7), None);
    let with_arm = ScopeFrameLabelMeta {
        arm_frame_label_masks: [FrameLabelMask::EMPTY, FrameLabelMask::from_frame_label(7)],
        flags: no_arm.flags | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM,
        ..no_arm
    };
    assert_eq!(with_arm.current_recv_arm_for_frame_label(7), Some(1));
    assert_eq!(with_arm.arm_for_frame_label(7), Some(1));
    assert!(!with_arm.matches_current_recv_frame_label(8));

    let high_frame = ScopeFrameLabelMeta {
        arm_frame_label_masks: [FrameLabelMask::EMPTY, FrameLabelMask::from_frame_label(200)],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::EMPTY,
            FrameLabelMask::from_frame_label(200),
        ],
        ..ScopeFrameLabelMeta::EMPTY
    };
    assert!(high_frame.matches_frame_hint(200));
    assert_eq!(high_frame.arm_for_frame_label(200), Some(1));
    assert_eq!(high_frame.preferred_binding_frame_label(Some(1)), Some(200));
}

#[test]
fn scope_frame_label_meta_controller_frame_labels_map_to_binary_arms_exactly() {
    let meta = ScopeFrameLabelMeta {
        controller_frame_labels: [11, 13],
        arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(11),
            FrameLabelMask::from_frame_label(13),
        ],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(11),
            FrameLabelMask::from_frame_label(13),
        ],
        flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
            | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
        ..ScopeFrameLabelMeta::EMPTY
    };
    assert_eq!(meta.controller_arm_for_frame_label(11), Some(0));
    assert_eq!(meta.controller_arm_for_frame_label(13), Some(1));
    assert_eq!(meta.controller_arm_for_frame_label(17), None);
    assert_eq!(meta.arm_for_frame_label(11), Some(0));
    assert_eq!(meta.arm_for_frame_label(13), Some(1));
}

#[test]
fn scope_frame_label_meta_dispatch_frame_labels_do_not_count_as_ready_evidence() {
    let mut meta = ScopeFrameLabelMeta::EMPTY;
    meta.record_dispatch_arm_frame_label(1, 29);

    assert!(meta.matches_frame_hint(29));
    assert_eq!(meta.arm_for_frame_label(29), Some(1));
    assert_eq!(meta.evidence_arm_for_frame_label(29), None);
}

#[test]
fn scope_frame_label_meta_binding_evidence_can_be_stricter_than_hint_evidence() {
    let meta = ScopeFrameLabelMeta {
        recv_frame_label: 41,
        recv_arm: 0,
        arm_frame_label_masks: [FrameLabelMask::from_frame_label(41), FrameLabelMask::EMPTY],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(41),
            FrameLabelMask::EMPTY,
        ],
        flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED,
        ..ScopeFrameLabelMeta::EMPTY
    };

    assert!(meta.matches_frame_hint(41));
    assert_eq!(meta.arm_for_frame_label(41), Some(0));
    assert_eq!(meta.evidence_arm_for_frame_label(41), Some(0));
    assert_eq!(meta.binding_evidence_arm_for_frame_label(41), None);
}

#[test]
fn scope_frame_label_meta_preferred_binding_frame_label_is_exact_only_for_singletons() {
    let meta = ScopeFrameLabelMeta {
        recv_frame_label: 41,
        recv_arm: 0,
        controller_frame_labels: [43, 47],
        arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(41) | FrameLabelMask::from_frame_label(43),
            FrameLabelMask::from_frame_label(47),
        ],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(41) | FrameLabelMask::from_frame_label(43),
            FrameLabelMask::from_frame_label(47),
        ],
        flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED
            | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
            | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
        ..ScopeFrameLabelMeta::EMPTY
    };

    assert_eq!(meta.preferred_binding_frame_label(Some(0)), Some(43));
    assert_eq!(meta.preferred_binding_frame_label(Some(1)), Some(47));
    assert_eq!(meta.preferred_binding_frame_label(None), None);

    let singleton = ScopeFrameLabelMeta {
        controller_frame_labels: [53, 0],
        arm_frame_label_masks: [FrameLabelMask::from_frame_label(53), FrameLabelMask::EMPTY],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(53),
            FrameLabelMask::EMPTY,
        ],
        flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0,
        ..ScopeFrameLabelMeta::EMPTY
    };
    assert_eq!(singleton.preferred_binding_frame_label(None), Some(53));
}

#[test]
fn scope_frame_label_meta_preferred_binding_frame_label_mask_respects_authoritative_arm() {
    let meta = ScopeFrameLabelMeta {
        arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(11) | FrameLabelMask::from_frame_label(13),
            FrameLabelMask::from_frame_label(17),
        ],
        ..ScopeFrameLabelMeta::EMPTY
    };

    assert_eq!(
        meta.preferred_binding_frame_label_mask(Some(0)),
        FrameLabelMask::from_frame_label(11) | FrameLabelMask::from_frame_label(13)
    );
    assert_eq!(
        meta.preferred_binding_frame_label_mask(Some(1)),
        FrameLabelMask::from_frame_label(17)
    );
    assert_eq!(
        meta.preferred_binding_frame_label_mask(None),
        meta.frame_hint_mask()
    );
}

#[test]
fn scope_frame_label_meta_preferred_binding_frame_label_mask_keeps_current_recv_for_demux() {
    let meta = ScopeFrameLabelMeta {
        recv_frame_label: 41,
        recv_arm: 0,
        controller_frame_labels: [43, 47],
        arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(41) | FrameLabelMask::from_frame_label(43),
            FrameLabelMask::from_frame_label(47),
        ],
        evidence_arm_frame_label_masks: [
            FrameLabelMask::from_frame_label(41) | FrameLabelMask::from_frame_label(43),
            FrameLabelMask::from_frame_label(47),
        ],
        flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
            | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED
            | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
            | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
        ..ScopeFrameLabelMeta::EMPTY
    };

    assert_eq!(
        meta.preferred_binding_frame_label_mask(Some(0)),
        FrameLabelMask::from_frame_label(41) | FrameLabelMask::from_frame_label(43)
    );
}

#[test]
fn lane_offer_state_roundtrips_static_frontier_flags() {
    let state = LaneOfferState {
        scope: ScopeId::generic(5),
        entry: StateIndex::from_usize(11),
        parallel_root: ScopeId::generic(2),
        frontier: FrontierKind::Parallel,
        static_ready: true,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
    };
    assert!(state.is_controller());
    assert!(state.is_dynamic());
    assert!(state.static_ready());
    assert_eq!(state.frontier, FrontierKind::Parallel);
}

#[test]
fn refresh_lane_offer_state_caches_scope_frame_label_meta() {
    run_offer_regression_test(
        "refresh_lane_offer_state_caches_scope_frame_label_meta",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(997);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    worker.refresh_lane_offer_state(0);
                    let entry_idx =
                        state_index_to_usize(worker.route_state.lane_offer_state(0).entry);
                    let entry_state = worker
                        .offer_entry_state_snapshot(entry_idx)
                        .expect("offer entry state snapshot");
                    let cached = RouteFrontierMachine::offer_entry_frame_label_meta(
                        &worker, scope, entry_idx,
                    )
                    .expect("cached offer-entry label metadata");
                    let recv_meta = worker.cursor.try_recv_meta().expect("recv metadata");
                    assert_eq!(cached.scope_id(), scope);
                    assert_eq!(
                        cached.loop_meta().flags,
                        entry_state.frame_label_meta.loop_meta().flags
                    );
                    assert!(cached.matches_current_recv_frame_label(recv_meta.frame_label));
                    assert_eq!(
                        cached.current_recv_arm_for_frame_label(recv_meta.frame_label),
                        recv_meta.route_arm
                    );
                    assert_eq!(entry_state.scope_id, scope);
                    assert_eq!(
                        entry_state.frontier,
                        worker.route_state.lane_offer_state(0).frontier
                    );
                    assert_eq!(entry_state.frame_label_meta.scope_id(), scope);
                    assert!(entry_state.selection_meta.is_route_entry());
                    assert_eq!(
                        entry_state.selection_meta.is_controller(),
                        worker.route_state.lane_offer_state(0).is_controller()
                    );
                    assert_eq!(
                        entry_state.summary.frontier_mask,
                        worker.route_state.lane_offer_state(0).frontier.bit()
                    );
                    assert_eq!(
                        entry_state.summary.is_controller(),
                        worker.route_state.lane_offer_state(0).is_controller()
                    );
                    assert_eq!(
                        entry_state.summary.is_dynamic(),
                        worker.route_state.lane_offer_state(0).is_dynamic()
                    );
                    assert_eq!(
                        entry_state.summary.static_ready(),
                        worker.route_state.lane_offer_state(0).static_ready()
                    );
                    let observed = worker
                        .recompute_offer_entry_observed_state_non_consuming(entry_idx)
                        .expect("observed state");
                    assert_eq!(
                        worker.offer_entry_observed_state_cached(entry_idx),
                        Some(observed)
                    );
                    assert_lane_set_eq(
                        worker.offer_lane_set_for_scope(scope),
                        worker.cursor.logical_lane_count(),
                        &[0],
                    );
                    assert_eq!(entry_state.lane_idx, 0);
                    assert_eq!(
                        worker
                            .offer_entry_lane_state(scope, entry_idx)
                            .map(|info| info.entry),
                        Some(worker.route_state.lane_offer_state(0).entry)
                    );
                    let materialization = entry_state.materialization_meta;
                    assert_eq!(
                        materialization.arm_count,
                        worker.cursor.route_scope_arm_count(scope).unwrap_or(0)
                    );
                    let mut arm = 0u8;
                    while arm <= 1 {
                        let expected_controller_cross_role_recv = worker
                            .cursor
                            .controller_arm_entry_by_arm(scope, arm)
                            .and_then(|(entry, _)| {
                                worker.cursor.try_recv_meta_at(state_index_to_usize(entry))
                            })
                            .map(|recv_meta| recv_meta.peer != 1)
                            .unwrap_or(false);
                        assert_eq!(
                            materialization.controller_arm_entry(arm),
                            worker.cursor.controller_arm_entry_by_arm(scope, arm)
                        );
                        assert_eq!(
                            materialization.controller_arm_requires_ready_evidence(arm),
                            expected_controller_cross_role_recv
                        );
                        assert_eq!(
                            materialization.recv_entry(arm),
                            worker
                                .cursor
                                .route_scope_arm_recv_index(scope, arm)
                                .map(StateIndex::from_usize)
                        );
                        assert_eq!(
                            materialization.passive_arm_entry(arm),
                            worker
                                .cursor
                                .follow_passive_observer_arm_for_scope(scope, arm)
                                .map(|nav| match nav {
                                    PassiveArmNavigation::WithinArm { entry } => entry,
                                })
                        );
                        let mut lane_idx = 0usize;
                        while lane_idx < worker.cursor.logical_lane_count() {
                            let mut expected_binding_demux_lane = false;
                            if let Some((entry, _)) =
                                worker.cursor.controller_arm_entry_by_arm(scope, arm)
                                && let Some(recv_meta) =
                                    worker.cursor.try_recv_meta_at(state_index_to_usize(entry))
                                && recv_meta.lane as usize == lane_idx
                            {
                                expected_binding_demux_lane = true;
                            }
                            if let Some(entry) =
                                worker.cursor.route_scope_arm_recv_index(scope, arm)
                                && let Some(recv_meta) = worker.cursor.try_recv_meta_at(entry)
                                && recv_meta.lane as usize == lane_idx
                            {
                                expected_binding_demux_lane = true;
                            }
                            let mut dispatch_idx = 0usize;
                            while let Some((frame_label, lane, dispatch_arm, target)) = worker
                                .cursor
                                .route_scope_first_recv_dispatch_entry(scope, dispatch_idx)
                            {
                                if (dispatch_arm == arm || dispatch_arm == ARM_SHARED)
                                    && let Some(recv_meta) =
                                        worker.cursor.try_recv_meta_at(state_index_to_usize(target))
                                    && recv_meta.frame_label == frame_label
                                    && recv_meta.lane == lane
                                    && lane as usize == lane_idx
                                {
                                    expected_binding_demux_lane = true;
                                }
                                dispatch_idx += 1;
                            }
                            assert_eq!(
                                worker.binding_demux_contains_lane(scope, Some(arm), lane_idx),
                                expected_binding_demux_lane
                            );
                            lane_idx += 1;
                        }
                        if arm == 1 {
                            break;
                        }
                        arm += 1;
                    }
                    let mut dispatch_idx = 0usize;
                    while let Some((frame_label, lane, arm, target)) = worker
                        .cursor
                        .route_scope_first_recv_dispatch_entry(scope, dispatch_idx)
                    {
                        assert_eq!(
                            materialization
                                .first_recv_target_for_lane_frame_label(lane, frame_label),
                            Some((arm, target))
                        );
                        dispatch_idx += 1;
                    }
                    assert_eq!(materialization.first_recv_len as usize, dispatch_idx);
                });
            });
        },
    );
}

#[test]
fn attach_endpoint_keeps_primary_lane_on_first_live_application_lane() {
    run_offer_regression_test(
        "attach_endpoint_keeps_primary_lane_on_first_live_application_lane",
        || {
            offer_fixture!(2048, clock, config);
            type LaneThreeWorkerSteps =
                StepCons<SendStep<Role<0>, Role<1>, Msg<0x66, u8>, 3>, StepNil>;
            let lane_three_program: g::Program<LaneThreeWorkerSteps> =
                g::send::<Role<0>, Role<1>, Msg<0x66, u8>, 3>();

            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(998);
                    let worker_program: RoleProgram<1> = project(&lane_three_program);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &worker_program,
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    assert_eq!(
                        worker.primary_lane, 3,
                        "primary lane must follow the first live application lane instead of falling back to lane 0",
                    );
                    assert!(
                        worker.ports[worker.primary_lane].is_some(),
                        "the live primary lane must hold the leased primary port"
                    );
                });
            });
        },
    );
}

#[test]
fn selection_materialization_helpers_match_reference_lookup_logic() {
    run_offer_regression_test(
        "selection_materialization_helpers_match_reference_lookup_logic",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(999);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller = controller_slot.borrow_mut();
                        let worker = worker_slot.borrow_mut();

                        controller.refresh_lane_offer_state(0);
                        let controller_scope = controller.cursor.node_scope_id();
                        let controller_selection = RouteFrontierMachine::new(&mut *controller)
                            .select_scope()
                            .expect("controller selection");
                        worker.refresh_lane_offer_state(0);
                        let worker_scope = worker.cursor.node_scope_id();
                        let worker_selection = RouteFrontierMachine::new(&mut *worker)
                            .select_scope()
                            .expect("worker selection");

                        let mut arm = 0u8;
                        while arm <= 1 {
                            assert_eq!(
                                controller.selection_arm_requires_materialization_ready_evidence(
                                    controller_selection,
                                    true,
                                    arm,
                                ),
                                controller.arm_requires_materialization_ready_evidence(
                                    controller_scope,
                                    arm
                                )
                            );
                            assert_eq!(
                                worker.selection_arm_requires_materialization_ready_evidence(
                                    worker_selection,
                                    false,
                                    arm,
                                ),
                                if worker_selection.at_route_offer_entry
                                    && worker
                                        .selection_materialization_meta(worker_selection)
                                        .passive_arm_entry(arm)
                                        .is_some()
                                {
                                    if worker
                                        .selection_materialization_meta(worker_selection)
                                        .arm_has_first_recv_dispatch(arm)
                                    {
                                        !worker.selection_arm_dispatch_materializes_without_ready_evidence(
                                            worker_selection,
                                            arm,
                                        )
                                    } else {
                                        false
                                    }
                                } else {
                                    worker.arm_requires_materialization_ready_evidence(
                                        worker_scope,
                                        arm,
                                    )
                                }
                            );
                            assert_eq!(
                                controller.selection_non_wire_loop_control_recv(
                                    controller_selection,
                                    true,
                                    arm,
                                    TEST_LOOP_CONTINUE_LOGICAL,
                                ),
                                controller.is_non_wire_loop_control_recv(
                                    controller_scope,
                                    arm,
                                    TEST_LOOP_CONTINUE_LOGICAL,
                                )
                            );
                            assert_eq!(
                                controller.selection_non_wire_loop_control_recv(
                                    controller_selection,
                                    true,
                                    arm,
                                    TEST_LOOP_BREAK_LOGICAL,
                                ),
                                controller.is_non_wire_loop_control_recv(
                                    controller_scope,
                                    arm,
                                    TEST_LOOP_BREAK_LOGICAL,
                                )
                            );
                            assert_eq!(
                                worker.selection_non_wire_loop_control_recv(
                                    worker_selection,
                                    false,
                                    arm,
                                    TEST_LOOP_CONTINUE_LOGICAL,
                                ),
                                worker.is_non_wire_loop_control_recv(
                                    worker_scope,
                                    arm,
                                    TEST_LOOP_CONTINUE_LOGICAL
                                )
                            );
                            assert_eq!(
                                worker.selection_non_wire_loop_control_recv(
                                    worker_selection,
                                    false,
                                    arm,
                                    TEST_LOOP_BREAK_LOGICAL,
                                ),
                                worker.is_non_wire_loop_control_recv(
                                    worker_scope,
                                    arm,
                                    TEST_LOOP_BREAK_LOGICAL
                                )
                            );
                            if arm == 1 {
                                break;
                            }
                            arm += 1;
                        }
                    });
                });
            });
        },
    );
}

#[test]
fn scope_arm_materialization_meta_caches_passive_recv_meta_exactly() {
    run_offer_regression_test(
        "scope_arm_materialization_meta_caches_passive_recv_meta_exactly",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(998);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &ENTRY_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    worker.refresh_lane_offer_state(0);
                    let offer_lane = worker.offer_lane_for_scope(scope);
                    let materialization_meta = worker.compute_scope_arm_materialization_meta(scope);
                    let passive_recv_meta = worker.compute_scope_passive_recv_meta(
                        materialization_meta,
                        scope,
                        offer_lane,
                    );
                    let region = worker
                        .cursor
                        .scope_region_by_id(scope)
                        .expect("scope region should exist");

                    let mut arm = 0u8;
                    while arm <= 1 {
                        let expected = worker
                            .cursor
                            .follow_passive_observer_arm_for_scope(scope, arm)
                            .map(|nav| match nav {
                                PassiveArmNavigation::WithinArm { entry } => entry,
                            })
                            .and_then(|entry| {
                                let target_idx = state_index_to_usize(entry);
                                if let Some(recv_meta) = worker.cursor.try_recv_meta_at(target_idx)
                                {
                                    return Some((target_idx, recv_meta));
                                }
                                if let Some(send_meta) = worker.cursor.try_send_meta_at(target_idx)
                                {
                                    return Some((
                                        target_idx,
                                        RecvMeta {
                                            eff_index: send_meta.eff_index,
                                            label: send_meta.label,
                                            frame_label: send_meta.frame_label,
                                            peer: send_meta.peer,
                                            resource: send_meta.resource,
                                            semantic: send_meta.semantic,
                                            is_control: send_meta.is_control,
                                            next: target_idx,
                                            scope,
                                            route_arm: Some(arm),
                                            is_choice_determinant: false,
                                            shot: send_meta.shot,
                                            policy: send_meta.policy(),
                                            lane: send_meta.lane,
                                        },
                                    ));
                                }
                                if worker.cursor.is_jump_at(target_idx) {
                                    let scope_end =
                                        worker.cursor.jump_target_at(target_idx).unwrap_or(0);
                                    if region.linger {
                                        let (controller_entry, synthetic_label) =
                                            materialization_meta.controller_arm_entry(arm)?;
                                        let synthetic_semantic = loop_control_semantic_kind(
                                            worker.cursor.control_semantic_at(
                                                state_index_to_usize(controller_entry),
                                            ),
                                        )
                                        .unwrap_or(ControlSemanticKind::RouteArm);
                                        return Some((
                                            scope_end,
                                            RecvMeta {
                                                eff_index: EffIndex::ZERO,
                                                label: synthetic_label,
                                                frame_label: 0,
                                                peer: 1,
                                                resource: None,
                                                semantic: synthetic_semantic,
                                                is_control: true,
                                                next: scope_end,
                                                scope,
                                                route_arm: Some(arm),
                                                is_choice_determinant: false,
                                                shot: None,
                                                policy: PolicyMode::static_mode(),
                                                lane: offer_lane,
                                            },
                                        ));
                                    }
                                    if let Some(recv_meta) =
                                        worker.cursor.try_recv_meta_at(scope_end)
                                    {
                                        return Some((scope_end, recv_meta));
                                    }
                                    if let Some(send_meta) =
                                        worker.cursor.try_send_meta_at(scope_end)
                                    {
                                        return Some((
                                            scope_end,
                                            RecvMeta {
                                                eff_index: send_meta.eff_index,
                                                label: send_meta.label,
                                                frame_label: send_meta.frame_label,
                                                peer: send_meta.peer,
                                                resource: send_meta.resource,
                                                semantic: send_meta.semantic,
                                                is_control: send_meta.is_control,
                                                next: scope_end,
                                                scope,
                                                route_arm: Some(arm),
                                                is_choice_determinant: false,
                                                shot: send_meta.shot,
                                                policy: send_meta.policy(),
                                                lane: send_meta.lane,
                                            },
                                        ));
                                    }
                                    return None;
                                }
                                if region.linger {
                                    let (controller_entry, synthetic_label) =
                                        materialization_meta.controller_arm_entry(arm)?;
                                    let synthetic_semantic = loop_control_semantic_kind(
                                        worker.cursor.control_semantic_at(state_index_to_usize(
                                            controller_entry,
                                        )),
                                    )
                                    .unwrap_or(ControlSemanticKind::RouteArm);
                                    return Some((
                                        target_idx,
                                        RecvMeta {
                                            eff_index: EffIndex::ZERO,
                                            label: synthetic_label,
                                            frame_label: 0,
                                            peer: 1,
                                            resource: None,
                                            semantic: synthetic_semantic,
                                            is_control: true,
                                            next: target_idx,
                                            scope,
                                            route_arm: Some(arm),
                                            is_choice_determinant: false,
                                            shot: None,
                                            policy: PolicyMode::static_mode(),
                                            lane: offer_lane,
                                        },
                                    ));
                                }
                                None
                            });
                        let cached = passive_recv_meta
                            .get(arm as usize)
                            .copied()
                            .and_then(|meta| meta.recv_meta());
                        assert_eq!(cached, expected);
                        if region.linger {
                            assert!(
                                materialization_meta.controller_arm_entry(arm).is_some(),
                                "passive linger route must retain controller arm facts for arm {arm}"
                            );
                            let cached_semantic = cached.map(|(_, meta)| meta.semantic);
                            let expected_semantic = materialization_meta
                                .controller_arm_entry(arm)
                                .and_then(|(entry, _)| {
                                    loop_control_semantic_kind(
                                        worker
                                            .cursor
                                            .control_semantic_at(state_index_to_usize(entry)),
                                    )
                                });
                            assert_eq!(cached_semantic, expected_semantic);
                        }
                        if arm == 1 {
                            break;
                        }
                        arm += 1;
                    }
                });
            });
        },
    );
}

#[test]
fn align_cursor_to_selected_scope_skips_observation_for_single_active_entry() {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_skips_observation_for_single_active_entry",
        || {
            offer_fixture!(2048, clock, config);
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(998);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();

                        worker.refresh_lane_offer_state(0);
                        let current_idx = worker.cursor.index();
                        assert!(
                            worker
                                .active_frontier_entries(None)
                                .contains_only(current_idx)
                        );
                        let observed_key = worker.cached_global_frontier_observation_key();
                        let observed_entries = worker.global_frontier_observed_entries();

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect("single current entry should select directly");

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert!(
                            worker.cached_global_frontier_observation_key() == observed_key,
                            "single-active fast path must not rebuild cached observation key during align"
                        );
                        assert!(
                            worker
                                .global_frontier_observed_entries()
                                .entry_bit(current_idx)
                                == observed_entries.entry_bit(current_idx)
                                && worker.frontier_state.global_frontier_observed.progress_mask
                                    == observed_entries.progress_mask
                                && worker
                                    .frontier_state
                                    .global_frontier_observed
                                    .ready_arm_mask
                                    == observed_entries.ready_arm_mask
                                && worker.frontier_state.global_frontier_observed.ready_mask
                                    == observed_entries.ready_mask,
                            "single-active fast path must not rebuild observation during align"
                        );
                    });
                }
            );
        },
    );
}

#[test]
fn align_cursor_to_selected_scope_reuses_cached_multi_entry_observation() {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_reuses_cached_multi_entry_observation",
        || {
            offer_fixture!(2048, clock, config);
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(999);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();

                        worker.refresh_lane_offer_state(0);
                        let current_idx = worker.cursor.index();
                        let (_active_slots, active_entries) =
                            active_entry_set_from_pairs(&[(current_idx, 0)]);
                        overwrite_global_active_entries_fixture(&mut *worker, active_entries);
                        let (_observed_slots, observed_entries) =
                            observed_entries_with_ready_current_only(current_idx);
                        overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);
                        let stored_key = RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            ScopeId::none(),
                            false,
                        );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 17;

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect("fresh cached observation should be reused");

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert_eq!(
                            worker.frontier_state.frontier_observation_epoch, 17,
                            "cache hit must not rebuild frontier observation"
                        );
                    });
                }
            );
        },
    );
}

#[test]
fn align_cursor_to_selected_scope_ignores_unrelated_lane_binding_changes() {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_ignores_unrelated_lane_binding_changes",
        || {
            offer_fixture!(2048, clock, config);
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1000);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();

                        worker.refresh_lane_offer_state(0);
                        let current_idx = worker.cursor.index();
                        let (_active_slots, active_entries) =
                            active_entry_set_from_pairs(&[(current_idx, 0)]);
                        overwrite_global_active_entries_fixture(&mut *worker, active_entries);
                        let (_observed_slots, observed_entries) =
                            observed_entries_with_ready_current_only(current_idx);
                        overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);
                        let stored_key = RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            ScopeId::none(),
                            false,
                        );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 23;

                        let unrelated = crate::binding::IngressEvidence {
                            frame_label: FrameLabel::new(91),
                            channel: crate::binding::Channel::new(7),
                            instance: 7,
                            has_fin: false,
                        };
                        assert!(worker.binding_inbox.push_back(2, unrelated));

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect(
                                "unrelated binding changes must not invalidate cached observation",
                            );

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert_eq!(
                            worker.frontier_state.frontier_observation_epoch, 23,
                            "cache hit must survive unrelated-lane binding updates"
                        );
                    });
                }
            );
        },
    );
}

#[test]
fn align_cursor_to_selected_scope_ignores_relevant_lane_binding_content_changes() {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_ignores_relevant_lane_binding_content_changes",
        || {
            offer_fixture!(2048, clock, config);
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1003);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();

                        worker.refresh_lane_offer_state(0);
                        let current_idx = worker.cursor.index();
                        let (_active_slots, active_entries) =
                            active_entry_set_from_pairs(&[(current_idx, 0)]);
                        overwrite_global_active_entries_fixture(&mut *worker, active_entries);
                        let (_observed_slots, observed_entries) =
                            observed_entries_with_ready_current_only(current_idx);
                        overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);

                        let first = crate::binding::IngressEvidence {
                            frame_label: FrameLabel::new(31),
                            channel: crate::binding::Channel::new(3),
                            instance: 3,
                            has_fin: false,
                        };
                        let second = crate::binding::IngressEvidence {
                            frame_label: FrameLabel::new(32),
                            channel: crate::binding::Channel::new(4),
                            instance: 4,
                            has_fin: false,
                        };
                        assert!(worker.binding_inbox.push_back(0, first));
                        let stored_key = RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            ScopeId::none(),
                            false,
                        );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 27;

                        assert!(worker.binding_inbox.push_back(0, second));

                        RouteFrontierMachine::new(&mut *worker).align_cursor_to_selected_scope().expect(
                            "relevant lane content-only changes must not invalidate cached observation",
                        );

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert_eq!(
                            worker.frontier_state.frontier_observation_epoch, 27,
                            "cache hit must survive content-only updates on already-nonempty offer lanes"
                        );
                    });
                }
            );
        },
    );
}

#[test]
fn align_cursor_to_selected_scope_ignores_unrelated_scope_evidence_changes() {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_ignores_unrelated_scope_evidence_changes",
        || {
            offer_fixture!(2048, clock, config);
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1001);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();

                        if crate::eff::meta::MAX_EFF_NODES < 2 {
                            return;
                        }

                        worker.refresh_lane_offer_state(0);
                        let current_idx = worker.cursor.index();
                        let (_active_slots, active_entries) =
                            active_entry_set_from_pairs(&[(current_idx, 0)]);
                        overwrite_global_active_entries_fixture(&mut *worker, active_entries);
                        let (_observed_slots, observed_entries) =
                            observed_entries_with_ready_current_only(current_idx);
                        overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);
                        let stored_key = RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            ScopeId::none(),
                            false,
                        );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 29;

                        let current_scope_slot = worker
                            .scope_slot_for_route(worker.cursor.node_scope_id())
                            .expect("current node scope should be a route scope");
                        if worker.cursor.route_scope_count() < 2 {
                            return;
                        }
                        let unrelated_slot = if current_scope_slot == 0 { 1 } else { 0 };
                        worker.route_state.scope_evidence[unrelated_slot].ready_arm_mask =
                            ScopeEvidence::ARM0_READY;
                        worker.bump_scope_evidence_generation(unrelated_slot);

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect(
                                "unrelated scope evidence must not invalidate cached observation",
                            );

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert_eq!(
                            worker.frontier_state.frontier_observation_epoch, 29,
                            "cache hit must survive unrelated-scope evidence updates"
                        );
                    });
                }
            );
        },
    );
}

#[test]
fn align_cursor_to_selected_scope_ignores_unrelated_lane_frontier_refresh() {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_ignores_unrelated_lane_frontier_refresh",
        || {
            offer_fixture!(2048, clock, config);
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1002);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();

                        assert!(worker.cursor.logical_lane_count() > 2);

                        worker.refresh_lane_offer_state(0);
                        let current_idx = worker.cursor.index();
                        let (_active_slots, active_entries) =
                            active_entry_set_from_pairs(&[(current_idx, 0)]);
                        overwrite_global_active_entries_fixture(&mut *worker, active_entries);
                        let (_observed_slots, observed_entries) =
                            observed_entries_with_ready_current_only(current_idx);
                        overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);
                        let stored_key = RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            ScopeId::none(),
                            false,
                        );
                        overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                        worker.frontier_state.frontier_observation_epoch = 31;

                        worker.refresh_lane_offer_state(2);

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect("unrelated lane frontier refresh must not invalidate cached observation");

                        assert_eq!(worker.cursor.index(), current_idx);
                        assert_eq!(
                            worker.frontier_state.frontier_observation_epoch, 31,
                            "cache hit must survive unrelated-lane frontier refresh"
                        );
                    });
                }
            );
        },
    );
}

#[test]
fn align_cursor_to_selected_scope_keeps_descended_nested_route_entry_authoritative() {
    run_offer_regression_test(
        "align_cursor_to_selected_scope_keeps_descended_nested_route_entry_authoritative",
        || {
            offer_fixture!(2048, clock, config);
            let nested_program = NESTED_ROUTE_PROGRAM();
            let worker_program = project(&nested_program);
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1004);
                    type WorkerEndpoint = CursorEndpoint<
                        'static,
                        1,
                        HintOnlyTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        crate::control::cap::mint::EpochTbl,
                        4,
                        crate::control::cap::mint::MintConfig,
                        NoBinding,
                    >;
                    let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                                rv_id,
                                sid,
                                &worker_program,
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker =
                        unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };
                    let nested_scope = worker
                        .cursor
                        .seek_label_index(ENTRY_ARM0_SIGNAL_LABEL)
                        .map(|idx| worker.cursor.node_scope_id_at(idx))
                        .expect("nested route recv label must exist");

                    worker.refresh_lane_offer_state(0);
                    let outer_scope = worker.cursor.node_scope_id();
                    let outer_entry = worker.cursor.index();
                    let nested_entry = worker
                        .route_scope_offer_entry_index(nested_scope)
                        .expect("nested route must have offer entry");

                    assert_ne!(outer_entry, nested_entry);
                    worker
                        .test_commit_route_arm(0, outer_scope, 1)
                        .expect("select outer nested arm");
                    worker
                        .test_commit_route_arm(0, nested_scope, 0)
                        .expect("select nested arm");
                    worker.set_cursor_index(nested_entry);

                    assert_eq!(
                        worker.cursor.node_scope_id(),
                        nested_scope,
                        "cursor must already be positioned at the descended nested route",
                    );
                    assert_eq!(
                        worker.current_offer_scope_id(),
                        nested_scope,
                        "selected nested route must become the current offer scope",
                    );
                    assert_eq!(
                        worker.route_state.lane_offer_state(0).scope,
                        outer_scope,
                        "pre-align lane state intentionally still points at the ancestor route",
                    );

                    RouteFrontierMachine::new(worker)
                        .align_cursor_to_selected_scope()
                        .expect("selected nested route entry should remain authoritative");

                    assert_eq!(
                        worker.cursor.index(),
                        nested_entry,
                        "align must not bounce a selected nested route entry back to the ancestor scope",
                    );
                    assert_eq!(worker.current_offer_scope_id(), nested_scope);
                    unsafe {
                        core::ptr::drop_in_place(worker);
                    }
                }
            );
        },
    );
}

#[test]
fn active_entry_set_orders_entries_by_representative_lane() {
    let (_entry_slots, mut entries) = active_entry_set_storage(3);
    assert!(entries.insert_entry(9, 4));
    assert!(entries.insert_entry(3, 1));
    assert!(entries.insert_entry(7, 1));
    assert_eq!(entries.entry_at(0), Some(3));
    assert_eq!(entries.entry_at(1), Some(7));
    assert_eq!(entries.entry_at(2), Some(9));

    assert!(entries.remove_entry(3));
    assert_eq!(entries.entry_at(0), Some(7));
    assert_eq!(entries.entry_at(1), Some(9));
    assert_eq!(entries.occupancy_mask(), 0b0000_0011);
}

#[test]
fn current_passive_without_evidence_keeps_priority_with_controller_present() {
    assert!(!current_entry_is_candidate(false, false, false, 0, false,));
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
fn current_passive_with_evidence_keeps_priority() {
    assert!(current_entry_is_candidate(true, false, true, 1, false,));
}

#[test]
fn current_passive_without_controller_keeps_priority() {
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
fn current_passive_observer_without_evidence_keeps_priority() {
    assert!(current_entry_is_candidate(true, false, false, 1, false,));
}

#[test]
fn current_candidate_stays_selectable_without_route_lane_metadata() {
    assert!(current_entry_matches_after_filter(true, true, 43, None));
}

#[test]
fn current_candidate_respects_hint_filter() {
    assert!(!current_entry_matches_after_filter(
        true,
        true,
        43,
        Some(47)
    ));
}

#[test]
fn current_without_candidate_stays_blocked() {
    assert!(!current_entry_matches_after_filter(false, true, 43, None));
}

#[test]
fn current_without_offer_lanes_stays_blocked() {
    assert!(!current_entry_matches_after_filter(true, false, 43, None));
}

#[test]
fn offer_entry_observed_state_merges_static_summary_and_dynamic_evidence() {
    let mut summary = OfferEntryStaticSummary::EMPTY;
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        flags: LaneOfferState::FLAG_CONTROLLER,
        ..LaneOfferState::EMPTY
    });
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::Parallel,
        static_ready: true,
        flags: LaneOfferState::FLAG_DYNAMIC,
        ..LaneOfferState::EMPTY
    });
    let observed = offer_entry_observed_state(ScopeId::generic(41), summary, true, false, true);

    assert_eq!(observed.scope_id, ScopeId::generic(41));
    assert!(observed.matches_frontier(FrontierKind::Parallel));
    assert!(observed.is_controller());
    assert!(observed.is_dynamic());
    assert!(observed.has_progress_evidence());
    assert!(observed.has_ready_arm_evidence());
    assert!(observed.binding_ready());
    assert_ne!(observed.flags & OfferEntryObservedState::FLAG_READY, 0);
}

#[test]
fn cached_offer_entry_observed_state_preserves_arbitration_bits() {
    let mut summary = OfferEntryStaticSummary::EMPTY;
    summary.observe_lane(LaneOfferState {
        frontier: FrontierKind::PassiveObserver,
        flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
        ..LaneOfferState::EMPTY
    });
    let observed = offer_entry_observed_state(ScopeId::generic(51), summary, true, false, true);
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(1);
    let (observed_bit, inserted) = observed_entries.insert_entry(17).expect("insert entry");
    assert!(inserted);
    observed_entries.observe(observed_bit, observed);

    let cached = cached_offer_entry_observed_state(
        ScopeId::generic(51),
        summary,
        observed_entries,
        observed_bit,
    );
    let original_candidate = offer_entry_frontier_candidate(
        ScopeId::generic(51),
        17,
        ScopeId::generic(9),
        FrontierKind::PassiveObserver,
        observed,
    );
    let cached_candidate = offer_entry_frontier_candidate(
        ScopeId::generic(51),
        17,
        ScopeId::generic(9),
        FrontierKind::PassiveObserver,
        cached,
    );

    assert!(cached.matches_frontier(FrontierKind::PassiveObserver));
    assert!(cached.is_controller());
    assert!(cached.is_dynamic());
    assert!(cached.has_progress_evidence());
    assert!(cached.has_ready_arm_evidence());
    assert!(cached.ready());
    assert_eq!(cached_candidate.scope_id, original_candidate.scope_id);
    assert_eq!(
        cached_candidate.parallel_root,
        original_candidate.parallel_root
    );
    assert_eq!(cached_candidate.frontier, original_candidate.frontier);
    assert_eq!(
        cached_candidate.is_controller(),
        original_candidate.is_controller()
    );
    assert_eq!(
        cached_candidate.is_dynamic(),
        original_candidate.is_dynamic()
    );
    assert_eq!(
        cached_candidate.has_evidence(),
        original_candidate.has_evidence()
    );
    assert_eq!(cached_candidate.ready(), original_candidate.ready());
}

#[test]
fn observed_entry_set_entry_bit_tracks_inserted_entries_exactly() {
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (first_bit, inserted_first) = observed_entries.insert_entry(17).expect("insert first");
    assert!(inserted_first);
    let (second_bit, inserted_second) = observed_entries.insert_entry(3).expect("insert second");
    assert!(inserted_second);
    let (reused_bit, inserted_reused) = observed_entries.insert_entry(17).expect("reuse first");
    assert!(!inserted_reused);
    assert_eq!(reused_bit, first_bit);
    assert_eq!(observed_entries.entry_bit(17), first_bit);
    assert_eq!(observed_entries.entry_bit(3), second_bit);
    assert_eq!(observed_entries.entry_bit(9), 0);
}

fn observed_entries_with_ready_current_only(
    current_idx: usize,
) -> (std::vec::Vec<FrontierObservationSlot>, ObservedEntrySet) {
    observed_entry_set_from_states(&[(
        current_idx,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY,
        },
    )])
}

#[test]
fn refresh_cached_frontier_observation_entry_updates_stable_slot_in_place() {
    run_offer_regression_test(
        "refresh_cached_frontier_observation_entry_updates_stable_slot_in_place",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1013);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();

                    worker.refresh_lane_offer_state(0);
                    let current_idx = worker.cursor.index();
                    let mut summary = worker.compute_offer_entry_static_summary(current_idx);
                    summary.flags &= !OfferEntryStaticSummary::FLAG_STATIC_READY;
                    worker
                        .route_state
                        .lane_offer_state_mut(0)
                        .expect("lane 0 offer state")
                        .static_ready = false;

                    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(1);
                    let (observed_bit, inserted) = observed_entries
                        .insert_entry(current_idx)
                        .expect("insert current entry");
                    assert!(inserted);
                    observed_entries.observe(
                        observed_bit,
                        offer_entry_observed_state(
                            worker
                                .offer_entry_state_snapshot(current_idx)
                                .expect("offer entry state snapshot")
                                .scope_id,
                            summary,
                            false,
                            false,
                            false,
                        ),
                    );
                    overwrite_global_frontier_observed_fixture(&mut *worker, observed_entries);
                    let stored_key = RouteFrontierMachine::frontier_observation_key(
                        &worker,
                        ScopeId::none(),
                        false,
                    );
                    overwrite_global_frontier_observed_key_fixture(&mut *worker, stored_key);
                    worker.frontier_state.frontier_observation_epoch = 41;
                    assert_eq!(
                        worker.frontier_state.global_frontier_observed.ready_mask & observed_bit,
                        0
                    );

                    worker
                        .route_state
                        .lane_offer_state_mut(0)
                        .expect("lane 0 offer state")
                        .static_ready = true;
                    let updated_key = RouteFrontierMachine::frontier_observation_key(
                        &worker,
                        ScopeId::none(),
                        false,
                    );
                    assert!(
                        worker
                            .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
                            .is_none(),
                        "summary fingerprint change should invalidate the stale cached key before patching",
                    );

                    assert!(
                        worker.refresh_cached_frontier_observation_entry(
                            ScopeId::none(),
                            false,
                            current_idx
                        ),
                        "stable active-entry slot should patch the cached frontier observation in place",
                    );
                    assert!(
                        worker.cached_global_frontier_observation_key() == updated_key,
                        "targeted patch should publish the refreshed observation under the new key",
                    );
                    let current_bit = worker
                        .global_frontier_observed_entries()
                        .entry_bit(current_idx);
                    assert_ne!(current_bit, 0);
                    assert_ne!(
                        worker.frontier_state.global_frontier_observed.ready_mask & current_bit,
                        0,
                        "patched observation should reflect the updated static ready bit",
                    );
                    assert!(
                        worker.frontier_state.frontier_observation_epoch > 41,
                        "targeted patch should publish a fresh frontier observation epoch",
                    );
                });
            });
        },
    );
}

#[test]
fn observed_entry_set_move_entry_slot_remaps_masks_exactly() {
    let current_idx = 17usize;
    let fake_entry_idx = 23usize;
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );
    let (fake_bit, inserted_fake) = observed_entries
        .insert_entry(fake_entry_idx)
        .expect("insert fake entry");
    assert!(inserted_fake);
    observed_entries.observe(
        fake_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    );

    assert!(observed_entries.move_entry_slot(fake_entry_idx, 0));
    assert_eq!(observed_entries.entry_bit(fake_entry_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 1);
    assert_eq!(observed_entries.controller_mask, 1u8 << 0);
    assert_eq!(observed_entries.progress_mask, 1u8 << 1);
    assert_eq!(observed_entries.ready_mask, 1u8 << 1);
    assert_eq!(
        observed_entries.frontier_mask(FrontierKind::Parallel),
        1 << 0
    );
    assert_eq!(
        observed_entries.frontier_mask(FrontierKind::Route),
        1u8 << 1
    );
}

#[test]
fn observed_entry_set_insert_observation_at_slot_remaps_masks_exactly() {
    let current_idx = 17usize;
    let fake_entry_idx = 23usize;
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );

    assert!(observed_entries.insert_observation_at_slot(
        fake_entry_idx,
        0,
        FrontierObservationSlot {
            entry: StateIndex::new(fake_entry_idx as u16),
            meta: FrontierObservationMetaSlot::EMPTY,
        },
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    ));
    assert_eq!(observed_entries.entry_bit(fake_entry_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 1);
    assert_eq!(observed_entries.controller_mask, 1u8 << 0);
    assert_eq!(observed_entries.progress_mask, 1u8 << 1);
    assert_eq!(observed_entries.ready_mask, 1u8 << 1);
    assert_eq!(
        observed_entries.frontier_mask(FrontierKind::Parallel),
        1 << 0
    );
    assert_eq!(
        observed_entries.frontier_mask(FrontierKind::Route),
        1u8 << 1
    );
}

#[test]
fn observed_entry_set_remove_observation_remaps_masks_exactly() {
    let current_idx = 17usize;
    let fake_entry_idx = 23usize;
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );
    assert!(observed_entries.insert_observation_at_slot(
        fake_entry_idx,
        0,
        FrontierObservationSlot {
            entry: StateIndex::new(fake_entry_idx as u16),
            meta: FrontierObservationMetaSlot::EMPTY,
        },
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    ));

    assert!(observed_entries.remove_observation(fake_entry_idx));
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(fake_entry_idx), 0);
    assert_eq!(observed_entries.controller_mask, 0);
    assert_eq!(observed_entries.progress_mask, 1u8 << 0);
    assert_eq!(observed_entries.ready_mask, 1u8 << 0);
    assert_eq!(observed_entries.frontier_mask(FrontierKind::Parallel), 0);
    assert_eq!(observed_entries.frontier_mask(FrontierKind::Route), 1 << 0);
}

#[test]
fn observed_entry_set_replace_entry_at_slot_remaps_masks_exactly() {
    let current_idx = 17usize;
    let old_entry_idx = 23usize;
    let new_entry_idx = 29usize;
    let (_observed_slots, mut observed_entries) = observed_entry_set_storage(2);
    let (current_bit, inserted_current) = observed_entries
        .insert_entry(current_idx)
        .expect("insert current entry");
    assert!(inserted_current);
    observed_entries.observe(
        current_bit,
        OfferEntryObservedState {
            scope_id: ScopeId::generic(7),
            frontier_mask: FrontierKind::Route.bit(),
            flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
        },
    );
    assert!(observed_entries.insert_observation_at_slot(
        old_entry_idx,
        1,
        FrontierObservationSlot {
            entry: StateIndex::new(old_entry_idx as u16),
            meta: FrontierObservationMetaSlot::EMPTY,
        },
        OfferEntryObservedState {
            scope_id: ScopeId::generic(8),
            frontier_mask: FrontierKind::Parallel.bit(),
            flags: OfferEntryObservedState::FLAG_CONTROLLER,
        },
    ));

    assert!(observed_entries.replace_entry_at_slot(
        old_entry_idx,
        new_entry_idx,
        FrontierObservationSlot {
            entry: StateIndex::new(new_entry_idx as u16),
            meta: FrontierObservationMetaSlot::EMPTY,
        },
        OfferEntryObservedState {
            scope_id: ScopeId::generic(9),
            frontier_mask: FrontierKind::Loop.bit(),
            flags: OfferEntryObservedState::FLAG_READY_ARM | OfferEntryObservedState::FLAG_DYNAMIC,
        },
    ));
    assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 0);
    assert_eq!(observed_entries.entry_bit(old_entry_idx), 0);
    assert_eq!(observed_entries.entry_bit(new_entry_idx), 1u8 << 1);
    assert_eq!(observed_entries.controller_mask, 0);
    assert_eq!(observed_entries.dynamic_controller_mask, 1u8 << 1);
    assert_eq!(observed_entries.progress_mask, 1u8 << 0);
    assert_eq!(observed_entries.ready_arm_mask, 1u8 << 1);
    assert_eq!(observed_entries.ready_mask, 1u8 << 0);
    assert_eq!(observed_entries.frontier_mask(FrontierKind::Parallel), 0);
    assert_eq!(observed_entries.frontier_mask(FrontierKind::Loop), 1u8 << 1);
    assert_eq!(observed_entries.frontier_mask(FrontierKind::Route), 1 << 0);
}

#[test]
fn frontier_observation_structural_entry_detection_is_exact() {
    with_active_entry_set_storage(2, |cached_entries| {
        assert!(cached_entries.insert_entry(11, 0));
        assert!(cached_entries.insert_entry(17, 0));

        with_frontier_observation_key_storage(2, 1, |cached_key| {
            cached_key.set_active_entries_from(*cached_entries);

            with_active_entry_set_storage(3, |inserted_entries| {
                inserted_entries.copy_from(*cached_entries);
                assert!(inserted_entries.insert_entry(23, 0));
                assert_eq!(
                    CursorEndpoint::<
                        1,
                        HintOnlyTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        EpochTbl,
                        4,
                    >::structural_inserted_entry_idx(
                        *inserted_entries, *cached_key
                    ),
                    Some(23)
                );

                with_frontier_observation_key_storage(3, 1, |inserted_key| {
                    inserted_key.set_active_entries_from(*inserted_entries);
                    assert_eq!(
                        CursorEndpoint::<
                            1,
                            HintOnlyTransport,
                            DefaultLabelUniverse,
                            CounterClock,
                            EpochTbl,
                            4,
                        >::structural_removed_entry_idx(
                            *cached_entries, *inserted_key
                        ),
                        Some(23)
                    );
                });
            });

            with_active_entry_set_storage(2, |replaced_entries| {
                assert!(replaced_entries.insert_entry(11, 0));
                assert!(replaced_entries.insert_entry(19, 0));
                assert_eq!(
                    CursorEndpoint::<
                        1,
                        HintOnlyTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        EpochTbl,
                        4,
                    >::structural_replaced_entry_idx(
                        *replaced_entries, *cached_key
                    ),
                    Some(19)
                );
            });
        });
    });

    with_active_entry_set_storage(2, |shifted_entries| {
        assert!(shifted_entries.insert_entry(17, 0));
        assert!(shifted_entries.insert_entry(11, 1));
        with_active_entry_set_storage(2, |shifted_cached_entries| {
            assert!(shifted_cached_entries.insert_entry(11, 0));
            assert!(shifted_cached_entries.insert_entry(17, 1));
            with_frontier_observation_key_storage(2, 1, |shifted_cached_key| {
                shifted_cached_key.set_active_entries_from(*shifted_cached_entries);
                assert_eq!(
                    CursorEndpoint::<
                        1,
                        HintOnlyTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        EpochTbl,
                        4,
                    >::structural_shifted_entry_idx(
                        *shifted_entries, *shifted_cached_key
                    ),
                    Some(17)
                );
            });
        });
    });
}

#[test]
fn cached_frontier_changed_entry_slot_mask_ignores_non_representative_route_lane_changes() {
    run_offer_regression_test(
        "cached_frontier_changed_entry_slot_mask_ignores_non_representative_route_lane_changes",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1013);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();

                    assert!(worker.cursor.logical_lane_count() > 2);
                    worker.refresh_lane_offer_state(0);
                    let current_idx = worker.cursor.index();
                    let (_active_slots, active_entries) =
                        active_entry_set_from_pairs(&[(current_idx, 0)]);
                    overwrite_global_active_entries_fixture(&mut *worker, active_entries);

                    let (
                        _cached_key_slots,
                        _cached_offer_lane_words,
                        _cached_binding_lane_words,
                        cached_key,
                    ) = copied_frontier_observation_key_storage(
                        RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            ScopeId::none(),
                            false,
                        ),
                        worker.cursor.max_frontier_entries(),
                        worker.cursor.logical_lane_count(),
                    );
                    let unrelated = crate::binding::IngressEvidence {
                        frame_label: FrameLabel::new(91),
                        channel: crate::binding::Channel::new(7),
                        instance: 7,
                        has_fin: false,
                    };
                    assert!(worker.binding_inbox.push_back(2, unrelated));
                    let observation_key = RouteFrontierMachine::frontier_observation_key(
                        &worker,
                        ScopeId::none(),
                        false,
                    );

                    let changed_slot_mask = worker
                        .cached_frontier_changed_entry_slot_mask(
                            ScopeId::none(),
                            false,
                            observation_key,
                            cached_key,
                        )
                        .expect("active frontier is unchanged");

                    assert_eq!(
                        changed_slot_mask, 0,
                        "route changes on non-representative offer lanes must not invalidate the entry"
                    );
                });
            });
        },
    );
}

#[test]
fn refresh_frontier_observed_entries_from_cache_updates_changed_offer_lane_slots() {
    run_offer_regression_test(
        "refresh_frontier_observed_entries_from_cache_updates_changed_offer_lane_slots",
        || {
            const OUTER_LEFT_LABEL: u8 = 0x61;
            const OUTER_RIGHT_LABEL: u8 = 0x62;
            const OUTER_LEFT_DATA_LABEL: u8 = 0x53;
            const INNER_LEFT_LABEL: u8 = 0x63;
            const INNER_RIGHT_LABEL: u8 = 0x64;
            const INNER_LEFT_DATA_LABEL: u8 = 0x54;
            const INNER_RIGHT_DATA_LABEL: u8 = 0x55;
            const INNER_REPLY_DATA_LABEL: u8 = 0x56;

            type InnerArm0 = SeqSteps<
                SendOnly<2, Role<0>, Role<0>, Msg<INNER_LEFT_LABEL, u8>>,
                SeqSteps<
                    SendOnly<2, Role<0>, Role<1>, Msg<INNER_LEFT_DATA_LABEL, u8>>,
                    SendOnly<2, Role<1>, Role<0>, Msg<INNER_REPLY_DATA_LABEL, u8>>,
                >,
            >;
            type InnerArm1 = SeqSteps<
                SendOnly<2, Role<0>, Role<0>, Msg<INNER_RIGHT_LABEL, u8>>,
                SendOnly<2, Role<0>, Role<1>, Msg<INNER_RIGHT_DATA_LABEL, u8>>,
            >;
            type InnerRouteSteps = RouteSteps<InnerArm0, InnerArm1>;
            type OuterLeftSteps = SeqSteps<
                SendOnly<0, Role<0>, Role<0>, Msg<OUTER_LEFT_LABEL, u8>>,
                SendOnly<0, Role<0>, Role<1>, Msg<OUTER_LEFT_DATA_LABEL, u8>>,
            >;
            type OuterRightSteps = SeqSteps<
                SendOnly<0, Role<0>, Role<0>, Msg<OUTER_RIGHT_LABEL, u8>>,
                InnerRouteSteps,
            >;
            type NestedSplitRouteSteps = RouteSteps<OuterLeftSteps, OuterRightSteps>;

            let inner_arm0_program: g::Program<InnerArm0> = g::seq(
                g::send::<Role<0>, Role<0>, Msg<INNER_LEFT_LABEL, u8>, 2>(),
                g::seq(
                    g::send::<Role<0>, Role<1>, Msg<INNER_LEFT_DATA_LABEL, u8>, 2>(),
                    g::send::<Role<1>, Role<0>, Msg<INNER_REPLY_DATA_LABEL, u8>, 2>(),
                ),
            );
            let inner_arm1_program: g::Program<InnerArm1> = g::seq(
                g::send::<Role<0>, Role<0>, Msg<INNER_RIGHT_LABEL, u8>, 2>(),
                g::send::<Role<0>, Role<1>, Msg<INNER_RIGHT_DATA_LABEL, u8>, 2>(),
            );
            let inner_route_program: g::Program<InnerRouteSteps> =
                g::route(inner_arm0_program, inner_arm1_program);
            let outer_left_program: g::Program<OuterLeftSteps> = g::seq(
                g::send::<Role<0>, Role<0>, Msg<OUTER_LEFT_LABEL, u8>, 0>(),
                g::send::<Role<0>, Role<1>, Msg<OUTER_LEFT_DATA_LABEL, u8>, 0>(),
            );
            let outer_right_program: g::Program<OuterRightSteps> = g::seq(
                g::send::<Role<0>, Role<0>, Msg<OUTER_RIGHT_LABEL, u8>, 0>(),
                inner_route_program,
            );
            let nested_split_route_program: g::Program<NestedSplitRouteSteps> =
                g::route(outer_left_program, outer_right_program);

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1008);
                    let worker_program: RoleProgram<1> = project(&nested_split_route_program);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &worker_program,
                                NoBinding,
                            )
                            .expect("attach nested worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();

                    let outer_scope = worker.cursor.node_scope_id();
                    assert!(
                        !outer_scope.is_none(),
                        "worker must start at outer route scope"
                    );
                    let nested_scope = worker
                        .cursor
                        .seek_label_index(INNER_LEFT_DATA_LABEL)
                        .map(|idx| worker.cursor.node_scope_id_at(idx))
                        .expect("nested route recv label must exist");
                    let left_entry = worker.cursor.index();
                    let right_entry = worker
                        .route_scope_offer_entry_index(nested_scope)
                        .expect("nested route must retain an offer entry");

                    worker
                        .test_commit_route_arm(0, outer_scope, 1)
                        .expect("select outer right arm");
                    worker.set_cursor_index(right_entry);
                    RouteFrontierMachine::new(&mut *worker)
                        .align_cursor_to_selected_scope()
                        .expect("selected nested route must become current scope");
                    worker.refresh_lane_offer_state(0);
                    worker.refresh_lane_offer_state(2);

                    let left_info = worker.route_state.lane_offer_state(0);
                    let right_info = worker.route_state.lane_offer_state(2);
                    assert_eq!(left_info.scope, outer_scope);
                    assert_eq!(state_index_to_usize(left_info.entry), left_entry);
                    assert_eq!(right_info.scope, nested_scope);
                    assert_eq!(state_index_to_usize(right_info.entry), right_entry);
                    assert!(
                        worker.cursor.max_frontier_entries() >= 2,
                        "nested split fixture must retain two compiled frontier slots"
                    );
                    let active_entries = worker.global_active_entries();
                    assert_eq!(active_entries.occupancy_mask(), 0b11);
                    let (
                        _cached_key_slots,
                        _cached_offer_lane_words,
                        _cached_binding_lane_words,
                        cached_key,
                    ) = copied_frontier_observation_key_storage(
                        RouteFrontierMachine::frontier_observation_key(
                            &worker,
                            ScopeId::none(),
                            false,
                        ),
                        worker.cursor.max_frontier_entries(),
                        worker.cursor.logical_lane_count(),
                    );
                    let (_cached_observed_slots, mut cached_observed_entries) =
                        observed_entry_set_storage(worker.cursor.max_frontier_entries());
                    for entry_idx in [left_entry, right_entry] {
                        let entry_state = worker
                            .offer_entry_state_snapshot(entry_idx)
                            .expect("offer entry state snapshot");
                        let observed = worker
                            .recompute_offer_entry_observed_state_non_consuming(entry_idx)
                            .expect("cached observed state");
                        let (observed_bit, inserted) = cached_observed_entries
                            .insert_entry(entry_idx)
                            .expect("insert cached observed entry");
                        assert!(inserted);
                        cached_observed_entries.observe_with_frontier_mask(
                            observed_bit,
                            observed,
                            worker.offer_entry_frontier_mask(entry_idx, entry_state),
                        );
                    }
                    let left_bit = cached_observed_entries.entry_bit(left_entry);
                    let right_bit = cached_observed_entries.entry_bit(right_entry);
                    assert_eq!(left_bit, 1u8 << 0);
                    assert_eq!(right_bit, 1u8 << 1);
                    let cached_left_ready = cached_observed_entries.ready_mask & left_bit;
                    let cached_left_progress = cached_observed_entries.progress_mask & left_bit;
                    let cached_right_ready = cached_observed_entries.ready_mask & right_bit;
                    let cached_right_progress = cached_observed_entries.progress_mask & right_bit;
                    let inner_left_data_frame =
                        frame_label_for_cursor_label(&worker.cursor, INNER_LEFT_DATA_LABEL);

                    assert!(worker.binding_inbox.push_back(
                        2,
                        crate::binding::IngressEvidence {
                            frame_label: FrameLabel::new(inner_left_data_frame),
                            channel: crate::binding::Channel::new(7),
                            instance: 7,
                            has_fin: false,
                        },
                    ));
                    let observation_key = RouteFrontierMachine::frontier_observation_key(
                        &worker,
                        ScopeId::none(),
                        false,
                    );
                    let changed_slot_mask = worker
                        .cached_frontier_changed_entry_slot_mask(
                            ScopeId::none(),
                            false,
                            observation_key,
                            cached_key,
                        )
                        .expect("same active frontier must stay structurally reusable");
                    let expected_right = worker
                        .recompute_offer_entry_observed_state_non_consuming(right_entry)
                        .expect("expected right observed state");

                    assert_eq!(
                        changed_slot_mask, right_bit,
                        "lane-2 binding changes must invalidate only the secondary frontier slot"
                    );

                    let refreshed = worker
                        .refresh_frontier_observed_entries_from_cache(
                            ScopeId::none(),
                            false,
                            active_entries,
                            observation_key,
                            cached_key,
                            cached_observed_entries,
                        )
                        .expect("same active frontier should refresh changed entry slots in place");

                    assert_eq!(refreshed.entry_bit(left_entry), left_bit);
                    assert_eq!(refreshed.entry_bit(right_entry), right_bit);
                    assert_eq!(
                        refreshed.ready_mask & left_bit,
                        cached_left_ready,
                        "lane-2 updates must not rewrite slot 0 readiness"
                    );
                    assert_eq!(
                        refreshed.progress_mask & left_bit,
                        cached_left_progress,
                        "lane-2 updates must not rewrite slot 0 progress"
                    );
                    assert_eq!(
                        refreshed.ready_mask & right_bit != 0,
                        (expected_right.flags & OfferEntryObservedState::FLAG_READY) != 0
                    );
                    assert_eq!(
                        refreshed.progress_mask & right_bit != 0,
                        expected_right.has_progress_evidence()
                    );
                    assert!(
                        refreshed.ready_mask & right_bit != cached_right_ready
                            || refreshed.progress_mask & right_bit != cached_right_progress,
                        "slot 1 must refresh at least one observed bit from the changed lane-2 binding state"
                    );
                });
            });
        },
    );
}

#[test]
fn offer_entry_reentry_prefers_first_ready_lane_candidate() {
    let current_scope = ScopeId::generic(11);
    let current_parallel_root = ScopeId::generic(7);
    let mut ready_entry_idx = None;
    let mut any_entry_idx = None;
    record_offer_entry_reentry_candidate(
        current_scope,
        3,
        current_parallel_root,
        FrontierCandidate {
            scope_id: ScopeId::generic(20),
            entry_idx: 9,
            parallel_root: current_parallel_root,
            frontier: FrontierKind::Parallel,
            flags: FrontierCandidate::pack_flags(false, false, false, false),
        },
        &mut ready_entry_idx,
        &mut any_entry_idx,
    );
    record_offer_entry_reentry_candidate(
        current_scope,
        3,
        current_parallel_root,
        FrontierCandidate {
            scope_id: ScopeId::generic(21),
            entry_idx: 10,
            parallel_root: current_parallel_root,
            frontier: FrontierKind::Parallel,
            flags: FrontierCandidate::pack_flags(false, false, true, true),
        },
        &mut ready_entry_idx,
        &mut any_entry_idx,
    );
    record_offer_entry_reentry_candidate(
        current_scope,
        3,
        current_parallel_root,
        FrontierCandidate {
            scope_id: ScopeId::generic(20),
            entry_idx: 9,
            parallel_root: current_parallel_root,
            frontier: FrontierKind::Parallel,
            flags: FrontierCandidate::pack_flags(false, false, true, true),
        },
        &mut ready_entry_idx,
        &mut any_entry_idx,
    );

    assert_eq!(any_entry_idx, Some(9));
    assert_eq!(ready_entry_idx, Some(10));
}

#[test]
fn current_controller_without_evidence_yields_to_progress_sibling() {
    assert!(!current_entry_is_candidate(true, true, false, 1, true,));
}

#[test]
fn current_controller_without_evidence_keeps_priority_without_progress_sibling() {
    assert!(current_entry_is_candidate(true, true, false, 1, false,));
}

#[test]
fn current_controller_without_alternative_keeps_priority() {
    assert!(current_entry_is_candidate(true, true, false, 0, true,));
}

#[test]
fn current_controller_with_evidence_keeps_priority() {
    assert!(current_entry_is_candidate(true, true, true, 1, true,));
}

#[test]
fn controller_candidate_with_no_evidence_stays_blocked_when_current_has_offer_lanes() {
    assert!(!controller_candidate_ready(true, 10, 7, false,));
}

#[test]
fn controller_candidate_without_progress_stays_blocked_in_passive_frontier() {
    assert!(!controller_candidate_ready(true, 10, 7, false,));
}

#[test]
fn passive_current_is_suppressed_only_by_controller_progress_sibling() {
    assert!(should_suppress_current_passive_without_evidence(
        FrontierKind::PassiveObserver,
        false,
        false,
        true,
    ));
    assert!(!should_suppress_current_passive_without_evidence(
        FrontierKind::PassiveObserver,
        false,
        false,
        false,
    ));
}

#[test]
fn evidence_less_non_current_candidate_requires_progress_or_unrunnable_current() {
    assert!(!candidate_participates_in_frontier_arbitration(
        10, 7, false, false,
    ));
    assert!(candidate_participates_in_frontier_arbitration(
        10, 7, false, true,
    ));
}

#[test]
fn passive_recv_cursor_is_not_progress_evidence_for_sibling_preempt() {
    assert!(!candidate_has_progress_evidence(false, false, false));
    assert!(candidate_has_progress_evidence(true, false, false));
    assert!(candidate_has_progress_evidence(false, true, false));
    assert!(candidate_has_progress_evidence(false, false, true));
}

fn has_progress_controller_sibling(
    snapshot: FrontierSnapshot,
    scope_id: ScopeId,
    entry_idx: usize,
) -> bool {
    snapshot
        .select_exhausted_controller_candidate(empty_frontier_visit_set())
        .is_some_and(|candidate| {
            candidate.scope_id != scope_id || candidate.entry_idx as usize != entry_idx
        })
}

#[test]
fn passive_frontier_detects_progress_controller_sibling() {
    let current_scope = ScopeId::generic(71);
    let controller_scope = ScopeId::generic(72);
    let mut candidates = frontier_candidates::<3>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 63,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: controller_scope,
        entry_idx: 53,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, false, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        63,
        ScopeId::none(),
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    assert!(has_progress_controller_sibling(snapshot, current_scope, 63));
}

#[test]
fn passive_frontier_ignores_controller_without_progress_evidence() {
    let current_scope = ScopeId::generic(171);
    let controller_scope = ScopeId::generic(172);
    let mut candidates = frontier_candidates::<3>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 63,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: controller_scope,
        entry_idx: 53,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, false, false, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        63,
        ScopeId::none(),
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    assert!(!has_progress_controller_sibling(
        snapshot,
        current_scope,
        63
    ));
}

#[test]
fn passive_frontier_ignores_non_controller_sibling_for_controller_preemption() {
    let current_scope = ScopeId::generic(81);
    let sibling_scope = ScopeId::generic(82);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 63,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 59,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        63,
        ScopeId::none(),
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    assert!(!has_progress_controller_sibling(
        snapshot,
        current_scope,
        63
    ));
}

#[test]
fn frontier_yield_ping_pong_is_bounded() {
    let mut visited_slots = frontier_visit_slots::<2>();
    let mut visited = frontier_visit_set_fixture(&mut visited_slots);
    let scope_a = ScopeId::generic(31);
    let scope_b = ScopeId::generic(32);
    visited.record(scope_a);
    visited.record(scope_b);
    visited.record(scope_a);
    assert!(visited.contains(scope_a));
    assert!(visited.contains(scope_b));
    assert_eq!(visited.len, 2);
}

#[test]
fn route_defer_yields_to_sibling_scope() {
    let current_scope = ScopeId::generic(41);
    let sibling_scope = ScopeId::generic(42);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 10,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 12,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: FrontierCandidate::pack_flags(true, true, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        10,
        ScopeId::none(),
        FrontierKind::Route,
        &mut candidates,
        2,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("route frontier must yield to progress sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Route);
}

#[test]
fn loop_defer_yields_to_sibling_scope() {
    let current_scope = ScopeId::generic(51);
    let sibling_scope = ScopeId::generic(52);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 20,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 24,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, true, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        20,
        ScopeId::none(),
        FrontierKind::Loop,
        &mut candidates,
        2,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("loop frontier must yield to progress sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Loop);
}

#[test]
fn defer_yields_across_frontier_in_same_parallel_root() {
    let root = ScopeId::generic(55);
    let current_scope = ScopeId::generic(56);
    let sibling_scope = ScopeId::generic(57);
    let mut candidates = frontier_candidates::<3>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 20,
        parallel_root: root,
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 24,
        parallel_root: root,
        frontier: FrontierKind::Route,
        flags: FrontierCandidate::pack_flags(true, true, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        20,
        root,
        FrontierKind::Loop,
        &mut candidates,
        2,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("defer must yield to progress sibling in same parallel root");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_eq!(picked.frontier, FrontierKind::Route);
}

#[test]
fn parallel_frontier_prefers_ready_lane_before_phase_join() {
    let current_scope = ScopeId::generic(61);
    let root = ScopeId::generic(60);
    let ready_scope = ScopeId::generic(62);
    let mut candidates = frontier_candidates::<3>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 30,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        flags: FrontierCandidate::pack_flags(true, true, false, false),
    };
    candidates[1] = FrontierCandidate {
        scope_id: ScopeId::generic(63),
        entry_idx: 31,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        flags: FrontierCandidate::pack_flags(false, false, false, false),
    };
    candidates[2] = FrontierCandidate {
        scope_id: ready_scope,
        entry_idx: 32,
        parallel_root: root,
        frontier: FrontierKind::Parallel,
        flags: FrontierCandidate::pack_flags(false, false, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        30,
        root,
        FrontierKind::Parallel,
        &mut candidates,
        3,
    );
    let picked = snapshot
        .select_yield_candidate(empty_frontier_visit_set())
        .expect("parallel frontier must choose progress sibling");
    assert_eq!(picked.scope_id, ready_scope);
    assert_eq!(picked.entry_idx, 32);
}

#[test]
fn passive_observer_defer_follow_is_progressive() {
    let current_scope = ScopeId::generic(71);
    let sibling_scope = ScopeId::generic(72);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 40,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 44,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, true, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        40,
        ScopeId::none(),
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    let mut visited_slots = frontier_visit_slots::<1>();
    let mut visited = frontier_visit_set_fixture(&mut visited_slots);
    visited.record(current_scope);
    let picked = snapshot
        .select_yield_candidate(visited)
        .expect("passive observer defer must progress to sibling");
    assert_eq!(picked.scope_id, sibling_scope);
    assert_ne!(picked.scope_id, current_scope);
}

#[test]
fn passive_observer_defer_stops_without_progress_evidence() {
    let root = ScopeId::generic(73);
    let current_scope = ScopeId::generic(74);
    let sibling_scope = ScopeId::generic(75);
    let mut candidates = frontier_candidates::<2>();
    candidates[0] = FrontierCandidate {
        scope_id: current_scope,
        entry_idx: 50,
        parallel_root: root,
        frontier: FrontierKind::PassiveObserver,
        flags: FrontierCandidate::pack_flags(false, false, false, true),
    };
    candidates[1] = FrontierCandidate {
        scope_id: sibling_scope,
        entry_idx: 53,
        parallel_root: root,
        frontier: FrontierKind::Loop,
        flags: FrontierCandidate::pack_flags(true, false, false, true),
    };
    let snapshot = frontier_snapshot_fixture(
        current_scope,
        50,
        root,
        FrontierKind::PassiveObserver,
        &mut candidates,
        2,
    );
    let mut visited_slots = frontier_visit_slots::<1>();
    let mut visited = frontier_visit_set_fixture(&mut visited_slots);
    visited.record(current_scope);
    assert_eq!(snapshot.select_yield_candidate(visited), None);
}

#[test]
fn controller_local_ready_is_not_progress_evidence_for_sibling_preempt() {
    assert!(
        current_entry_is_candidate(true, true, false, 1, false),
        "controller local-ready only must not preempt without progress evidence"
    );
}

#[test]
fn frontier_arbitration_is_uniform_across_route_loop_parallel_observer() {
    let cases = [
        (ScopeId::none(), FrontierKind::Route),
        (ScopeId::none(), FrontierKind::Loop),
        (ScopeId::generic(101), FrontierKind::Parallel),
        (ScopeId::none(), FrontierKind::PassiveObserver),
    ];
    let mut idx = 0usize;
    while idx < cases.len() {
        let (parallel_root, frontier) = cases[idx];
        let current_scope = ScopeId::generic((110 + idx) as u16);
        let sibling_scope = ScopeId::generic((120 + idx) as u16);
        let mut candidates = frontier_candidates::<2>();
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: (70 + idx) as u16,
            parallel_root,
            frontier,
            flags: FrontierCandidate::pack_flags(false, false, false, true),
        };
        candidates[1] = FrontierCandidate {
            scope_id: sibling_scope,
            entry_idx: (80 + idx) as u16,
            parallel_root,
            frontier,
            flags: FrontierCandidate::pack_flags(true, true, true, true),
        };
        let snapshot = frontier_snapshot_fixture(
            current_scope,
            70 + idx,
            parallel_root,
            frontier,
            &mut candidates,
            2,
        );
        let picked = snapshot
            .select_yield_candidate(empty_frontier_visit_set())
            .expect("uniform frontier defer must pick progress-evidence-bearing sibling");
        assert_eq!(picked.scope_id, sibling_scope);
        assert_eq!(picked.frontier, frontier);
        idx += 1;
    }
}

#[test]
fn dynamic_route_ignores_hint_evidence_for_authority() {
    run_offer_regression_test("dynamic_route_ignores_hint_evidence_for_authority", || {
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_LEFT_DATA_FRAME);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(904);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_CONTROLLER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }

                    let scope = {
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");
                        assert!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    scope,
                                    0,
                                    HINT_LEFT_DATA_FRAME
                                )
                                .is_none(),
                            "dynamic route arm authority must not depend on first-recv dispatch"
                        );
                        scope
                    };

                    let worker = worker_slot.borrow_mut();
                    let mut cx = Context::from_waker(noop_waker_ref());
                    let branch = {
                        let mut offer = pin!(cursor_offer(worker));
                        let first_poll = offer.as_mut().poll(&mut cx);
                        let mut branch = match first_poll {
                            Poll::Ready(Ok(next_branch)) => Some(next_branch),
                            Poll::Ready(Err(err)) => {
                                panic!("offer should not fail before decision: {err:?}")
                            }
                            Poll::Pending => None,
                        };
                        {
                            let controller = controller_slot.borrow_mut();
                            controller.port_for_lane(0).record_route_decision(scope, 0);
                        }
                        if branch.is_none() {
                            let mut attempts = 0usize;
                            while attempts < 4 {
                                match offer.as_mut().poll(&mut cx) {
                                    Poll::Ready(Ok(next_branch)) => {
                                        branch = Some(next_branch);
                                        break;
                                    }
                                    Poll::Ready(Err(err)) => {
                                        panic!(
                                            "offer should resolve via authoritative decision: {err:?}"
                                        );
                                    }
                                    Poll::Pending => {}
                                }
                                attempts += 1;
                            }
                        }
                        branch.expect("offer should become ready after authoritative decision")
                    };
                    assert_eq!(
                        branch_label(&branch),
                        HINT_LEFT_DATA_LABEL,
                        "resolved branch must follow authoritative arm, not hint-derived ACK"
                    );
                    drop(branch);
                    assert!(
                        worker.peek_scope_ack(scope).is_some(),
                        "dropping a preview branch must not consume authoritative ACK evidence"
                    );
                    assert!(
                        worker.scope_has_ready_arm_evidence(scope),
                        "dropping a preview branch must not clear ready-arm evidence"
                    );
                    assert!(
                        worker.selected_arm_for_scope(scope).is_none(),
                        "dropping a preview branch must not commit route progress"
                    );
                });
            });
        });
    });
}

#[test]
fn select_scope_prepass_keeps_pending_scope_evidence_non_consuming() {
    run_offer_regression_test(
        "select_scope_prepass_keeps_pending_scope_evidence_non_consuming",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_LEFT_DATA_FRAME);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9041);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        {
                            let controller = controller_slot.borrow_mut();
                            controller.port_for_lane(0).record_route_decision(scope, 0);
                        }
                        let frame_label_meta =
                            endpoint_scope_frame_label_meta(worker, scope, ScopeLoopMeta::EMPTY);
                        worker.refresh_lane_offer_state(0);
                        let entry_idx =
                            state_index_to_usize(worker.route_state.lane_offer_state(0).entry);
                        let entry_state = worker
                            .offer_entry_state_snapshot(entry_idx)
                            .expect("offer entry state snapshot");
                        let (_binding_ready, has_ack, has_ready_arm_evidence) = worker
                            .preview_offer_entry_evidence_non_consuming(entry_idx, entry_state);
                        assert!(has_ack, "prepass may observe pending ACK authority");
                        assert!(
                            !has_ready_arm_evidence,
                            "pending demux hints must not be promoted to ready-arm evidence during prepass"
                        );

                        RouteFrontierMachine::new(&mut *worker)
                            .align_cursor_to_selected_scope()
                            .expect("scope prepass should succeed without consuming evidence");
                        assert!(
                            worker.peek_scope_ack(scope).is_none(),
                            "prepass must not consume route ACK authority into scope evidence"
                        );
                        assert!(
                            worker.peek_scope_frame_hint(scope).is_none(),
                            "prepass must not consume route hints into scope evidence"
                        );
                        assert_eq!(
                            worker.scope_ready_arm_mask(scope),
                            0,
                            "prepass must not synthesize ready-arm evidence before selected-scope ingest"
                        );
                        assert_eq!(
                            worker.port_for_lane(0).peek_route_decision(scope, 1),
                            Some(0),
                            "authoritative route ACK must remain pending on the port after prepass"
                        );
                        assert!(
                            worker
                                .port_for_lane(0)
                                .has_route_hint_matching(|label| label == HINT_LEFT_DATA_FRAME),
                            "matching route hint must remain queued on the port after prepass"
                        );

                        with_lane_set_view(&[0], |offer_lanes| {
                            worker.ingest_scope_evidence_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                true,
                                frame_label_meta,
                            );
                        });

                        assert_eq!(
                            worker
                                .peek_scope_ack(scope)
                                .map(|token| token.arm().as_u8()),
                            Some(0),
                            "selected-scope ingest must materialize the pending ACK exactly once"
                        );
                        assert!(
                            worker.scope_has_ready_arm_evidence(scope),
                            "selected-scope ingest must materialize ready-arm evidence from the pending hint"
                        );
                        assert_eq!(
                            worker.port_for_lane(0).peek_route_decision(scope, 1),
                            None,
                            "selected-scope ingest must consume the pending ACK from the port"
                        );
                        assert!(
                            !worker
                                .port_for_lane(0)
                                .has_route_hint_matching(|label| label == HINT_LEFT_DATA_FRAME),
                            "selected-scope ingest must consume the pending hint from the port"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn preview_offer_entry_evidence_skips_binding_probe_when_ack_already_progresses_scope() {
    run_offer_regression_test(
        "preview_offer_entry_evidence_skips_binding_probe_when_ack_already_progresses_scope",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9042);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let scope = {
                            let worker = worker_slot.borrow_mut();
                            let scope = worker.cursor.node_scope_id();
                            assert!(!scope.is_none(), "worker must start at route scope");
                            scope
                        };

                        {
                            let controller = controller_slot.borrow_mut();
                            controller.port_for_lane(0).record_route_decision(scope, 0);
                        }
                        let worker = worker_slot.borrow_mut();
                        worker.refresh_lane_offer_state(0);
                        let entry_idx =
                            state_index_to_usize(worker.route_state.lane_offer_state(0).entry);
                        let entry_state = worker
                            .offer_entry_state_snapshot(entry_idx)
                            .expect("offer entry state snapshot");
                        let (binding_ready, has_ack, has_ready_arm_evidence) = worker
                            .preview_offer_entry_evidence_non_consuming(entry_idx, entry_state);

                        assert!(!binding_ready, "empty binding must remain not-ready");
                        assert!(has_ack, "pending route decision must count as ACK evidence");
                        assert!(
                            !has_ready_arm_evidence,
                            "ACK-only preview must not synthesize ready-arm evidence"
                        );
                        assert_eq!(
                            worker.binding.poll_count(),
                            0,
                            "binding probe must be skipped when ACK already supplies progress evidence"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn preview_offer_entry_evidence_defers_binding_poll_until_selected_scope() {
    run_offer_regression_test(
        "preview_offer_entry_evidence_defers_binding_poll_until_selected_scope",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9043);
                        let evidence = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(3),
                        };
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_WORKER_PROGRAM(),
                                    TestBinding::with_incoming(&[evidence]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        worker.refresh_lane_offer_state(0);
                        let entry_idx =
                            state_index_to_usize(worker.route_state.lane_offer_state(0).entry);
                        let entry_state = worker
                            .offer_entry_state_snapshot(entry_idx)
                            .expect("offer entry state snapshot");
                        let (binding_ready, has_ack, has_ready_arm_evidence) = worker
                            .preview_offer_entry_evidence_non_consuming(entry_idx, entry_state);

                        assert!(
                            !binding_ready,
                            "prepass must not probe binding to synthesize ready state"
                        );
                        assert!(
                            !has_ack,
                            "evidence-only prepass must not synthesize ACK authority"
                        );
                        assert!(
                            !has_ready_arm_evidence,
                            "evidence-only prepass must not synthesize ready-arm evidence"
                        );
                        assert_eq!(
                            worker.binding.poll_count(),
                            0,
                            "prepass must not touch binding before selected-scope demux"
                        );

                        let picked = worker.poll_binding_for_offer(
                            scope,
                            entry_state.lane_idx as usize,
                            entry_state.frame_label_meta,
                            entry_state.materialization_meta,
                        );
                        assert_eq!(
                            picked,
                            Some((0, evidence)),
                            "selected-scope poll must still resolve the deferred binding evidence"
                        );
                        assert_eq!(
                            worker.binding.poll_count(),
                            1,
                            "binding must be polled exactly once after scope selection"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn hint_or_evidence_never_writes_ack_authority() {
    run_offer_regression_test("hint_or_evidence_never_writes_ack_authority", || {
        offer_fixture!(2048, clock, config);
        type WorkerEndpoint = CursorEndpoint<
            'static,
            1,
            HintOnlyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
            4,
            crate::control::cap::mint::MintConfig,
            TestBinding,
        >;
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_LEFT_DATA_FRAME);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(905);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_CONTROLLER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                TestBinding::with_incoming(&[IngressEvidence {
                                    frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                                    instance: 0,
                                    has_fin: false,
                                    channel: Channel::new(1),
                                }]),
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    let frame_label_meta =
                        endpoint_scope_frame_label_meta(worker, scope, ScopeLoopMeta::EMPTY);

                    with_lane_set_view(&[0], |offer_lanes| {
                        worker.ingest_scope_evidence_for_offer_lanes(
                            scope,
                            0,
                            offer_lanes,
                            true,
                            frame_label_meta,
                        );
                    });
                    assert!(
                        worker.peek_scope_ack(scope).is_none(),
                        "dynamic hint ingest must not mint ack authority"
                    );

                    let mut binding_evidence = None;
                    worker.cache_binding_evidence_for_offer(
                        scope,
                        0,
                        frame_label_meta,
                        worker.offer_scope_materialization_meta(scope, 0),
                        &mut binding_evidence,
                    );
                    assert!(
                        binding_evidence.is_some(),
                        "binding evidence should still be staged for decode/demux"
                    );
                    let evidence = binding_evidence.expect("binding evidence should be available");
                    worker.ingest_binding_scope_evidence(
                        scope,
                        evidence.lane(),
                        evidence.frame_label(),
                        true,
                        frame_label_meta,
                    );
                    assert!(
                        worker.peek_scope_ack(scope).is_none(),
                        "evidence must not mint ack authority for dynamic route"
                    );
                    assert_eq!(
                        worker.poll_arm_from_ready_mask(scope),
                        None,
                        "dynamic binding evidence must not materialize Poll authority"
                    );
                });
            });
        });
    });
}

#[test]
fn poll_binding_for_offer_prefers_exact_label_for_ack_arm() {
    run_offer_regression_test(
        "poll_binding_for_offer_prefers_exact_label_for_ack_arm",
        || {
            offer_fixture!(2048, clock, config);
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                TestBinding,
            >;
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9044);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::with_incoming(&[
                                        IngressEvidence {
                                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                                            instance: 7,
                                            has_fin: false,
                                            channel: Channel::new(3),
                                        },
                                        IngressEvidence {
                                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                                            instance: 9,
                                            has_fin: false,
                                            channel: Channel::new(5),
                                        },
                                    ]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        worker.refresh_lane_offer_state(0);
                        let entry_idx =
                            state_index_to_usize(worker.route_state.lane_offer_state(0).entry);
                        let entry_state = worker
                            .offer_entry_state_snapshot(entry_idx)
                            .expect("offer entry state snapshot");
                        let frame_label_meta = ScopeFrameLabelMeta {
                            controller_frame_labels: [HINT_LEFT_DATA_FRAME, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        assert_eq!(
                            frame_label_meta.preferred_binding_frame_label(Some(1)),
                            Some(HINT_RIGHT_DATA_FRAME)
                        );
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
                        );

                        let picked = worker.poll_binding_for_offer(
                            scope,
                            entry_state.lane_idx as usize,
                            frame_label_meta,
                            entry_state.materialization_meta,
                        );
                        assert_eq!(
                            picked
                                .map(|(lane_idx, evidence)| (lane_idx, evidence.frame_label.raw())),
                            Some((0, HINT_RIGHT_DATA_FRAME)),
                            "authoritative arm should narrow binding demux to the exact matching label"
                        );
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_LEFT_DATA_FRAME,
                        );
                        assert_eq!(
                            deferred.map(|evidence| evidence.frame_label.raw()),
                            Some(HINT_LEFT_DATA_FRAME),
                            "non-authoritative arm evidence must remain buffered"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_prefers_buffered_matching_lane_before_empty_poll_lane() {
    run_offer_regression_test(
        "poll_binding_for_offer_prefers_buffered_matching_lane_before_empty_poll_lane",
        || {
            offer_fixture!(2048, clock, config);
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                TestBinding,
            >;
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9046);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let buffered = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        worker.binding_inbox.put_back(2, buffered);

                        let frame_label_meta = ScopeFrameLabelMeta {
                            controller_frame_labels: [0, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::EMPTY,
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::EMPTY,
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
                        );

                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                frame_label_meta,
                                worker.offer_scope_materialization_meta(scope, 0),
                            )
                        });
                        assert_eq!(
                            picked,
                            Some((2, buffered)),
                            "buffered matching lane should be selected before probing empty poll lane"
                        );
                        assert_eq!(
                            worker.binding.poll_count(),
                            0,
                            "buffered cross-lane hit should not poll unrelated empty lanes first"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_skips_non_demux_lanes_for_authoritative_arm() {
    run_offer_regression_test(
        "poll_binding_for_offer_skips_non_demux_lanes_for_authoritative_arm",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9047);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        let loop_mismatch = IngressEvidence {
                            frame_label: FrameLabel::new(TEST_LOOP_CONTINUE_FRAME),
                            instance: 1,
                            has_fin: false,
                            channel: Channel::new(7),
                        };
                        worker.binding_inbox.put_back(0, loop_mismatch);
                        worker.binding_inbox.put_back(2, matching);

                        let extra_frame = 99;
                        let frame_label_meta = ScopeFrameLabelMeta {
                            recv_frame_label: extra_frame,
                            recv_arm: 1,
                            controller_frame_labels: [0, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::EMPTY,
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(extra_frame),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::EMPTY,
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(extra_frame),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
                                | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        let materialization_meta =
                            worker.compute_scope_arm_materialization_meta(scope);
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
                        );

                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                frame_label_meta,
                                materialization_meta,
                            )
                        });
                        assert_eq!(picked, Some((2, matching)));
                        assert_eq!(
                            worker.binding_inbox.take_matching_or_poll(
                                &mut worker.binding,
                                0,
                                TEST_LOOP_CONTINUE_FRAME,
                            ),
                            Some(loop_mismatch),
                            "authoritative arm demux must not scan unrelated loop-control lane"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_prefers_authoritative_arm_frame_label_mask_when_non_singleton() {
    run_offer_regression_test(
        "poll_binding_for_offer_prefers_authoritative_arm_frame_label_mask_when_non_singleton",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9045);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::with_incoming(&[
                                        IngressEvidence {
                                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                                            instance: 9,
                                            has_fin: false,
                                            channel: Channel::new(5),
                                        },
                                        IngressEvidence {
                                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                                            instance: 7,
                                            has_fin: false,
                                            channel: Channel::new(3),
                                        },
                                    ]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        worker.refresh_lane_offer_state(0);
                        let entry_idx =
                            state_index_to_usize(worker.route_state.lane_offer_state(0).entry);
                        let entry_state = worker
                            .offer_entry_state_snapshot(entry_idx)
                            .expect("offer entry state snapshot");
                        let extra_frame = 99;
                        let frame_label_meta = ScopeFrameLabelMeta {
                            recv_frame_label: extra_frame,
                            recv_arm: 0,
                            controller_frame_labels: [HINT_LEFT_DATA_FRAME, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(extra_frame),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(extra_frame),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
                                | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        assert_eq!(
                            frame_label_meta.preferred_binding_frame_label(Some(0)),
                            None
                        );
                        assert_eq!(
                            frame_label_meta.preferred_binding_frame_label_mask(Some(0)),
                            FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                | FrameLabelMask::from_frame_label(extra_frame)
                        );
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(0).expect("binary route arm")),
                        );

                        let picked = worker.poll_binding_for_offer(
                            scope,
                            entry_state.lane_idx as usize,
                            frame_label_meta,
                            entry_state.materialization_meta,
                        );
                        assert_eq!(
                            picked
                                .map(|(lane_idx, evidence)| (lane_idx, evidence.frame_label.raw())),
                            Some((0, HINT_LEFT_DATA_FRAME)),
                            "authoritative arm mask should skip buffered labels from the other arm"
                        );
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_RIGHT_DATA_FRAME,
                        );
                        assert_eq!(
                            deferred.map(|evidence| evidence.frame_label.raw()),
                            Some(HINT_RIGHT_DATA_FRAME),
                            "non-authoritative arm evidence must remain buffered after mask match"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_uses_frame_label_mask_to_skip_other_arm_lanes_without_authority() {
    run_offer_regression_test(
        "poll_binding_for_offer_uses_frame_label_mask_to_skip_other_arm_lanes_without_authority",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9048);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        let loop_mismatch = IngressEvidence {
                            frame_label: FrameLabel::new(TEST_LOOP_CONTINUE_FRAME),
                            instance: 1,
                            has_fin: false,
                            channel: Channel::new(7),
                        };
                        worker.binding_inbox.put_back(0, loop_mismatch);
                        worker.binding_inbox.put_back(2, matching);

                        let frame_label_meta = ScopeFrameLabelMeta {
                            controller_frame_labels: [HINT_LEFT_DATA_FRAME, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        let materialization_meta =
                            worker.compute_scope_arm_materialization_meta(scope);
                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                frame_label_meta,
                                materialization_meta,
                            )
                        });
                        assert_eq!(picked, Some((2, matching)));
                        assert_eq!(
                            worker.binding_inbox.take_matching_or_poll(
                                &mut worker.binding,
                                0,
                                TEST_LOOP_CONTINUE_FRAME,
                            ),
                            Some(loop_mismatch),
                            "no-authority demux should still restrict scans to lanes implied by the label mask"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_buffered_match_skips_drop_only_preferred_lane_for_non_singleton_mask() {
    run_offer_regression_test(
        "poll_binding_for_offer_buffered_match_skips_drop_only_preferred_lane_for_non_singleton_mask",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9050);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }

                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        let loop_mismatch = IngressEvidence {
                            frame_label: FrameLabel::new(TEST_LOOP_CONTINUE_FRAME),
                            instance: 1,
                            has_fin: false,
                            channel: Channel::new(7),
                        };
                        worker.binding_inbox.put_back(0, loop_mismatch);
                        worker.binding_inbox.put_back(2, matching);

                        let frame_label_meta = ScopeFrameLabelMeta {
                            controller_frame_labels: [HINT_LEFT_DATA_FRAME, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        let materialization_meta =
                            worker.compute_scope_arm_materialization_meta(scope);
                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                frame_label_meta,
                                materialization_meta,
                            )
                        });
                        assert_eq!(picked, Some((2, matching)));
                        assert_eq!(
                            worker.binding_inbox.take_matching_or_poll(
                                &mut worker.binding,
                                0,
                                TEST_LOOP_CONTINUE_FRAME,
                            ),
                            Some(loop_mismatch),
                            "buffered matching lane should win before scanning drop-only preferred lane"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_polls_only_selected_lane_for_unbuffered_generic_mask() {
    run_offer_regression_test(
        "poll_binding_for_offer_polls_only_selected_lane_for_unbuffered_generic_mask",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintLaneAwareWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9052);
                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_WORKER_PROGRAM(),
                                    LaneAwareTestBinding::with_lane_incoming(&[(2, matching)]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let frame_label_meta = ScopeFrameLabelMeta {
                            controller_frame_labels: [HINT_LEFT_DATA_FRAME, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        let materialization_meta =
                            worker.compute_scope_arm_materialization_meta(scope);

                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                frame_label_meta,
                                materialization_meta,
                            )
                        });
                        assert_eq!(
                            picked, None,
                            "generic mask path must not probe unbuffered cross-lane bindings before the selected lane"
                        );
                        assert_eq!(worker.binding.poll_count_for_lane(0), 1);
                        assert_eq!(worker.binding.poll_count_for_lane(2), 0);

                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                2,
                                offer_lanes,
                                frame_label_meta,
                                materialization_meta,
                            )
                        });
                        assert_eq!(picked, Some((2, matching)));
                        assert_eq!(worker.binding.poll_count_for_lane(2), 1);
                    });
                });
            });
        },
    );
}

#[test]
fn poll_binding_for_offer_polls_authoritative_demux_lane_when_current_lane_is_excluded() {
    run_offer_regression_test(
        "poll_binding_for_offer_polls_authoritative_demux_lane_when_current_lane_is_excluded",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintLaneAwareWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9053);
                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 11,
                            has_fin: false,
                            channel: Channel::new(6),
                        };
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_WORKER_PROGRAM(),
                                    LaneAwareTestBinding::with_lane_incoming(&[(2, matching)]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
                        );
                        let frame_label_meta = ScopeFrameLabelMeta {
                            controller_frame_labels: [0, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::EMPTY,
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::EMPTY,
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        let materialization_meta =
                            worker.compute_scope_arm_materialization_meta(scope);

                        let picked = with_lane_set_view(&[0, 2], |offer_lanes| {
                            worker.poll_binding_for_offer_lanes(
                                scope,
                                0,
                                offer_lanes,
                                frame_label_meta,
                                materialization_meta,
                            )
                        });
                        assert_eq!(picked, Some((2, matching)));
                        assert_eq!(worker.binding.poll_count_for_lane(0), 0);
                        assert_eq!(worker.binding.poll_count_for_lane(2), 1);
                    });
                });
            });
        },
    );
}

#[test]
fn record_route_decision_for_scope_lanes_refreshes_sibling_frontier_cache() {
    run_offer_regression_test(
        "record_route_decision_for_scope_lanes_refreshes_sibling_frontier_cache",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintLaneAwareWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9054);
                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 12,
                            has_fin: false,
                            channel: Channel::new(6),
                        };
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_SPLIT_WORKER_PROGRAM(),
                                    LaneAwareTestBinding::with_lane_incoming(&[(2, matching)]),
                                )
                                .expect("attach worker endpoint");
                        }

                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let mut cx = Context::from_waker(noop_waker_ref());
                        {
                            let mut offer = pin!(cursor_offer(worker));
                            assert!(
                                matches!(offer.as_mut().poll(&mut cx), Poll::Pending),
                                "unresolved split-lane route must cache a pending frontier observation before the decision arrives"
                            );
                        }

                        worker.record_route_decision_for_scope_lanes(scope, 1, 0);
                        worker.record_scope_ack(
                            scope,
                            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
                        );

                        let mut offer = pin!(cursor_offer(worker));
                        let branch =
                            poll_ready_ok(&mut cx, offer.as_mut(), "split-lane sibling offer");
                        assert_eq!(
                            branch_label(&branch),
                            HINT_RIGHT_DATA_LABEL,
                            "broadcast route decisions must invalidate sibling-lane frontier caches immediately"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn take_binding_for_selected_arm_preserves_cached_other_arm_evidence() {
    run_offer_regression_test(
        "take_binding_for_selected_arm_preserves_cached_other_arm_evidence",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9049);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let matching = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                            instance: 9,
                            has_fin: true,
                            channel: Channel::new(5),
                        };
                        let cached_mismatch = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 7,
                            has_fin: false,
                            channel: Channel::new(3),
                        };
                        worker.binding_inbox.put_back(0, matching);
                        let extra_frame = 99;
                        let frame_label_meta = ScopeFrameLabelMeta {
                            recv_frame_label: extra_frame,
                            recv_arm: 0,
                            controller_frame_labels: [HINT_LEFT_DATA_FRAME, HINT_RIGHT_DATA_FRAME],
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(extra_frame),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(extra_frame),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            flags: ScopeFrameLabelMeta::FLAG_CURRENT_RECV_FRAME_LABEL
                                | ScopeFrameLabelMeta::FLAG_CURRENT_RECV_ARM
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM0
                                | ScopeFrameLabelMeta::FLAG_CONTROLLER_ARM1,
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        let mut binding_evidence = Some(cached_mismatch);

                        let selected = worker.take_binding_for_selected_arm(
                            0,
                            0,
                            frame_label_meta,
                            &mut binding_evidence,
                        );
                        assert_eq!(
                            selected,
                            Some(matching),
                            "selected-arm helper must preserve the matched ingress evidence exactly"
                        );
                        assert!(
                            binding_evidence.is_none(),
                            "cached mismatch should be re-buffered, not left staged"
                        );
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_RIGHT_DATA_FRAME,
                        );
                        assert_eq!(
                            deferred,
                            Some(cached_mismatch),
                            "selected-arm demux must preserve cached other-arm evidences"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn selected_arm_mask_preserves_actual_binding_evidence_label() {
    run_offer_regression_test(
        "selected_arm_mask_preserves_actual_binding_evidence_label",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9059);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let observed = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM1_SIGNAL_FRAME),
                            instance: 11,
                            has_fin: true,
                            channel: Channel::new(8),
                        };
                        let frame_label_meta = ScopeFrameLabelMeta {
                            arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(observed.frame_label.raw()),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            evidence_arm_frame_label_masks: [
                                FrameLabelMask::from_frame_label(HINT_LEFT_DATA_FRAME)
                                    | FrameLabelMask::from_frame_label(observed.frame_label.raw()),
                                FrameLabelMask::from_frame_label(HINT_RIGHT_DATA_FRAME),
                            ],
                            ..ScopeFrameLabelMeta::EMPTY
                        };
                        assert_eq!(
                            frame_label_meta.preferred_binding_frame_label(Some(0)),
                            None,
                            "fixture must exercise the non-singleton selected-arm mask path"
                        );
                        let mut binding_evidence = Some(observed);

                        let selected = worker.take_binding_for_selected_arm(
                            0,
                            0,
                            frame_label_meta,
                            &mut binding_evidence,
                        );

                        assert_eq!(
                            selected,
                            Some(observed),
                            "selected-arm demux must not rewrite the observed evidence label"
                        );
                        assert!(
                            binding_evidence.is_none(),
                            "matched evidence should be consumed by the selected branch"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn decode_rejects_when_branch_label_and_binding_evidence_label_disagree() {
    run_offer_regression_test(
        "decode_rejects_when_branch_label_and_binding_evidence_label_disagree",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9063);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_WORKER_PROGRAM(),
                                    TestBinding::with_incoming_and_payloads(&[], &[&[42]]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let selected = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 15,
                            has_fin: false,
                            channel: Channel::new(12),
                        };
                        let mismatched = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM1_SIGNAL_FRAME),
                            instance: 15,
                            has_fin: false,
                            channel: Channel::new(13),
                        };
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
                            offer_lane_idx: 0,
                            at_route_offer_entry: false,
                        };
                        assert_ne!(
                            mismatched.frame_label.raw(),
                            ENTRY_ARM0_SIGNAL_FRAME,
                            "fixture must exercise evidence label != branch recv label"
                        );
                        let resolved = ResolvedRouteDecision {
                            route_token: RouteDecisionToken::from_ack(
                                Arm::new(0).expect("binary route arm"),
                            ),
                            selected_arm: 0,
                            resolved_hint_frame_label: None,
                        };

                        let mut branch = RouteFrontierMachine::new(worker)
                            .materialize_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, selected)),
                                0,
                                0,
                                None,
                            )
                            .expect("materialize selected branch")
                            .into();
                        assert_eq!(branch_label(&branch), ENTRY_ARM0_SIGNAL_LABEL);
                        branch.binding_evidence =
                            PackedIngressEvidence::from_option(Some(mismatched));

                        let mut cx = Context::from_waker(noop_waker_ref());
                        let err = {
                            let mut decode =
                                pin!(CursorDecode::<Msg<103, u8>>::run(worker, branch));
                            match decode.as_mut().poll(&mut cx) {
                                Poll::Ready(Err(err)) => err,
                                Poll::Ready(Ok(_)) => {
                                    panic!("mismatched binding evidence label must fail closed")
                                }
                                Poll::Pending => {
                                    panic!("mismatched binding evidence label must not await I/O")
                                }
                            }
                        };
                        match err {
                            RecvError::LabelMismatch { expected, actual } => {
                                assert_eq!(expected, ENTRY_ARM0_SIGNAL_FRAME);
                                assert_eq!(actual, mismatched.frame_label.raw());
                            }
                            other => panic!("expected label mismatch, got {other:?}"),
                        }
                        assert_eq!(
                            worker.binding.last_recv_channel(),
                            None,
                            "label mismatch must be rejected before reading the evidence channel"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn materialized_branch_preserves_actual_binding_evidence_label() {
    run_offer_regression_test(
        "materialized_branch_preserves_actual_binding_evidence_label",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9060);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let observed = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 12,
                            has_fin: false,
                            channel: Channel::new(9),
                        };
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
                            offer_lane_idx: 0,
                            at_route_offer_entry: false,
                        };
                        let resolved = ResolvedRouteDecision {
                            route_token: RouteDecisionToken::from_ack(
                                Arm::new(0).expect("binary route arm"),
                            ),
                            selected_arm: 0,
                            resolved_hint_frame_label: None,
                        };

                        let branch: MaterializedRouteBranch<'_> = RouteFrontierMachine::new(worker)
                            .materialize_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, observed)),
                                0,
                                0,
                                None,
                            )
                            .expect("materialize selected branch")
                            .into();

                        assert_eq!(branch_label(&branch), ENTRY_ARM0_SIGNAL_LABEL);
                        assert_eq!(
                            branch.binding_evidence.into_option(),
                            Some(observed),
                            "materialization must keep the binding evidence label observed by demux"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn materialize_non_wire_recv_evidence_requeues_staged_transport_payload() {
    run_offer_regression_test(
        "materialize_non_wire_recv_evidence_requeues_staged_transport_payload",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, PendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(PendingControllerBindingEndpoint, controller_slot, {
                    with_offer_value_slot!(PendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport = PendingTransport::new(pending_state);
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(9063);
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        controller_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &ENTRY_CONTROLLER_PROGRAM(),
                                        TestBinding::default(),
                                    )
                                    .expect("attach controller endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        worker_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &ENTRY_WORKER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach worker endpoint");
                            }

                            let controller = controller_slot.borrow_mut();
                            let worker_borrow = worker_slot.borrow_mut();
                            core::hint::black_box(&worker_borrow);
                            let scope = controller.cursor.node_scope_id();
                            assert!(
                                !scope.is_none(),
                                "controller must start at route controller scope"
                            );
                            let evidence = IngressEvidence {
                                frame_label: FrameLabel::new(ENTRY_ARM1_SIGNAL_FRAME),
                                instance: 14,
                                has_fin: false,
                                channel: Channel::new(11),
                            };
                            let selection = OfferScopeSelection {
                                scope_id: scope,
                                frontier_parallel_root: None,
                                offer_lane: 0,
                                offer_lane_idx: 0,
                                at_route_offer_entry: false,
                            };
                            let resolved = ResolvedRouteDecision {
                                route_token: RouteDecisionToken::from_ack(
                                    Arm::new(0).expect("binary route arm"),
                                ),
                                selected_arm: 0,
                                resolved_hint_frame_label: None,
                            };
                            let staged_payload = Payload::new(&[0x6b]);
                            let staged_payload_len = staged_payload.as_bytes().len();

                            let branch: MaterializedRouteBranch<'_> =
                                RouteFrontierMachine::new(controller)
                                    .materialize_branch(
                                        selection,
                                        resolved,
                                        true,
                                        Some(LaneIngressEvidence::new(0, evidence)),
                                        staged_payload_len,
                                        0,
                                        Some(staged_payload),
                                    )
                                    .expect("non-wire branch must rebuffer stray binding evidence")
                                    .into();
                            assert!(
                                !branch_has_staged_payload(&branch),
                                "requeued transport payload must not remain staged on a non-wire branch"
                            );
                            assert_eq!(
                                transport_probe.requeue_count(),
                                1,
                                "non-wire materialization must requeue staged transport payload once"
                            );
                            controller.restore_materialized_route_branch(branch);
                            assert_eq!(
                                transport_probe.requeue_count(),
                                1,
                                "restoring the materialized branch must not requeue an already requeued payload"
                            );
                            assert_eq!(
                                controller.binding_inbox.take_matching_or_poll(
                                    &mut controller.binding,
                                    0,
                                    evidence.frame_label.raw(),
                                ),
                                Some(evidence),
                                "non-wire materialization must rebuffer stray binding evidence"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn pico_budget_offer_fixture_is_separate_from_large_host_fixture() {
    assert!(
        PICO_OFFER_FIXTURE_SLAB_CAPACITY < LARGE_HOST_OFFER_FIXTURE_SLAB_CAPACITY,
        "Pico-equivalent fixture budget must stay distinct from large host stress storage"
    );
    assert!(
        4096 <= PICO_OFFER_FIXTURE_SLAB_CAPACITY,
        "Pico-equivalent fixture must still cover common offer regressions"
    );
}

#[test]
fn dropped_branch_restores_original_binding_evidence_label() {
    run_offer_regression_test(
        "dropped_branch_restores_original_binding_evidence_label",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9061);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let observed = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 13,
                            has_fin: false,
                            channel: Channel::new(10),
                        };
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
                            offer_lane_idx: 0,
                            at_route_offer_entry: false,
                        };
                        let resolved = ResolvedRouteDecision {
                            route_token: RouteDecisionToken::from_ack(
                                Arm::new(0).expect("binary route arm"),
                            ),
                            selected_arm: 0,
                            resolved_hint_frame_label: None,
                        };
                        let branch = RouteFrontierMachine::new(worker)
                            .materialize_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, observed)),
                                0,
                                0,
                                None,
                            )
                            .expect("materialize selected branch")
                            .into();

                        worker.restore_materialized_route_branch(branch);
                        let restored = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            observed.frame_label.raw(),
                        );
                        assert_eq!(
                            restored,
                            Some(observed),
                            "branch restore must rebuffer the original observed evidence label"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn selected_branch_decode_uses_original_evidence_channel() {
    run_offer_regression_test(
        "selected_branch_decode_uses_original_evidence_channel",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9062);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &ENTRY_WORKER_PROGRAM(),
                                    TestBinding::with_incoming_and_payloads(&[], &[&[42]]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let observed = IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 14,
                            has_fin: false,
                            channel: Channel::new(11),
                        };
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
                            offer_lane_idx: 0,
                            at_route_offer_entry: false,
                        };
                        let resolved = ResolvedRouteDecision {
                            route_token: RouteDecisionToken::from_ack(
                                Arm::new(0).expect("binary route arm"),
                            ),
                            selected_arm: 0,
                            resolved_hint_frame_label: None,
                        };
                        let branch: MaterializedRouteBranch<'_> = RouteFrontierMachine::new(worker)
                            .materialize_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, observed)),
                                0,
                                0,
                                None,
                            )
                            .expect("materialize selected branch")
                            .into();

                        assert_eq!(
                            branch.binding_evidence.into_option(),
                            Some(observed),
                            "decode preview must retain the original demux evidence identity"
                        );
                        let mut cx = Context::from_waker(noop_waker_ref());
                        let decoded = {
                            let mut decode =
                                pin!(CursorDecode::<Msg<103, u8>>::run(worker, branch));
                            poll_ready_ok(
                                &mut cx,
                                decode.as_mut(),
                                "selected branch binding decode",
                            )
                        };
                        assert_eq!(decoded, 42);
                        assert_eq!(
                            worker.binding.last_recv_channel(),
                            Some(observed.channel),
                            "decode must use the original observed binding channel"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn wire_recv_with_selected_binding_evidence_requeues_transport_without_staging_it() {
    run_offer_regression_test(
        "wire_recv_with_selected_binding_evidence_requeues_transport_without_staging_it",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, PendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(PendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(PendingWorkerBindingEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport = PendingTransport::new(pending_state);
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(9064);
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        controller_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &ENTRY_CONTROLLER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach controller endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        worker_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &ENTRY_WORKER_PROGRAM(),
                                        TestBinding::with_incoming_and_payloads(&[], &[&[42]]),
                                    )
                                    .expect("attach worker endpoint");
                            }
                            let controller_borrow = controller_slot.borrow_mut();
                            core::hint::black_box(&controller_borrow);
                            let worker = worker_slot.borrow_mut();
                            let scope = worker.cursor.node_scope_id();
                            assert!(!scope.is_none(), "worker must start at route scope");

                            let observed = IngressEvidence {
                                frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                                instance: 16,
                                has_fin: false,
                                channel: Channel::new(21),
                            };
                            let selection = OfferScopeSelection {
                                scope_id: scope,
                                frontier_parallel_root: None,
                                offer_lane: 0,
                                offer_lane_idx: 0,
                                at_route_offer_entry: false,
                            };
                            let resolved = ResolvedRouteDecision {
                                route_token: RouteDecisionToken::from_ack(
                                    Arm::new(0).expect("binary route arm"),
                                ),
                                selected_arm: 0,
                                resolved_hint_frame_label: None,
                            };
                            let staged_transport = Payload::new(&[0x6b]);
                            let staged_transport_len = staged_transport.as_bytes().len();
                            let branch: MaterializedRouteBranch<'_> =
                                RouteFrontierMachine::new(worker)
                                    .materialize_branch(
                                        selection,
                                        resolved,
                                        false,
                                        Some(LaneIngressEvidence::new(0, observed)),
                                        staged_transport_len,
                                        0,
                                        Some(staged_transport),
                                    )
                                    .expect("materialize selected branch")
                                    .into();

                            assert_eq!(
                                transport_probe.requeue_count(),
                                1,
                                "selected binding evidence must requeue competing transport bytes"
                            );
                            assert!(
                                !branch_has_transport_payload(&branch),
                                "requeued transport bytes must not remain staged on the branch"
                            );
                            assert_eq!(
                                branch.binding_evidence.into_option(),
                                Some(observed),
                                "binding evidence must remain the selected payload authority"
                            );
                            let mut cx = Context::from_waker(noop_waker_ref());
                            let decoded = {
                                let mut decode =
                                    pin!(CursorDecode::<Msg<103, u8>>::run(worker, branch));
                                poll_ready_ok(
                                    &mut cx,
                                    decode.as_mut(),
                                    "selected binding decode with requeued transport",
                                )
                            };
                            assert_eq!(
                                decoded, 42,
                                "decode must consume binding payload, not requeued transport bytes"
                            );
                            assert_eq!(
                                worker.binding.last_recv_channel(),
                                Some(observed.channel),
                                "decode must call the selected binding channel"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn finish_resolved_rebuffers_carried_other_arm_evidence() {
    run_offer_regression_test(
        "finish_resolved_rebuffers_carried_other_arm_evidence",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9056);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let carried_other_arm = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 7,
                            has_fin: false,
                            channel: Channel::new(3),
                        };
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
                            offer_lane_idx: 0,
                            at_route_offer_entry: false,
                        };
                        let resolved = ResolvedRouteDecision {
                            route_token: RouteDecisionToken::from_ack(
                                Arm::new(0).expect("binary route arm"),
                            ),
                            selected_arm: 0,
                            resolved_hint_frame_label: None,
                        };
                        let branch: MaterializedRouteBranch<'_> = RouteFrontierMachine::new(worker)
                            .materialize_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, carried_other_arm)),
                                0,
                                0,
                                None,
                            )
                            .expect("materialize selected branch")
                            .into();

                        assert_eq!(branch_label(&branch), HINT_LEFT_DATA_LABEL);
                        assert!(
                            branch.binding_evidence.into_option().is_none(),
                            "selected branch must not carry other-arm demux evidence"
                        );
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_RIGHT_DATA_FRAME,
                        );
                        assert_eq!(
                            deferred,
                            Some(carried_other_arm),
                            "other-arm evidence must be rebuffered for its own branch"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn finish_resolved_does_not_drop_carried_other_arm_when_fresh_selected_evidence_exists() {
    run_offer_regression_test(
        "finish_resolved_does_not_drop_carried_other_arm_when_fresh_selected_evidence_exists",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9057);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let fresh_selected = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                            instance: 9,
                            has_fin: true,
                            channel: Channel::new(5),
                        };
                        let carried_other_arm = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 7,
                            has_fin: false,
                            channel: Channel::new(3),
                        };
                        worker.binding_inbox.put_back(0, fresh_selected);

                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
                            offer_lane_idx: 0,
                            at_route_offer_entry: false,
                        };
                        let resolved = ResolvedRouteDecision {
                            route_token: RouteDecisionToken::from_ack(
                                Arm::new(0).expect("binary route arm"),
                            ),
                            selected_arm: 0,
                            resolved_hint_frame_label: None,
                        };
                        let branch: MaterializedRouteBranch<'_> = RouteFrontierMachine::new(worker)
                            .materialize_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, carried_other_arm)),
                                0,
                                0,
                                None,
                            )
                            .expect("materialize selected branch")
                            .into();

                        assert_eq!(branch.binding_evidence.into_option(), Some(fresh_selected));
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_RIGHT_DATA_FRAME,
                        );
                        assert_eq!(
                            deferred,
                            Some(carried_other_arm),
                            "fresh selected evidence must not cause carried other-arm evidence to be dropped"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn authoritative_arm_decode_never_reads_other_arm_binding_channel() {
    run_offer_regression_test(
        "authoritative_arm_decode_never_reads_other_arm_binding_channel",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9058);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    TestBinding::with_incoming_and_payloads(&[], &[&[42]]),
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller_borrow = controller_slot.borrow_mut();
                        core::hint::black_box(&controller_borrow);
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let selected_evidence = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_LEFT_DATA_FRAME),
                            instance: 9,
                            has_fin: false,
                            channel: Channel::new(5),
                        };
                        let other_arm_evidence = IngressEvidence {
                            frame_label: FrameLabel::new(HINT_RIGHT_DATA_FRAME),
                            instance: 7,
                            has_fin: false,
                            channel: Channel::new(3),
                        };
                        worker.binding_inbox.put_back(0, selected_evidence);
                        let selection = OfferScopeSelection {
                            scope_id: scope,
                            frontier_parallel_root: None,
                            offer_lane: 0,
                            offer_lane_idx: 0,
                            at_route_offer_entry: false,
                        };
                        let resolved = ResolvedRouteDecision {
                            route_token: RouteDecisionToken::from_ack(
                                Arm::new(0).expect("binary route arm"),
                            ),
                            selected_arm: 0,
                            resolved_hint_frame_label: None,
                        };
                        let branch = RouteFrontierMachine::new(worker)
                            .materialize_branch(
                                selection,
                                resolved,
                                false,
                                Some(LaneIngressEvidence::new(0, other_arm_evidence)),
                                0,
                                0,
                                None,
                            )
                            .expect("materialize selected branch")
                            .into();

                        let mut cx = Context::from_waker(noop_waker_ref());
                        let decoded = {
                            let mut decode =
                                pin!(CursorDecode::<Msg<100, u8>>::run(worker, branch));
                            poll_ready_ok(
                                &mut cx,
                                decode.as_mut(),
                                "selected branch binding decode",
                            )
                        };
                        assert_eq!(decoded, 42);
                        assert_eq!(
                            worker.binding.last_recv_channel(),
                            Some(selected_evidence.channel),
                            "decode must use the selected arm binding channel, not carried other-arm evidence"
                        );
                        let deferred = worker.binding_inbox.take_matching_or_poll(
                            &mut worker.binding,
                            0,
                            HINT_RIGHT_DATA_FRAME,
                        );
                        assert_eq!(deferred, Some(other_arm_evidence));
                    });
                });
            });
        },
    );
}

#[test]
fn selected_route_arm_keeps_later_same_lane_sends_available() {
    run_offer_regression_test(
        "selected_route_arm_keeps_later_same_lane_sends_available",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(9055);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &MULTI_SEND_ROUTE_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &MULTI_SEND_ROUTE_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }

                        let controller = controller_slot.borrow_mut();
                        let mut cx = Context::from_waker(noop_waker_ref());

                        {
                            let mut route_right =
                                pin!(CursorSend::<MultiSendRouteRightMsg>::run(controller, ()));
                            let _ =
                                poll_ready_ok(&mut cx, route_right.as_mut(), "route-right send");
                        }

                        assert!(
                            controller.preview_flow::<MultiSendRightFirstMsg>().is_ok(),
                            "first payload send must remain available after choosing the route arm"
                        );
                        {
                            let mut first_payload =
                                pin!(CursorSend::<MultiSendRightFirstMsg>::run(controller, &1));
                            let _ = poll_ready_ok(
                                &mut cx,
                                first_payload.as_mut(),
                                "first payload send",
                            );
                        }

                        assert!(
                            controller.preview_flow::<MultiSendRightSecondMsg>().is_ok(),
                            "later payload send on the same route arm must remain available after the first send"
                        );
                        {
                            let mut second_payload =
                                pin!(CursorSend::<MultiSendRightSecondMsg>::run(controller, &2));
                            let _ = poll_ready_ok(
                                &mut cx,
                                second_payload.as_mut(),
                                "second payload send",
                            );
                        }
                    });
                });
            });
        },
    );
}

#[test]
fn static_passive_binding_label_materializes_poll() {
    run_offer_regression_test("static_passive_binding_label_materializes_poll", || {
        let entry_route_program = ENTRY_ROUTE_PROGRAM();
        let entry_controller_program = project(&entry_route_program);
        let entry_worker_program = project(&entry_route_program);
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            let transport = HintOnlyTransport::new(HINT_NONE);
            let rv_id = cluster_ref
                .add_rendezvous_from_config(config, transport)
                .expect("register rendezvous");
            let sid = SessionId::new(906);
            type ControllerEndpoint = CursorEndpoint<
                'static,
                0,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                TestBinding,
            >;
            let mut controller_storage = MaybeUninit::<OfferValueStorage>::uninit();
            let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
            unsafe {
                cluster_ref
                    .attach_endpoint_into::<0, _, _, _>(
                        controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                        rv_id,
                        sid,
                        &entry_controller_program,
                        NoBinding,
                    )
                    .expect("attach controller endpoint");
                cluster_ref
                    .attach_endpoint_into::<1, _, _, _>(
                        worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                        rv_id,
                        sid,
                        &entry_worker_program,
                        TestBinding::with_incoming(&[IngressEvidence {
                            frame_label: FrameLabel::new(ENTRY_ARM0_SIGNAL_FRAME),
                            instance: 0,
                            has_fin: false,
                            channel: Channel::new(1),
                        }]),
                    )
                    .expect("attach worker endpoint");
            }
            let controller =
                unsafe { &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>() };
            core::hint::black_box(&controller);
            let worker = unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };
            let scope = worker.cursor.node_scope_id();
            assert!(!scope.is_none(), "worker must start at route scope");
            assert!(
                worker
                    .cursor
                    .first_recv_target_for_lane_frame_label(scope, 0, ENTRY_ARM0_SIGNAL_FRAME)
                    .is_some(),
                "test requires a static passive recv dispatch target"
            );

            let frame_label_meta =
                endpoint_scope_frame_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);

            let mut binding_evidence = None;
            worker.cache_binding_evidence_for_offer(
                scope,
                0,
                frame_label_meta,
                worker.offer_scope_materialization_meta(scope, 0),
                &mut binding_evidence,
            );
            let evidence = binding_evidence.expect("binding evidence should be staged for poll");
            with_lane_set_view(&[0], |offer_lanes| {
                worker.ingest_scope_evidence_for_offer_lanes(
                    scope,
                    0,
                    offer_lanes,
                    false,
                    frame_label_meta,
                );
            });
            worker.ingest_binding_scope_evidence(
                scope,
                evidence.lane(),
                evidence.frame_label(),
                false,
                frame_label_meta,
            );

            assert!(
                worker.peek_scope_ack(scope).is_none(),
                "binding-backed static dispatch must not mint ack authority"
            );
            let resolved_label = worker.take_scope_frame_hint(scope);
            assert_eq!(
                resolved_label,
                Some(evidence.frame_label()),
                "binding-backed poll should still preserve the resolved ingress label"
            );
            assert_eq!(
                worker.poll_arm_from_ready_mask(scope),
                Some(Arm::new(0).expect("binary route arm")),
                "exact binding ingress on a static passive route must materialize Poll authority"
            );

            unsafe {
                core::ptr::drop_in_place(worker_storage.as_mut_ptr().cast::<WorkerEndpoint>());
                core::ptr::drop_in_place(
                    controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                );
            }
        });
    });
}

#[test]
fn static_passive_staged_transport_hint_materializes_poll() {
    run_offer_regression_test(
        "static_passive_staged_transport_hint_materializes_poll",
        || {
            let entry_route_program = ENTRY_ROUTE_PROGRAM();
            let entry_controller_program = project(&entry_route_program);
            let entry_worker_program = project(&entry_route_program);
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                let transport = HintOnlyTransport::new(ENTRY_ARM0_SIGNAL_FRAME);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(907);
                type ControllerEndpoint = CursorEndpoint<
                    'static,
                    0,
                    HintOnlyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                    4,
                    crate::control::cap::mint::MintConfig,
                    NoBinding,
                >;
                type WorkerEndpoint = CursorEndpoint<
                    'static,
                    1,
                    HintOnlyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                    4,
                    crate::control::cap::mint::MintConfig,
                    NoBinding,
                >;
                let mut controller_storage = MaybeUninit::<OfferValueStorage>::uninit();
                let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<0, _, _, _>(
                            controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                            rv_id,
                            sid,
                            &entry_controller_program,
                            NoBinding,
                        )
                        .expect("attach controller endpoint");
                    cluster_ref
                        .attach_endpoint_into::<1, _, _, _>(
                            worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                            rv_id,
                            sid,
                            &entry_worker_program,
                            NoBinding,
                        )
                        .expect("attach worker endpoint");
                }
                let controller =
                    unsafe { &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>() };
                core::hint::black_box(&controller);
                let worker = unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };
                let scope = worker.cursor.node_scope_id();
                assert!(!scope.is_none(), "worker must start at route scope");
                assert!(
                    worker
                        .cursor
                        .first_recv_target_for_lane_frame_label(scope, 0, ENTRY_ARM0_SIGNAL_FRAME)
                        .is_some(),
                    "test requires a static passive recv dispatch target"
                );

                let frame_label_meta =
                    endpoint_scope_frame_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
                with_lane_set_view(&[0], |offer_lanes| {
                    worker.ingest_scope_evidence_for_offer_lanes(
                        scope,
                        0,
                        offer_lanes,
                        false,
                        frame_label_meta,
                    );
                });

                assert_eq!(
                    worker.poll_arm_from_ready_mask(scope),
                    None,
                    "transport hint alone must remain non-authoritative until ingress is staged"
                );
                assert!(
                    worker.peek_scope_ack(scope).is_none(),
                    "transport-backed static dispatch must not mint ack authority"
                );
                let resolved_label = worker.take_scope_frame_hint(scope);
                assert_eq!(
                    resolved_label,
                    Some(ENTRY_ARM0_SIGNAL_FRAME),
                    "transport-backed poll should still preserve the resolved ingress label"
                );
                worker.mark_scope_ready_arm_from_frame_label(
                    scope,
                    0,
                    resolved_label.expect("transport hint must resolve"),
                    frame_label_meta,
                );
                assert_eq!(
                    worker.poll_arm_from_ready_mask(scope),
                    Some(Arm::new(0).expect("binary route arm")),
                    "staged exact transport ingress on a static passive route must materialize Poll authority"
                );

                unsafe {
                    core::ptr::drop_in_place(worker_storage.as_mut_ptr().cast::<WorkerEndpoint>());
                    core::ptr::drop_in_place(
                        controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                    );
                }
            });
        },
    );
}

#[test]
fn nested_static_passive_binding_dispatch_materializes_poll_on_ancestor_scopes() {
    run_offer_regression_test(
        "nested_static_passive_binding_dispatch_materializes_poll_on_ancestor_scopes",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(909);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &NESTED_STATIC_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &NESTED_STATIC_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();

                        let outer_scope = worker.cursor.node_scope_id();
                        let middle_scope = worker
                            .cursor
                            .passive_arm_scope_by_arm(outer_scope, 1)
                            .expect("outer right arm should enter middle route");
                        let inner_scope = worker
                            .cursor
                            .passive_arm_scope_by_arm(middle_scope, 0)
                            .expect("middle left arm should enter inner route");
                        let nested_leaf_frame = frame_label_for_cursor_label(&worker.cursor, 0x51);

                        assert_eq!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    outer_scope,
                                    0,
                                    nested_leaf_frame
                                )
                                .map(|(arm, _)| arm),
                            Some(1),
                            "outer scope must resolve the leaf reply through first-recv dispatch"
                        );
                        assert_eq!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    middle_scope,
                                    0,
                                    nested_leaf_frame
                                )
                                .map(|(arm, _)| arm),
                            Some(0),
                            "middle scope must resolve the leaf reply through first-recv dispatch"
                        );

                        for (scope, expected_arm) in
                            [(outer_scope, 1u8), (middle_scope, 0u8), (inner_scope, 0u8)]
                        {
                            let frame_label_meta = endpoint_scope_frame_label_meta(
                                worker,
                                scope,
                                ScopeLoopMeta::EMPTY,
                            );
                            with_lane_set_view(&[0], |offer_lanes| {
                                worker.ingest_scope_evidence_for_offer_lanes(
                                    scope,
                                    0,
                                    offer_lanes,
                                    false,
                                    frame_label_meta,
                                );
                            });
                            worker.ingest_binding_scope_evidence(
                                scope,
                                0,
                                nested_leaf_frame,
                                false,
                                frame_label_meta,
                            );
                            assert_eq!(
                                worker.poll_arm_from_ready_mask(scope),
                                Some(Arm::new(expected_arm).expect("binary route arm")),
                                "exact nested leaf ingress must materialize Poll for scope {scope:?}"
                            );
                        }
                    });
                });
            });
        },
    );
}

#[test]
fn dynamic_linger_parent_route_without_authoritative_arm_fails_decode_commit() {
    run_offer_regression_test(
        "dynamic_linger_parent_route_without_authoritative_arm_fails_decode_commit",
        || {
            type EntryArm0SignalMsg = Msg<{ ENTRY_ARM0_SIGNAL_LABEL }, u8>;
            type EntryArm0ReplyMsg = Msg<104, u8>;
            type DynamicParentLeftSteps =
                SeqSteps<HintLeftHead, SendOnly<0, Role<0>, Role<1>, Msg<100, u8>>>;
            type DynamicParentRightBodySteps = SeqSteps<
                SendOnly<0, Role<0>, Role<1>, EntryArm0SignalMsg>,
                SendOnly<0, Role<1>, Role<0>, EntryArm0ReplyMsg>,
            >;
            type DynamicParentRightSteps = SeqSteps<HintRightHead, DynamicParentRightBodySteps>;
            type DynamicParentEntrySteps =
                BranchSteps<DynamicParentLeftSteps, DynamicParentRightSteps>;
            static DYNAMIC_DECODE_PAYLOAD: [u8; 1] = [0x5a];
            type ControllerEndpoint = CursorEndpoint<
                'static,
                0,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;

            let program: g::Program<DynamicParentEntrySteps> = g::route(
                HINT_LEFT_ARM(),
                g::seq(
                    g::send::<
                        Role<0>,
                        Role<0>,
                        Msg<
                            ROUTE_HINT_RIGHT_LABEL,
                            GenericCapToken<RouteHintRightKind>,
                            RouteHintRightKind,
                        >,
                        0,
                    >()
                    .policy::<HINT_ROUTE_POLICY_ID>(),
                    g::seq(
                        g::send::<Role<0>, Role<1>, EntryArm0SignalMsg, 0>(),
                        g::send::<Role<1>, Role<0>, EntryArm0ReplyMsg, 0>(),
                    ),
                ),
            );
            let controller_program: RoleProgram<0> = project(&program);
            let worker_program: RoleProgram<1> = project(&program);
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(ControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(913);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &controller_program,
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &worker_program,
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }

                        let worker = worker_slot.borrow_mut();
                        let parent_scope = worker.cursor.node_scope_id();
                        let entry_arm0_signal_frame =
                            frame_label_for_cursor_label(&worker.cursor, ENTRY_ARM0_SIGNAL_LABEL);
                        assert!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    parent_scope,
                                    0,
                                    entry_arm0_signal_frame,
                                )
                                .is_none(),
                            "dynamic parent route must not expose static Poll dispatch"
                        );
                        let (parent_arm, target_idx) = (0..worker.cursor.local_steps_len())
                            .find_map(|idx| {
                                let recv_meta = worker.cursor.try_recv_meta_at(idx)?;
                                if recv_meta.label == ENTRY_ARM0_SIGNAL_LABEL
                                    && recv_meta.scope == parent_scope
                                {
                                    Some((recv_meta.route_arm?, idx))
                                } else {
                                    None
                                }
                            })
                            .expect("dynamic parent right arm should contain the staged recv");
                        assert_eq!(parent_arm, 1);
                        worker.set_cursor_index(target_idx);
                        let recv_meta = worker
                            .cursor
                            .try_recv_meta()
                            .expect("cursor must point at recv");
                        assert_eq!(recv_meta.label, ENTRY_ARM0_SIGNAL_LABEL);
                        let before_cursor = worker.cursor.index();
                        assert_eq!(
                            worker.selected_arm_for_scope(parent_scope),
                            None,
                            "dynamic parent must start without route authority"
                        );

                        let branch = MaterializedRouteBranch {
                            label: ENTRY_ARM0_SIGNAL_LABEL,
                            binding_evidence: PackedIngressEvidence::EMPTY,
                            binding_evidence_lane: u8::MAX,
                            staged_payload: Some(StagedPayload::Transport {
                                lane: recv_meta.lane,
                                payload: Payload::new(&DYNAMIC_DECODE_PAYLOAD),
                            }),
                            branch_meta: BranchMeta {
                                scope_id: parent_scope,
                                selected_arm: parent_arm,
                                lane_wire: recv_meta.lane,
                                eff_index: recv_meta.eff_index,
                                frame_label: recv_meta.frame_label,
                                kind: BranchKind::WireRecv,
                                route_source: RouteDecisionSource::Poll,
                            },
                        };
                        let mut cx = Context::from_waker(noop_waker_ref());
                        {
                            let mut decode =
                                pin!(CursorDecode::<EntryArm0SignalMsg>::run(worker, branch));
                            match decode.as_mut().poll(&mut cx) {
                                Poll::Ready(Err(RecvError::PhaseInvariant)) => {}
                                Poll::Ready(Ok(_)) => panic!(
                                    "decode must not commit a dynamic linger parent from child frame discriminator"
                                ),
                                Poll::Ready(Err(err)) => {
                                    panic!("decode failed with unexpected error: {err:?}")
                                }
                                Poll::Pending => panic!("staged decode unexpectedly pending"),
                            }
                        }

                        assert_eq!(
                            worker.selected_arm_for_scope(parent_scope),
                            None,
                            "dynamic parent must remain unselected after failed decode commit"
                        );
                        assert_eq!(
                            worker.cursor.index(),
                            before_cursor,
                            "decode commit failure must not publish cursor progress"
                        );
                        assert!(
                            worker.peek_scope_ack(parent_scope).is_none(),
                            "decode failure must not mint ACK authority for the dynamic parent"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn static_linger_parent_route_commits_only_through_static_poll_descriptor() {
    run_offer_regression_test(
        "static_linger_parent_route_commits_only_through_static_poll_descriptor",
        || {
            type EntryArm0SignalMsg = Msg<{ ENTRY_ARM0_SIGNAL_LABEL }, u8>;
            type EntryArm0ReplyMsg = Msg<104, u8>;
            static STATIC_DECODE_PAYLOAD: [u8; 1] = [0x5b];
            type StaticParentEntryLeftSteps = SeqSteps<
                SendOnly<0, Role<0>, Role<0>, NestedStaticRouteLeftMsg>,
                SendOnly<0, Role<0>, Role<1>, NestedStaticOuterLeftMsg>,
            >;
            type StaticParentEntryRightBodySteps = SeqSteps<
                SendOnly<0, Role<0>, Role<1>, EntryArm0SignalMsg>,
                SendOnly<0, Role<1>, Role<0>, EntryArm0ReplyMsg>,
            >;
            type StaticParentEntryRightSteps = SeqSteps<
                SendOnly<0, Role<0>, Role<0>, NestedStaticRouteRightMsg>,
                StaticParentEntryRightBodySteps,
            >;
            type StaticParentEntrySteps =
                BranchSteps<StaticParentEntryLeftSteps, StaticParentEntryRightSteps>;
            type ControllerEndpoint = CursorEndpoint<
                'static,
                0,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;
            type WorkerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;

            let program: g::Program<StaticParentEntrySteps> = g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, NestedStaticRouteLeftMsg, 0>(),
                    g::send::<Role<0>, Role<1>, NestedStaticOuterLeftMsg, 0>(),
                ),
                g::seq(
                    g::send::<Role<0>, Role<0>, NestedStaticRouteRightMsg, 0>(),
                    g::seq(
                        g::send::<Role<0>, Role<1>, EntryArm0SignalMsg, 0>(),
                        g::send::<Role<1>, Role<0>, EntryArm0ReplyMsg, 0>(),
                    ),
                ),
            );
            let controller_program: RoleProgram<0> = project(&program);
            let worker_program: RoleProgram<1> = project(&program);
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(ControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(WorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(914);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &controller_program,
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &worker_program,
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }

                        let worker = worker_slot.borrow_mut();
                        let parent_scope = worker.cursor.node_scope_id();
                        let entry_arm0_signal_frame =
                            frame_label_for_cursor_label(&worker.cursor, ENTRY_ARM0_SIGNAL_LABEL);
                        assert_eq!(
                            worker
                                .cursor
                                .first_recv_target_for_lane_frame_label(
                                    parent_scope,
                                    0,
                                    entry_arm0_signal_frame,
                                )
                                .map(|(arm, _)| arm),
                            Some(1),
                            "static parent must expose compiled Poll dispatch for the child frame label"
                        );
                        assert!(
                            worker.peek_scope_ack(parent_scope).is_none(),
                            "test must start without ACK/Resolver route authority"
                        );
                        let (parent_arm, target_idx) = worker
                            .cursor
                            .first_recv_target_for_lane_frame_label(
                                parent_scope,
                                0,
                                entry_arm0_signal_frame,
                            )
                            .expect("parent route should expose Poll dispatch");
                        assert_eq!(parent_arm, 1);
                        worker.set_cursor_index(state_index_to_usize(target_idx));
                        let recv_meta = worker
                            .cursor
                            .try_recv_meta()
                            .expect("cursor must point at child recv");
                        assert_eq!(recv_meta.label, ENTRY_ARM0_SIGNAL_LABEL);

                        let branch = MaterializedRouteBranch {
                            label: ENTRY_ARM0_SIGNAL_LABEL,
                            binding_evidence: PackedIngressEvidence::EMPTY,
                            binding_evidence_lane: u8::MAX,
                            staged_payload: Some(StagedPayload::Transport {
                                lane: recv_meta.lane,
                                payload: Payload::new(&STATIC_DECODE_PAYLOAD),
                            }),
                            branch_meta: BranchMeta {
                                scope_id: parent_scope,
                                selected_arm: parent_arm,
                                lane_wire: recv_meta.lane,
                                eff_index: recv_meta.eff_index,
                                frame_label: recv_meta.frame_label,
                                kind: BranchKind::WireRecv,
                                route_source: RouteDecisionSource::Poll,
                            },
                        };
                        let mut cx = Context::from_waker(noop_waker_ref());
                        {
                            let mut decode =
                                pin!(CursorDecode::<EntryArm0SignalMsg>::run(worker, branch));
                            let decoded = poll_ready_ok(
                                &mut cx,
                                decode.as_mut(),
                                "static child decode commit",
                            );
                            assert_eq!(decoded, STATIC_DECODE_PAYLOAD[0]);
                        }

                        assert_eq!(
                            worker.selected_arm_for_scope(parent_scope),
                            Some(1),
                            "static parent route must commit through compiled Poll descriptor"
                        );
                        assert!(
                            worker.peek_scope_ack(parent_scope).is_none(),
                            "static Poll commit must not synthesize ACK authority"
                        );
                        let follow_up = 0x6cu8;
                        let mut send_reply =
                            pin!(CursorSend::<EntryArm0ReplyMsg>::run(worker, &follow_up));
                        let _ = poll_ready_ok(
                            &mut cx,
                            send_reply.as_mut(),
                            "post-decode child route continuation",
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn deep_right_nested_static_passive_binding_dispatch_materializes_poll_on_all_ancestor_scopes() {
    run_offer_regression_test(
        "deep_right_nested_static_passive_binding_dispatch_materializes_poll_on_all_ancestor_scopes",
        || {
            let deep_right_program = DEEP_RIGHT_PROGRAM();
            let deep_right_controller_program = project(&deep_right_program);
            let deep_right_worker_program = project(&deep_right_program);
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                let transport = HintOnlyTransport::new(HINT_NONE);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(910);
                type ControllerEndpoint = CursorEndpoint<
                    'static,
                    0,
                    HintOnlyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                    4,
                    crate::control::cap::mint::MintConfig,
                    NoBinding,
                >;
                type WorkerEndpoint = CursorEndpoint<
                    'static,
                    1,
                    HintOnlyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                    4,
                    crate::control::cap::mint::MintConfig,
                    NoBinding,
                >;
                let mut controller_storage = MaybeUninit::<OfferValueStorage>::uninit();
                let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<0, _, _, _>(
                            controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                            rv_id,
                            sid,
                            &deep_right_controller_program,
                            NoBinding,
                        )
                        .expect("attach controller endpoint");
                    cluster_ref
                        .attach_endpoint_into::<1, _, _, _>(
                            worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                            rv_id,
                            sid,
                            &deep_right_worker_program,
                            NoBinding,
                        )
                        .expect("attach worker endpoint");
                }
                let worker = unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };

                let outer_scope = worker.cursor.node_scope_id();
                let middle_scope = worker
                    .cursor
                    .passive_arm_scope_by_arm(outer_scope, 1)
                    .expect("outer right arm should enter middle route");
                let third_scope = worker
                    .cursor
                    .passive_arm_scope_by_arm(middle_scope, 1)
                    .expect("middle right arm should enter third route");
                let final_scope = worker
                    .cursor
                    .passive_arm_scope_by_arm(third_scope, 1)
                    .expect("third right arm should enter final route");
                let deep_final_frame = frame_label_for_cursor_label(&worker.cursor, 0x55);

                for scope in [outer_scope, middle_scope, third_scope] {
                    assert_eq!(
                        worker
                            .cursor
                            .first_recv_target_for_lane_frame_label(scope, 0, deep_final_frame)
                            .map(|(arm, _)| arm),
                        Some(1),
                        "ancestor scope must resolve the deep final reply through first-recv dispatch"
                    );
                }

                let frame_label_meta =
                    endpoint_scope_frame_label_meta(worker, outer_scope, ScopeLoopMeta::EMPTY);
                with_lane_set_view(&[0], |offer_lanes| {
                    worker.ingest_scope_evidence_for_offer_lanes(
                        outer_scope,
                        0,
                        offer_lanes,
                        false,
                        frame_label_meta,
                    );
                });
                worker.ingest_binding_scope_evidence(
                    outer_scope,
                    0,
                    deep_final_frame,
                    false,
                    frame_label_meta,
                );

                for scope in [outer_scope, middle_scope, third_scope, final_scope] {
                    assert_eq!(
                        worker.poll_arm_from_ready_mask(scope),
                        Some(Arm::new(1).expect("binary route arm")),
                        "exact deep final ingress must materialize Poll for scope {scope:?}"
                    );
                    assert_eq!(
                        worker.preview_selected_arm_for_scope(scope),
                        Some(1),
                        "exact deep final ingress must seed descendant preview selection for scope {scope:?}"
                    );
                }

                unsafe {
                    core::ptr::drop_in_place(worker_storage.as_mut_ptr().cast::<WorkerEndpoint>());
                    core::ptr::drop_in_place(
                        controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                    );
                }
            });
        },
    );
}

#[test]
fn deep_right_nested_final_reply_offer_materializes_leaf_label() {
    run_offer_regression_test(
        "deep_right_nested_final_reply_offer_materializes_leaf_label",
        || {
            let deep_right_program = DEEP_RIGHT_PROGRAM();
            let deep_right_controller_program = project(&deep_right_program);
            let deep_right_worker_program = project(&deep_right_program);
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                let transport = HintOnlyTransport::new(HINT_NONE);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(911);
                let payload = 0x55u8;
                type ControllerEndpoint = CursorEndpoint<
                    'static,
                    0,
                    HintOnlyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                    4,
                    crate::control::cap::mint::MintConfig,
                    NoBinding,
                >;
                type WorkerEndpoint = CursorEndpoint<
                    'static,
                    1,
                    HintOnlyTransport,
                    DefaultLabelUniverse,
                    CounterClock,
                    crate::control::cap::mint::EpochTbl,
                    4,
                    crate::control::cap::mint::MintConfig,
                    TestBinding,
                >;
                let mut controller_storage = MaybeUninit::<OfferValueStorage>::uninit();
                let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<0, _, _, _>(
                            controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                            rv_id,
                            sid,
                            &deep_right_controller_program,
                            NoBinding,
                        )
                        .expect("attach controller endpoint");
                    cluster_ref
                        .attach_endpoint_into::<1, _, _, _>(
                            worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                            rv_id,
                            sid,
                            &deep_right_worker_program,
                            TestBinding::with_incoming_and_payloads(
                                &[IngressEvidence {
                                    frame_label: FrameLabel::new(DEEP_RIGHT_FINAL_RIGHT_FRAME),
                                    instance: 17,
                                    has_fin: false,
                                    channel: Channel::new(4),
                                }],
                                &[&[payload]],
                            ),
                        )
                        .expect("attach worker endpoint");
                }

                let waker = noop_waker_ref();
                let mut cx = Context::from_waker(waker);

                {
                    let controller = unsafe {
                        &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                    };
                    let mut route_right = core::pin::pin!(
                        CursorSend::<DeepRightStaticRouteRightMsg>::run(controller, ())
                    );
                    let _ = poll_ready_ok(&mut cx, route_right.as_mut(), "outer route-right send");
                }
                {
                    let controller = unsafe {
                        &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                    };
                    let mut route_right = core::pin::pin!(
                        CursorSend::<DeepRightStaticRouteRightMsg>::run(controller, ())
                    );
                    let _ = poll_ready_ok(&mut cx, route_right.as_mut(), "middle route-right send");
                }
                {
                    let controller = unsafe {
                        &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                    };
                    let mut route_right = core::pin::pin!(
                        CursorSend::<DeepRightStaticRouteRightMsg>::run(controller, ())
                    );
                    let _ = poll_ready_ok(&mut cx, route_right.as_mut(), "third route-right send");
                }
                {
                    let controller = unsafe {
                        &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                    };
                    let mut route_right = core::pin::pin!(
                        CursorSend::<DeepRightStaticRouteRightMsg>::run(controller, ())
                    );
                    let _ = poll_ready_ok(&mut cx, route_right.as_mut(), "final route-right send");
                }
                {
                    let controller = unsafe {
                        &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                    };
                    let mut reply_send = core::pin::pin!(
                        CursorSend::<DeepRightFinalRightMsg>::run(controller, &payload)
                    );
                    let _ = poll_ready_ok(&mut cx, reply_send.as_mut(), "final right reply send");
                }

                let worker = unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };
                let branch = {
                    let mut offer = pin!(cursor_offer(worker));
                    match offer.as_mut().poll(&mut cx) {
                        Poll::Ready(Ok(branch)) => branch,
                        Poll::Ready(Err(err)) => {
                            panic!("worker deep final offer failed: {err:?}")
                        }
                        Poll::Pending => {
                            panic!("worker deep final offer unexpectedly pending")
                        }
                    }
                };
                assert_eq!(
                    branch_label(&branch),
                    0x55,
                    "worker must materialize the deep final reply"
                );
                let mut decode = pin!(CursorDecode::<DeepRightFinalRightMsg>::run(worker, branch));
                match decode.as_mut().poll(&mut cx) {
                    Poll::Ready(Ok(reply)) => assert_eq!(reply, payload),
                    Poll::Ready(Err(err)) => {
                        panic!("worker deep final decode failed: {err:?}")
                    }
                    Poll::Pending => {
                        panic!("worker deep final decode unexpectedly pending")
                    }
                }

                unsafe {
                    core::ptr::drop_in_place(worker_storage.as_mut_ptr().cast::<WorkerEndpoint>());
                    core::ptr::drop_in_place(
                        controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                    );
                }
            });
        },
    );
}

#[test]
fn deep_right_nested_final_reply_offer_materializes_leaf_label_with_deferred_binding_ingress() {
    run_offer_regression_test(
        "deep_right_nested_final_reply_offer_materializes_leaf_label_with_deferred_binding_ingress",
        || {
            let deep_right_program = DEEP_RIGHT_PROGRAM();
            let deep_right_controller_program = project(&deep_right_program);
            let deep_right_worker_program = project(&deep_right_program);
            type DeferredCluster = SessionCluster<
                'static,
                DeferredIngressTransport,
                DefaultLabelUniverse,
                CounterClock,
                4,
            >;
            offer_fixture!(2048, clock, config);
            with_offer_value_slot!(DeferredIngressState, deferred_state_slot, {
                deferred_state_slot.store(DeferredIngressState::new());
                let deferred_state: &'static DeferredIngressState =
                    unsafe { &*deferred_state_slot.ptr() };
                with_offer_cluster!(clock, DeferredCluster, cluster_ref, {
                    let transport = DeferredIngressTransport::new(deferred_state);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(912);
                    let payload = 0x55u8;
                    type ControllerEndpoint = CursorEndpoint<
                        'static,
                        0,
                        DeferredIngressTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        crate::control::cap::mint::EpochTbl,
                        4,
                        crate::control::cap::mint::MintConfig,
                        NoBinding,
                    >;
                    type WorkerEndpoint = CursorEndpoint<
                        'static,
                        1,
                        DeferredIngressTransport,
                        DefaultLabelUniverse,
                        CounterClock,
                        crate::control::cap::mint::EpochTbl,
                        4,
                        crate::control::cap::mint::MintConfig,
                        DeferredIngressBinding,
                    >;
                    let mut controller_storage = MaybeUninit::<OfferValueStorage>::uninit();
                    let mut worker_storage = MaybeUninit::<OfferValueStorage>::uninit();
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                                rv_id,
                                sid,
                                &deep_right_controller_program,
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                                rv_id,
                                sid,
                                &deep_right_worker_program,
                                DeferredIngressBinding::with_incoming_and_payloads(
                                    deferred_state,
                                    &[IngressEvidence {
                                        frame_label: FrameLabel::new(DEEP_RIGHT_FINAL_RIGHT_FRAME),
                                        instance: 17,
                                        has_fin: false,
                                        channel: Channel::new(4),
                                    }],
                                    &[&[payload]],
                                ),
                            )
                            .expect("attach worker endpoint");
                    }

                    let waker = noop_waker_ref();
                    let mut cx = Context::from_waker(waker);

                    {
                        let controller = unsafe {
                            &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                        };
                        let mut route_right = core::pin::pin!(CursorSend::<
                            DeepRightStaticRouteRightMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right.as_mut(),
                            "outer deferred route-right send",
                        );
                    }
                    {
                        let controller = unsafe {
                            &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                        };
                        let mut route_right = core::pin::pin!(CursorSend::<
                            DeepRightStaticRouteRightMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right.as_mut(),
                            "middle deferred route-right send",
                        );
                    }
                    {
                        let controller = unsafe {
                            &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                        };
                        let mut route_right = core::pin::pin!(CursorSend::<
                            DeepRightStaticRouteRightMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right.as_mut(),
                            "third deferred route-right send",
                        );
                    }
                    {
                        let controller = unsafe {
                            &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                        };
                        let mut route_right = core::pin::pin!(CursorSend::<
                            DeepRightStaticRouteRightMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right.as_mut(),
                            "final deferred route-right send",
                        );
                    }
                    {
                        let controller = unsafe {
                            &mut *controller_storage.as_mut_ptr().cast::<ControllerEndpoint>()
                        };
                        let mut reply_send = core::pin::pin!(
                            CursorSend::<DeepRightFinalRightMsg>::run(controller, &payload)
                        );
                        let _ = poll_ready_ok(
                            &mut cx,
                            reply_send.as_mut(),
                            "final deferred right reply send",
                        );
                    }

                    let worker =
                        unsafe { &mut *worker_storage.as_mut_ptr().cast::<WorkerEndpoint>() };
                    let branch = {
                        let mut offer = pin!(cursor_offer(worker));
                        match offer.as_mut().poll(&mut cx) {
                            Poll::Ready(Ok(branch)) => branch,
                            Poll::Ready(Err(err)) => {
                                panic!("worker deep final deferred offer failed: {err:?}")
                            }
                            Poll::Pending => {
                                panic!("worker deep final deferred offer unexpectedly pending")
                            }
                        }
                    };
                    assert_eq!(
                        branch_label(&branch),
                        0x55,
                        "worker must materialize the deep final reply after deferred binding ingress"
                    );
                    let mut decode =
                        pin!(CursorDecode::<DeepRightFinalRightMsg>::run(worker, branch));
                    match decode.as_mut().poll(&mut cx) {
                        Poll::Ready(Ok(reply)) => assert_eq!(reply, payload),
                        Poll::Ready(Err(err)) => {
                            panic!("worker deep final deferred decode failed: {err:?}")
                        }
                        Poll::Pending => {
                            panic!("worker deep final deferred decode unexpectedly pending")
                        }
                    }

                    unsafe {
                        core::ptr::drop_in_place(
                            worker_storage.as_mut_ptr().cast::<WorkerEndpoint>(),
                        );
                        core::ptr::drop_in_place(
                            controller_storage.as_mut_ptr().cast::<ControllerEndpoint>(),
                        );
                    }
                });
            });
        },
    );
}

#[test]
fn unique_ready_arm_materializes_poll_without_hint() {
    run_offer_regression_test("unique_ready_arm_materializes_poll_without_hint", || {
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                let transport = HintOnlyTransport::new(HINT_NONE);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(908);
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<1, _, _, _>(
                            worker_slot.ptr(),
                            rv_id,
                            sid,
                            &HINT_WORKER_PROGRAM(),
                            NoBinding,
                        )
                        .expect("attach worker endpoint");
                }
                let worker = worker_slot.borrow_mut();
                let scope = worker.cursor.node_scope_id();
                assert!(!scope.is_none(), "worker must start at route scope");

                assert_eq!(
                    worker.poll_arm_from_ready_mask(scope),
                    None,
                    "no ready arm evidence must not materialize a poll arm"
                );

                worker.mark_scope_ready_arm(scope, 1);
                assert_eq!(
                    worker.poll_arm_from_ready_mask(scope).map(Arm::as_u8),
                    Some(1),
                    "a unique ready arm should materialize a poll arm"
                );

                worker.mark_scope_ready_arm(scope, 0);
                assert_eq!(
                    worker.poll_arm_from_ready_mask(scope),
                    None,
                    "ambiguous ready-arm evidence must not materialize a poll arm"
                );
            });
        });
    });
}

#[test]
fn select_scope_recovers_route_state_from_current_arm_position() {
    run_offer_regression_test(
        "select_scope_recovers_route_state_from_current_arm_position",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(907);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &ENTRY_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    let Some(PassiveArmNavigation::WithinArm { entry }) = worker
                        .cursor
                        .follow_passive_observer_arm_for_scope(scope, 0)
                    else {
                        panic!("worker should expose passive arm entry");
                    };
                    worker.set_cursor_index(state_index_to_usize(entry));
                    assert_eq!(
                        worker.selected_arm_for_scope(scope),
                        None,
                        "test requires missing runtime route state"
                    );
                    assert_eq!(
                        worker
                            .cursor
                            .typestate_node(worker.cursor.index())
                            .route_arm(),
                        Some(0),
                        "current node must carry structural arm annotation"
                    );

                    let recovered = worker.current_route_arm_authorized();
                    assert!(
                        recovered.is_err(),
                        "missing runtime route state must fail closed instead of being repaired"
                    );
                    assert_eq!(
                        worker.selected_arm_for_scope(scope),
                        None,
                        "current arm position must not restore selected arm state"
                    );
                });
            });
        },
    );
}

#[test]
fn route_decision_source_domain_is_closed() {
    assert!(matches!(
        RouteDecisionSource::from_tap_seq(1),
        Some(RouteDecisionSource::Ack)
    ));
    assert!(matches!(
        RouteDecisionSource::from_tap_seq(2),
        Some(RouteDecisionSource::Resolver)
    ));
    assert!(matches!(
        RouteDecisionSource::from_tap_seq(3),
        Some(RouteDecisionSource::Poll)
    ));
    assert!(RouteDecisionSource::from_tap_seq(0).is_none());
    assert!(RouteDecisionSource::from_tap_seq(4).is_none());
}

#[test]
fn defer_without_new_evidence_is_capped() {
    let mut liveness = OfferLivenessState::new(crate::runtime::config::LivenessPolicy {
        max_defer_per_offer: 8,
        max_no_evidence_defer: 1,
        force_poll_on_exhaustion: false,
        max_forced_poll_attempts: 0,
        exhaust_reason: 1,
    });
    let fingerprint = EvidenceFingerprint::new(false, false, false);
    assert_eq!(liveness.on_defer(fingerprint), DeferBudgetOutcome::Continue);
    assert_eq!(liveness.on_defer(fingerprint), DeferBudgetOutcome::Continue);
    assert_eq!(
        liveness.on_defer(fingerprint),
        DeferBudgetOutcome::Exhausted
    );
}

#[test]
fn defer_budget_exhaustion_forces_poll_then_abort() {
    let mut liveness = OfferLivenessState::new(crate::runtime::config::LivenessPolicy {
        max_defer_per_offer: 1,
        max_no_evidence_defer: 1,
        force_poll_on_exhaustion: true,
        max_forced_poll_attempts: 1,
        exhaust_reason: crate::policy_runtime::ENGINE_LIVENESS_EXHAUSTED,
    });
    let fingerprint = EvidenceFingerprint::new(false, false, false);
    assert_eq!(liveness.on_defer(fingerprint), DeferBudgetOutcome::Continue);
    assert_eq!(
        liveness.on_defer(fingerprint),
        DeferBudgetOutcome::Exhausted
    );
    assert!(liveness.can_force_poll());
    liveness.mark_forced_poll();
    assert!(!liveness.can_force_poll());
    assert_eq!(
        liveness.exhaust_reason(),
        crate::policy_runtime::ENGINE_LIVENESS_EXHAUSTED
    );
}

#[test]
fn defer_never_promotes_to_route_authority() {
    let defer_tag = DeferSource::Resolver.as_audit_tag();
    assert_eq!(
        DeferSource::from_audit_tag(defer_tag),
        Some(DeferSource::Resolver)
    );
    assert!(
        RouteDecisionSource::from_tap_seq(defer_tag).is_none(),
        "defer audit tags must stay outside route authority tap tags"
    );
    for authority_tag in [
        RouteDecisionSource::Ack.as_tap_seq(),
        RouteDecisionSource::Resolver.as_tap_seq(),
        RouteDecisionSource::Poll.as_tap_seq(),
    ] {
        assert!(
            DeferSource::from_audit_tag(authority_tag).is_none(),
            "route authority tap tags must not decode as defer sources"
        );
    }
}

#[test]
fn scope_evidence_is_one_shot_per_offer() {
    let token = RouteDecisionToken::from_ack(Arm::new(1).expect("arm"));
    let mut evidence = ScopeEvidence {
        ack: Some(token),
        hint_frame_label: 7,
        ready_arm_mask: ScopeEvidence::ARM1_READY,
        poll_ready_arm_mask: ScopeEvidence::ARM1_READY,
        flags: 0,
    };
    let first = {
        let ack = evidence.ack;
        evidence.ack = None;
        ack
    };
    let second = evidence.ack;
    assert_eq!(first, Some(token));
    assert_eq!(second, None);
}

#[test]
fn resolver_poll_token_requires_ready_arm_evidence_for_controller_and_observer() {
    run_offer_regression_test(
        "resolver_poll_token_requires_ready_arm_evidence_for_controller_and_observer",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(990);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let controller = controller_slot.borrow_mut();
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        let resolver_token =
                            RouteDecisionToken::from_resolver(Arm::new(0).expect("arm"));
                        assert!(
                            !worker.route_token_has_materialization_evidence(scope, resolver_token),
                            "resolver token must not materialize without arm-ready evidence"
                        );

                        worker.mark_scope_ready_arm(scope, 0);
                        assert!(
                            worker.route_token_has_materialization_evidence(scope, resolver_token),
                            "resolver token may materialize only when selected arm has ready evidence"
                        );

                        let poll_token = RouteDecisionToken::from_poll(Arm::new(1).expect("arm"));
                        assert!(
                            !worker.route_token_has_materialization_evidence(scope, poll_token),
                            "poll token must not materialize for unready arm"
                        );

                        worker.mark_scope_ready_arm(scope, 1);
                        assert!(
                            worker.route_token_has_materialization_evidence(scope, poll_token),
                            "poll token may materialize when selected arm has ready evidence"
                        );

                        let controller_scope = controller.cursor.node_scope_id();
                        assert!(
                            !controller_scope.is_none(),
                            "controller must start at route scope"
                        );
                        let controller_recv_arm = if controller.arm_has_recv(controller_scope, 0) {
                            Some(0)
                        } else if controller.arm_has_recv(controller_scope, 1) {
                            Some(1)
                        } else {
                            None
                        };
                        if let Some(controller_arm) = controller_recv_arm {
                            let controller_resolver_token = RouteDecisionToken::from_resolver(
                                Arm::new(controller_arm).expect("arm"),
                            );
                            assert!(
                                !controller.route_token_has_materialization_evidence(
                                    controller_scope,
                                    controller_resolver_token
                                ),
                                "controller resolver token must not materialize without arm-ready evidence when recv is required"
                            );
                            controller.mark_scope_ready_arm(controller_scope, controller_arm);
                            assert!(
                                controller.route_token_has_materialization_evidence(
                                    controller_scope,
                                    controller_resolver_token
                                ),
                                "controller resolver token requires selected arm evidence as well"
                            );
                        }
                    });
                });
            });
        },
    );
}

#[test]
fn recv_required_arm_needs_ready_arm_evidence_for_all_sources() {
    run_offer_regression_test(
        "recv_required_arm_needs_ready_arm_evidence_for_all_sources",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(993);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");
                    let recv_arm = if worker.arm_has_recv(scope, 0) {
                        0
                    } else if worker.arm_has_recv(scope, 1) {
                        1
                    } else {
                        return;
                    };
                    let ack_token = RouteDecisionToken::from_ack(Arm::new(recv_arm).expect("arm"));
                    let resolver_token =
                        RouteDecisionToken::from_resolver(Arm::new(recv_arm).expect("arm"));
                    let poll_token =
                        RouteDecisionToken::from_poll(Arm::new(recv_arm).expect("arm"));
                    assert!(
                        !worker.route_token_has_materialization_evidence(scope, ack_token),
                        "ack token must not materialize recv-required arm without ready-arm evidence"
                    );
                    assert!(
                        !worker.route_token_has_materialization_evidence(scope, resolver_token),
                        "resolver token must not materialize recv-required arm without ready-arm evidence"
                    );
                    assert!(
                        !worker.route_token_has_materialization_evidence(scope, poll_token),
                        "poll token must not materialize recv-required arm without ready-arm evidence"
                    );
                    worker.mark_scope_ready_arm(scope, recv_arm);
                    assert!(
                        worker.route_token_has_materialization_evidence(scope, ack_token),
                        "ack token may materialize recv-required arm when selected arm is ready"
                    );
                    assert!(
                        worker.route_token_has_materialization_evidence(scope, resolver_token),
                        "resolver token may materialize recv-required arm when selected arm is ready"
                    );
                    assert!(
                        worker.route_token_has_materialization_evidence(scope, poll_token),
                        "poll token may materialize recv-required arm when selected arm is ready"
                    );
                });
            });
        },
    );
}

#[test]
fn route_ack_does_not_imply_ready_arm_evidence() {
    run_offer_regression_test("route_ack_does_not_imply_ready_arm_evidence", || {
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                let transport = HintOnlyTransport::new(HINT_NONE);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(994);
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<1, _, _, _>(
                            worker_slot.ptr(),
                            rv_id,
                            sid,
                            &HINT_WORKER_PROGRAM(),
                            NoBinding,
                        )
                        .expect("attach worker endpoint");
                }
                let worker = worker_slot.borrow_mut();
                let scope = worker.cursor.node_scope_id();
                assert!(!scope.is_none(), "worker must start at route scope");
                let arm = if worker.arm_has_recv(scope, 0) { 0 } else { 1 };
                worker.record_scope_ack(
                    scope,
                    RouteDecisionToken::from_ack(Arm::new(arm).expect("arm")),
                );
                assert!(
                    worker.peek_scope_ack(scope).is_some(),
                    "ack authority should be preserved"
                );
                assert!(
                    !worker.scope_has_ready_arm(scope, arm),
                    "ack authority must not become recv-ready evidence"
                );
            });
        });
    });
}

#[test]
fn route_ack_conflict_is_fatal_and_not_cleared_by_recovery() {
    run_offer_regression_test(
        "route_ack_conflict_is_fatal_and_not_cleared_by_recovery",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1010);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    worker.record_scope_ack(
                        scope,
                        RouteDecisionToken::from_ack(Arm::new(0).expect("arm")),
                    );
                    worker.record_scope_ack(
                        scope,
                        RouteDecisionToken::from_ack(Arm::new(1).expect("arm")),
                    );

                    assert!(
                        worker.scope_evidence_conflicted(scope),
                        "conflicting ACK authorities must poison the scope"
                    );
                    assert!(
                        worker.peek_scope_ack(scope).is_none(),
                        "conflicting ACK authorities must not leave a selectable authority"
                    );
                    assert!(
                        !worker.recover_scope_evidence_conflict(scope, true, false),
                        "dynamic recovery may not erase an ACK authority conflict"
                    );
                    assert!(
                        worker.scope_evidence_conflicted(scope),
                        "ACK conflict must remain observable after rejected recovery"
                    );
                });
            });
        },
    );
}

#[test]
fn route_hint_conflict_recovery_clears_only_hint_conflict() {
    run_offer_regression_test(
        "route_hint_conflict_recovery_clears_only_hint_conflict",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1011);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");
                    let ack = RouteDecisionToken::from_ack(Arm::new(0).expect("arm"));

                    worker.record_scope_ack(scope, ack);
                    worker.record_scope_frame_hint(scope, HINT_LEFT_DATA_FRAME);
                    worker.record_scope_frame_hint(scope, HINT_RIGHT_DATA_FRAME);

                    assert!(
                        worker.scope_evidence_conflicted(scope),
                        "conflicting hints must be observable before recovery"
                    );
                    assert_eq!(
                        worker.peek_scope_ack(scope),
                        Some(ack),
                        "hint conflict must not erase ACK authority"
                    );
                    assert!(
                        worker.recover_scope_evidence_conflict(scope, true, false),
                        "dynamic recovery may clear hint-only conflict"
                    );
                    assert!(
                        !worker.scope_evidence_conflicted(scope),
                        "hint-only conflict must be cleared"
                    );
                    assert_eq!(
                        worker.peek_scope_ack(scope),
                        Some(ack),
                        "hint-only recovery must preserve ACK authority"
                    );
                });
            });
        },
    );
}

#[test]
fn ready_arm_mask_is_one_shot_and_cleared_on_scope_exit() {
    run_offer_regression_test(
        "ready_arm_mask_is_one_shot_and_cleared_on_scope_exit",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(991);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        assert!(!scope.is_none(), "worker must start at route scope");

                        worker.mark_scope_ready_arm(scope, 0);
                        assert!(worker.scope_has_ready_arm(scope, 0));
                        worker.consume_scope_ready_arm(scope, 0);
                        assert!(
                            !worker.scope_has_ready_arm(scope, 0),
                            "arm-ready evidence must be one-shot once consumed"
                        );

                        worker.mark_scope_ready_arm(scope, 1);
                        assert_ne!(worker.scope_ready_arm_mask(scope), 0);
                        worker.clear_scope_evidence(scope);
                        assert_eq!(
                            worker.scope_ready_arm_mask(scope),
                            0,
                            "scope exit must clear arm-ready evidence"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn send_entry_arm_with_later_recv_does_not_require_ready_evidence_to_materialize() {
    run_offer_regression_test(
        "send_entry_arm_with_later_recv_does_not_require_ready_evidence_to_materialize",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(995);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_slot.ptr(),
                                rv_id,
                                sid,
                                &ENTRY_CONTROLLER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                    }
                    let controller = controller_slot.borrow_mut();
                    let scope = controller.cursor.node_scope_id();
                    assert!(!scope.is_none(), "controller must start at route scope");

                    let mut arm = 0u8;
                    let mut found = false;
                    while arm <= 1 {
                        if controller.arm_has_recv(scope, arm)
                            && let Some((entry, _)) =
                                controller.cursor.controller_arm_entry_by_arm(scope, arm)
                            && controller
                                .cursor
                                .try_recv_meta_at(state_index_to_usize(entry))
                                .is_none()
                        {
                            let token =
                                RouteDecisionToken::from_resolver(Arm::new(arm).expect("arm"));
                            assert!(
                                controller.route_token_has_materialization_evidence(scope, token),
                                "send/local arm entry must materialize without ready-arm evidence even when recv appears later"
                            );
                            found = true;
                            break;
                        }
                        if arm == 1 {
                            break;
                        }
                        arm += 1;
                    }
                    assert!(
                        found,
                        "expected a controller arm with send/local entry and later recv in the same arm"
                    );
                });
            });
        },
    );
}

#[test]
fn lane_offer_state_reenters_same_route_scope_using_offer_entry() {
    run_offer_regression_test(
        "lane_offer_state_reenters_same_route_scope_using_offer_entry",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(996);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_CONTROLLER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                    }
                    let controller = controller_slot.borrow_mut();
                    let scope = controller.cursor.node_scope_id();
                    assert!(!scope.is_none(), "controller must start at route scope");
                    let offer_entry = controller
                        .cursor
                        .route_scope_offer_entry(scope)
                        .expect("offer entry");
                    assert!(!offer_entry.is_max(), "test requires concrete offer entry");
                    let next_idx = state_index_to_usize(offer_entry) + 1;
                    controller.set_cursor_index(next_idx);
                    let region = controller
                        .cursor
                        .scope_region_by_id(scope)
                        .expect("route scope region");
                    assert!(
                        next_idx >= region.start && next_idx < region.end,
                        "test cursor must remain inside the same route scope"
                    );

                    controller.refresh_lane_offer_state(0);
                    assert!(
                        controller.route_state.active_offer_lanes().contains(0),
                        "lane must remain pending while re-entering the same route scope"
                    );
                    assert_eq!(
                        controller.route_state.lane_offer_state(0).entry,
                        offer_entry,
                        "lane offer state must normalize to canonical route offer_entry"
                    );
                    assert_eq!(
                        controller.offer_entry_representative_lane_idx(
                            state_index_to_usize(offer_entry),
                            controller
                                .offer_entry_state_snapshot(state_index_to_usize(offer_entry))
                                .expect("offer entry state snapshot"),
                        ),
                        Some(0),
                        "offer entry index must cache a representative lane for direct lookup"
                    );
                    assert!(
                        controller.offer_entry_has_active_lanes(state_index_to_usize(offer_entry)),
                        "offer entry index must track active lanes while the route remains pending"
                    );
                    assert_eq!(
                        controller.global_active_entries().entry_at(0),
                        Some(state_index_to_usize(offer_entry)),
                        "global active-entry index must point at the canonical offer entry"
                    );
                    controller.clear_lane_offer_state(0);
                    assert!(
                        !controller.offer_entry_has_active_lanes(state_index_to_usize(offer_entry)),
                        "clearing lane offer state must detach the lane from the offer entry index"
                    );
                    assert_eq!(
                        controller.frontier_state.offer_entry_state
                            [state_index_to_usize(offer_entry)]
                        .lane_idx,
                        u8::MAX,
                        "detaching the last lane must clear the representative lane cache"
                    );
                    assert_eq!(
                        controller.global_active_entries().occupancy_mask(),
                        0,
                        "detaching the last lane must clear the global active-entry index"
                    );
                });
            });
        },
    );
}

#[test]
fn loop_semantics_are_metadata_authority() {
    run_offer_regression_test("loop_semantics_are_metadata_authority", || {
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                let transport = HintOnlyTransport::new(HINT_NONE);
                let rv_id = cluster_ref
                    .add_rendezvous_from_config(config, transport)
                    .expect("register rendezvous");
                let sid = SessionId::new(1005);
                unsafe {
                    cluster_ref
                        .attach_endpoint_into::<0, _, _, _>(
                            controller_slot.ptr(),
                            rv_id,
                            sid,
                            &LOOP_SEMANTICS_CONTROLLER_PROGRAM(),
                            NoBinding,
                        )
                        .expect("attach controller endpoint");
                }

                let controller = controller_slot.borrow_mut();
                let scope = controller.cursor.node_scope_id();
                assert!(
                    !scope.is_none(),
                    "controller must start at loop route scope"
                );

                let continue_kind = controller_arm_semantic_kind(
                    &controller.cursor,
                    &controller.control_semantics(),
                    scope,
                    0,
                )
                .expect("continue arm semantic kind");
                let break_kind = controller_arm_semantic_kind(
                    &controller.cursor,
                    &controller.control_semantics(),
                    scope,
                    1,
                )
                .expect("break arm semantic kind");
                assert_eq!(continue_kind, ControlSemanticKind::LoopContinue);
                assert_eq!(break_kind, ControlSemanticKind::LoopBreak);
                assert_eq!(
                    loop_control_meaning_from_semantic(continue_kind),
                    Some(LoopControlMeaning::Continue)
                );
                assert_eq!(
                    loop_control_meaning_from_semantic(break_kind),
                    Some(LoopControlMeaning::Break)
                );
                assert_eq!(
                    controller.control_semantic_kind(ControlSemanticKind::LoopContinue),
                    ControlSemanticKind::LoopContinue
                );
                assert_eq!(
                    controller.control_semantic_kind(ControlSemanticKind::LoopBreak),
                    ControlSemanticKind::LoopBreak
                );
            });
        });
    });
}

#[test]
fn loop_continue_then_nested_custom_route_right_send_stays_well_scoped() {
    run_offer_regression_test(
        "loop_continue_then_nested_custom_route_right_send_stays_well_scoped",
        || {
            #[inline(never)]
            fn send_loop_continue_then_prepare_route_right(
                cx: &mut Context<'_>,
                controller_slot: &mut OfferValueSlotGuard<'_, OfferHintControllerEndpoint>,
            ) -> SendMeta {
                {
                    let controller = controller_slot.borrow_mut();
                    let mut continue_send = core::pin::pin!(CursorSend::<
                        LoopContinueScopedContinueMsg,
                    >::run(
                        controller, ()
                    ));
                    let _ = poll_ready_ok(cx, continue_send.as_mut(), "loop continue send");
                }

                let controller = controller_slot.borrow_mut();
                controller
                    .preview_flow::<LoopContinueScopedRouteRightMsg>()
                    .map(|preview| preview.into_parts().0)
                    .expect("open nested route-right send after continue")
            }

            #[inline(never)]
            fn assert_route_right_meta_after_continue(
                controller_slot: &mut OfferValueSlotGuard<'_, OfferHintControllerEndpoint>,
                route_right_meta: &SendMeta,
            ) {
                let controller = controller_slot.borrow_mut();
                let offer_lane = controller
                    .port_for_lane(route_right_meta.lane as usize)
                    .lane();
                let policy = controller
                    .control
                    .cluster()
                    .expect("cluster must remain attached")
                    .policy_mode_for(
                        RendezvousId::new(controller.rendezvous_id().raw()),
                        Lane::new(offer_lane.raw()),
                        route_right_meta.eff_index,
                        RouteHintRightKind::TAG,
                        ControlOp::RouteDecision,
                    )
                    .expect("resolve route-right policy mode");
                let controller_policy = controller
                    .cursor
                    .route_scope_controller_policy(route_right_meta.scope);

                assert!(
                    !route_right_meta.scope.is_none(),
                    "nested route-right send must stay scoped"
                );
                assert_eq!(
                    route_right_meta.route_arm,
                    Some(1),
                    "nested route-right send must preserve the selected inner arm after loop continue: meta={route_right_meta:?} policy={policy:?} controller_policy={controller_policy:?}"
                );
                let shot = route_right_meta
                    .shot
                    .expect("nested route-right send must retain shot metadata");
                assert!(
                    controller
                        .mint_descriptor_token_bytes(
                            route_right_meta.peer,
                            shot,
                            controller
                                .port_for_lane(route_right_meta.lane as usize)
                                .lane(),
                            route_right_meta.scope,
                            0,
                            crate::global::ControlDesc::of::<RouteHintRightKind>(),
                            RouteHintRightKind::encode_handle(&(1, route_right_meta.scope.raw())),
                        )
                        .is_ok(),
                    "nested route-right canonical mint must succeed after loop continue: meta={route_right_meta:?} policy={policy:?} controller_policy={controller_policy:?} cursor_idx={} node_scope={:?}",
                    controller.cursor.index(),
                    controller.cursor.node_scope_id(),
                );
            }

            #[inline(never)]
            fn send_prepared_route_right(
                cx: &mut Context<'_>,
                controller_slot: &mut OfferValueSlotGuard<'_, OfferHintControllerEndpoint>,
                route_right_meta: &SendMeta,
            ) {
                let controller = controller_slot.borrow_mut();
                let mut route_right_send = core::pin::pin!(CursorSend::<
                    LoopContinueScopedRouteRightMsg,
                >::run_with_meta(
                    controller, *route_right_meta, None,
                ));
                let _ = poll_ready_ok(
                    cx,
                    route_right_send.as_mut(),
                    "nested route-right send after loop continue",
                );
            }

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1006);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_slot.ptr(),
                                rv_id,
                                sid,
                                &LOOP_CONTINUE_SCOPED_CONTROLLER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                    }

                    let waker = noop_waker_ref();
                    let mut cx = Context::from_waker(waker);
                    let route_right_meta =
                        send_loop_continue_then_prepare_route_right(&mut cx, controller_slot);
                    assert_route_right_meta_after_continue(controller_slot, &route_right_meta);
                    send_prepared_route_right(&mut cx, controller_slot, &route_right_meta);
                });
            });
        },
    );
}

#[test]
fn send_preview_commit_clears_stale_route_hints_on_selected_lane() {
    run_offer_regression_test(
        "send_preview_commit_clears_stale_route_hints_on_selected_lane",
        || {
            const STALE_FRAME_HINT: u8 = 77;
            let stale_frame_hint_mask = FrameLabelMask::from_frame_label(STALE_FRAME_HINT);

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1008);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_slot.ptr(),
                                rv_id,
                                sid,
                                &LOOP_CONTINUE_SCOPED_CONTROLLER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                    }

                    let waker = noop_waker_ref();
                    let mut cx = Context::from_waker(waker);
                    let route_right_meta = {
                        let controller = controller_slot.borrow_mut();
                        {
                            let mut continue_send = core::pin::pin!(CursorSend::<
                                LoopContinueScopedContinueMsg,
                            >::run(
                                controller, ()
                            ));
                            let _ = poll_ready_ok(
                                &mut cx,
                                continue_send.as_mut(),
                                "loop continue send",
                            );
                        }

                        controller
                            .preview_flow::<LoopContinueScopedRouteRightMsg>()
                            .map(|preview| preview.into_parts().0)
                            .expect("open nested route-right send after continue")
                    };

                    let controller = controller_slot.borrow_mut();
                    let lane = controller
                        .port_for_lane(route_right_meta.lane as usize)
                        .lane();
                    assert!(
                        !controller
                            .port_for_lane(route_right_meta.lane as usize)
                            .route_table()
                            .has_pending_frame_hint_for_lane(lane, stale_frame_hint_mask),
                        "test must start without stale route hints on the selected lane"
                    );
                    controller
                        .port_for_lane(route_right_meta.lane as usize)
                        .route_table()
                        .update_pending_frame_hint_mask_for_lane(
                            lane,
                            FrameLabelMask::EMPTY,
                            stale_frame_hint_mask,
                        );
                    assert!(
                        controller
                            .port_for_lane(route_right_meta.lane as usize)
                            .route_table()
                            .has_pending_frame_hint_for_lane(lane, stale_frame_hint_mask),
                        "stale route hint must be staged before the send commit"
                    );

                    {
                        let mut route_right_send = core::pin::pin!(CursorSend::<
                            LoopContinueScopedRouteRightMsg,
                        >::run_with_meta(
                            controller,
                            route_right_meta,
                            None
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right_send.as_mut(),
                            "nested route-right send clears stale route hints",
                        );
                    }

                    assert!(
                        !controller
                            .port_for_lane(route_right_meta.lane as usize)
                            .route_table()
                            .has_pending_frame_hint_for_lane(lane, stale_frame_hint_mask),
                        "send commit must clear offer-scoped route hints on the selected lane"
                    );
                });
            });
        },
    );
}

#[test]
fn send_preview_commits_ack_route_bookkeeping_on_flow_send() {
    run_offer_regression_test(
        "send_preview_commits_ack_route_bookkeeping_on_flow_send",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(1007);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_slot.ptr(),
                                rv_id,
                                sid,
                                &LOOP_CONTINUE_SCOPED_CONTROLLER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                    }

                    let waker = noop_waker_ref();
                    let mut cx = Context::from_waker(waker);

                    {
                        let controller = controller_slot.borrow_mut();
                        let mut continue_send = core::pin::pin!(CursorSend::<
                            LoopContinueScopedContinueMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ =
                            poll_ready_ok(&mut cx, continue_send.as_mut(), "loop continue send");
                    }

                    let controller = controller_slot.borrow_mut();
                    let scope = controller.cursor.node_scope_id();
                    assert!(
                        !scope.is_none(),
                        "controller must enter the nested route scope"
                    );

                    controller.mark_scope_ready_arm(scope, 1);
                    controller.record_scope_ack(
                        scope,
                        RouteDecisionToken::from_ack(Arm::new(1).expect("valid selected arm")),
                    );
                    assert!(
                        controller.scope_has_ready_arm_evidence(scope),
                        "test requires pending scope evidence before send-arm commit"
                    );

                    {
                        let mut route_right_send = core::pin::pin!(CursorSend::<
                            LoopContinueScopedRouteRightMsg,
                        >::run(
                            controller, ()
                        ));
                        let _ = poll_ready_ok(
                            &mut cx,
                            route_right_send.as_mut(),
                            "send right arm after preview",
                        );
                    }

                    assert!(
                        !controller.scope_has_ready_arm_evidence(scope),
                        "flow().send() must clear ready-arm scope evidence after consuming the preview"
                    );

                    let saw_ack_route_decision = OFFER_TEST_TAP.with(|tap| unsafe {
                        (&*tap.get()).iter().copied().any(|event| {
                            event.id == crate::observe::ids::ROUTE_DECISION
                                && event.arg0 == sid.raw()
                                && (event.arg1 & 0xFFFF) == 1
                                && event.causal_seq() == RouteDecisionSource::Ack.as_tap_seq()
                        })
                    });
                    assert!(
                        saw_ack_route_decision,
                        "flow().send() must emit Ack route-decision observability when it consumes a send preview"
                    );
                });
            });
        },
    );
}

#[test]
fn passive_offer_descends_into_nested_route_after_loop_continue_and_custom_route_right() {
    run_offer_regression_test(
        "passive_offer_descends_into_nested_route_after_loop_continue_and_custom_route_right",
        || {
            #[inline(never)]
            fn send_continue_and_route_right(
                cx: &mut Context<'_>,
                controller_slot: &mut OfferValueSlotGuard<'_, OfferHintControllerEndpoint>,
            ) {
                {
                    let controller = controller_slot.borrow_mut();
                    let mut continue_send = core::pin::pin!(CursorSend::<
                        LoopContinueScopedContinueMsg,
                    >::run(
                        controller, ()
                    ));
                    let _ = poll_ready_ok(cx, continue_send.as_mut(), "loop continue send");
                }
                {
                    let controller = controller_slot.borrow_mut();
                    let mut route_right_send = core::pin::pin!(CursorSend::<
                        LoopContinueScopedRouteRightMsg,
                    >::run(
                        controller, ()
                    ));
                    let _ = poll_ready_ok(cx, route_right_send.as_mut(), "nested route-right send");
                }
            }

            #[inline(never)]
            fn poll_passive_nested_offer(
                cx: &mut Context<'_>,
                worker_slot: &mut OfferValueSlotGuard<'_, OfferHintWorkerBindingEndpoint>,
            ) -> u8 {
                let worker = worker_slot.borrow_mut();
                let outer_scope = worker.cursor.node_scope_id();
                let outer_ack = worker.peek_scope_ack(outer_scope);
                let outer_ready_mask = worker.scope_ready_arm_mask(outer_scope);
                let outer_poll_ready_mask = worker.scope_poll_ready_arm_mask(outer_scope);
                let mut offer = pin!(cursor_offer(worker));
                let branch = match offer.as_mut().poll(cx) {
                    Poll::Ready(Ok(branch)) => branch,
                    Poll::Ready(Err(err)) => panic!(
                        "passive nested offer failed: {err:?}; outer_scope={outer_scope:?} ack={:?} ready_mask=0b{:02b} poll_ready_mask=0b{:02b}",
                        outer_ack, outer_ready_mask, outer_poll_ready_mask,
                    ),
                    Poll::Pending => match offer.as_mut().poll(cx) {
                        Poll::Ready(Ok(branch)) => branch,
                        Poll::Ready(Err(err)) => panic!(
                            "passive nested offer failed after retry: {err:?}; outer_scope={outer_scope:?} ack={:?} ready_mask=0b{:02b} poll_ready_mask=0b{:02b}",
                            outer_ack, outer_ready_mask, outer_poll_ready_mask,
                        ),
                        Poll::Pending => panic!("passive nested offer remained pending"),
                    },
                };
                branch_label(&branch)
            }

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerBindingEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1007);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &LOOP_CONTINUE_PASSIVE_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &LOOP_CONTINUE_PASSIVE_WORKER_PROGRAM(),
                                    TestBinding::default(),
                                )
                                .expect("attach worker endpoint");
                        }
                        {
                            let worker = worker_slot.borrow_mut();
                            let right_reply_frame = frame_label_for_cursor_label(
                                &worker.cursor,
                                LOOP_CONTINUE_PASSIVE_RIGHT_REPLY_LABEL,
                            );
                            worker.binding.incoming.push_back(IngressEvidence {
                                frame_label: FrameLabel::new(right_reply_frame),
                                instance: 1,
                                has_fin: false,
                                channel: Channel::new(7),
                            });
                        }

                        let waker = noop_waker_ref();
                        let mut cx = Context::from_waker(waker);
                        send_continue_and_route_right(&mut cx, controller_slot);
                        let label = poll_passive_nested_offer(&mut cx, worker_slot);
                        assert_eq!(
                            label, LOOP_CONTINUE_PASSIVE_RIGHT_REPLY_LABEL,
                            "passive offer must descend into the nested right arm after continue + route-right"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn loop_continue_request_then_triple_nested_reply_route_keeps_client_offer_and_server_offer_valid()
{
    run_offer_regression_test(
        "loop_continue_request_then_triple_nested_reply_route_keeps_client_offer_and_server_offer_valid",
        || {
            type LoopContinueMsg = Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
                crate::control::cap::resource_kinds::LoopContinueKind,
            >;
            type LoopBreakMsg = Msg<
                { TEST_LOOP_BREAK_LOGICAL },
                GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
                crate::control::cap::resource_kinds::LoopBreakKind,
            >;
            type SessionRequestWireMsg = Msg<0x10, u8>;
            type AdminReplyMsg = Msg<0x50, u8>;
            type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
            type SnapshotRejectedReplyMsg = Msg<0x52, u8>;
            type CommitCandidatesReplyMsg = Msg<0x53, u8>;
            type CommitFinalReplyMsg = Msg<0x55, u8>;
            type CheckpointMsg = Msg<
                { SNAPSHOT_CONTROL_LOGICAL },
                GenericCapToken<SnapshotControl>,
                SnapshotControl,
            >;
            type SessionCancelControlMsg =
                Msg<{ ABORT_CONTROL_LOGICAL }, GenericCapToken<AbortControl>, AbortControl>;
            type StaticRouteLeftMsg = Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >;
            type StaticRouteRightMsg = Msg<
                ROUTE_HINT_RIGHT_LABEL,
                GenericCapToken<RouteHintRightKind>,
                RouteHintRightKind,
            >;
            type SnapshotReplyLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                >,
            >;
            type SnapshotReplyRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, SnapshotRejectedReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
                >,
            >;
            type SnapshotReplyDecisionSteps =
                BranchSteps<SnapshotReplyLeftSteps, SnapshotReplyRightSteps>;
            type CommitReplyLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, CommitCandidatesReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                >,
            >;
            type CommitReplyRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, CommitFinalReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
                >,
            >;
            type CommitReplyDecisionSteps =
                BranchSteps<CommitReplyLeftSteps, CommitReplyRightSteps>;
            type ReplyDecisionLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
            >;
            type ReplyDecisionNestedLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SnapshotReplyDecisionSteps,
            >;
            type ReplyDecisionNestedRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                CommitReplyDecisionSteps,
            >;
            type ReplyDecisionNestedSteps =
                BranchSteps<ReplyDecisionNestedLeftSteps, ReplyDecisionNestedRightSteps>;
            type ReplyDecisionRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                ReplyDecisionNestedSteps,
            >;
            type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
            type RequestExchangeSteps =
                SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
            type ContinueArmSteps =
                SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
            type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
            type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

            let snapshot_reply_decision: g::Program<SnapshotReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                    ),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, SnapshotRejectedReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                    ),
                ),
            );
            let commit_reply_decision: g::Program<CommitReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, CommitCandidatesReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                    ),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, CommitFinalReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                    ),
                ),
            );
            let reply_decision: g::Program<ReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::route(
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                            snapshot_reply_decision,
                        ),
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                            commit_reply_decision,
                        ),
                    ),
                ),
            );
            let request_exchange: g::Program<RequestExchangeSteps> = g::seq(
                g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
                reply_decision,
            );
            let loop_program: g::Program<LoopProgramSteps> = g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
                    request_exchange,
                ),
                g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
            );
            let client_program: RoleProgram<0> = project(&loop_program);
            let server_program: RoleProgram<1> = project(&loop_program);
            type ClientEndpoint = CursorEndpoint<
                'static,
                0,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                TestBinding,
            >;
            type ServerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;

            #[inline(never)]
            fn client_send_request(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
                payload: u8,
                continue_context: &str,
                request_context: &str,
            ) {
                let client = client_slot.borrow_mut();
                {
                    let mut continue_send =
                        core::pin::pin!(CursorSend::<LoopContinueMsg>::run(client, ()));
                    let _ = poll_ready_ok(cx, continue_send.as_mut(), continue_context);
                }
                {
                    let mut request_send =
                        core::pin::pin!(CursorSend::<SessionRequestWireMsg>::run(client, &payload));
                    let _ = poll_ready_ok(cx, request_send.as_mut(), request_context);
                }
            }

            #[inline(never)]
            fn server_reply_snapshot_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut server_offer = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, server_offer.as_mut(), "server request offer")
                };
                assert_eq!(
                    branch_label(&branch),
                    0x10,
                    "server must first observe the request"
                );
                {
                    let mut server_decode =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let request =
                        poll_ready_ok(cx, server_decode.as_mut(), "server request decode");
                    core::hint::black_box(request);
                }
                {
                    let mut send_outer_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "outer reply route-right send",
                    );
                }
                {
                    let mut send_category_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ =
                        poll_ready_ok(cx, send_category_left.as_mut(), "category route-left send");
                }
                {
                    let mut send_snapshot_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ =
                        poll_ready_ok(cx, send_snapshot_left.as_mut(), "snapshot route-left send");
                }
                {
                    let mut reply_send = core::pin::pin!(
                        CursorSend::<SnapshotCandidatesReplyMsg>::run(server, &reply_payload)
                    );
                    let _ =
                        poll_ready_ok(cx, reply_send.as_mut(), "snapshot candidates reply send");
                }
            }

            #[inline(never)]
            fn client_decode_snapshot_reply_and_checkpoint(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let reply_branch = {
                    let mut client_offer = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(cx, client_offer.as_mut(), "client snapshot reply offer")
                };
                assert_eq!(
                    branch_label(&reply_branch),
                    0x51,
                    "client must materialize the selected snapshot candidates reply label"
                );
                let reply_branch_scope = branch_scope(&reply_branch);
                {
                    let mut client_decode = core::pin::pin!(CursorDecode::<
                        SnapshotCandidatesReplyMsg,
                    >::run(
                        client, reply_branch
                    ));
                    let reply =
                        poll_ready_ok(cx, client_decode.as_mut(), "client snapshot reply decode");
                    core::hint::black_box(reply);
                }
                {
                    let mut checkpoint_send =
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, ()));
                    let _ = poll_ready_ok(
                        cx,
                        checkpoint_send.as_mut(),
                        "client checkpoint control send",
                    );
                }
                assert_eq!(
                    client.selected_arm_for_scope(reply_branch_scope),
                    None,
                    "completed non-linger branch scope must not survive into next loop iteration",
                );
            }

            #[inline(never)]
            fn server_reply_commit_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut server_offer = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, server_offer.as_mut(), "server commit request offer")
                };
                assert_eq!(
                    branch_label(&branch),
                    0x10,
                    "server must observe the second request"
                );
                {
                    let mut server_decode =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let request =
                        poll_ready_ok(cx, server_decode.as_mut(), "server commit request decode");
                    core::hint::black_box(request);
                }
                {
                    let mut send_outer_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "outer commit route-right send",
                    );
                }
                {
                    let mut send_category_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_category_right.as_mut(),
                        "category commit route-right send",
                    );
                }
                {
                    let mut send_commit_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_commit_left.as_mut(),
                        "commit reply route-left send",
                    );
                }
                {
                    let mut commit_reply_send = core::pin::pin!(CursorSend::<
                        CommitCandidatesReplyMsg,
                    >::run(
                        server, &reply_payload
                    ));
                    let _ = poll_ready_ok(
                        cx,
                        commit_reply_send.as_mut(),
                        "commit candidates reply send",
                    );
                }
            }

            #[inline(never)]
            fn client_decode_commit_reply_and_checkpoint(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let commit_branch = {
                    let mut client_offer = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(cx, client_offer.as_mut(), "client commit reply offer")
                };
                assert_eq!(
                    branch_label(&commit_branch),
                    0x53,
                    "client must materialize the selected commit candidates reply label"
                );
                {
                    let mut client_decode = core::pin::pin!(
                        CursorDecode::<CommitCandidatesReplyMsg>::run(client, commit_branch)
                    );
                    let reply =
                        poll_ready_ok(cx, client_decode.as_mut(), "client commit reply decode");
                    core::hint::black_box(reply);
                }
                {
                    let mut checkpoint_send =
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, ()));
                    let _ = poll_ready_ok(
                        cx,
                        checkpoint_send.as_mut(),
                        "client post-commit checkpoint send",
                    );
                }
            }

            #[inline(never)]
            fn server_offer_stays_pending(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
            ) {
                let server = server_slot.borrow_mut();
                {
                    let mut server_next_offer = core::pin::pin!(cursor_offer(server));
                    match server_next_offer.as_mut().poll(cx) {
                        Poll::Ready(Err(err)) => {
                            panic!("server next offer after commit path must not fail: {err:?}")
                        }
                        Poll::Ready(Ok(branch)) => panic!(
                            "server next offer after commit path must not spuriously materialize a branch: label={}",
                            branch_label(&branch)
                        ),
                        Poll::Pending => {}
                    }
                }
            }

            offer_fixture!(4096, clock, config);
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(ClientEndpoint, client_slot, {
                        with_offer_value_slot!(ServerEndpoint, server_slot, {
                            let transport = HintOnlyTransport::new(HINT_NONE);
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1008);
                            let reply_payload = 0x51u8;
                            let commit_reply_payload = 0x53u8;
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        client_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &client_program,
                                        TestBinding::with_incoming_and_payloads(
                                            &[],
                                            &[&[reply_payload], &[commit_reply_payload]],
                                        ),
                                    )
                                    .expect("attach client endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        server_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &server_program,
                                        NoBinding,
                                    )
                                    .expect("attach server endpoint");
                            }
                            {
                                let client = client_slot.borrow_mut();
                                let snapshot_reply_frame =
                                    frame_label_for_cursor_label(&client.cursor, 0x51);
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(snapshot_reply_frame),
                                    instance: 11,
                                    has_fin: false,
                                    channel: Channel::new(9),
                                });
                            }

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);

                            client_send_request(
                                &mut cx,
                                client_slot,
                                7,
                                "client continue send",
                                "client request send",
                            );
                            server_reply_snapshot_request(&mut cx, server_slot, reply_payload);
                            client_decode_snapshot_reply_and_checkpoint(&mut cx, client_slot);
                            client_send_request(
                                &mut cx,
                                client_slot,
                                8,
                                "client second continue send",
                                "client commit request send",
                            );
                            server_reply_commit_request(&mut cx, server_slot, commit_reply_payload);
                            client_decode_commit_reply_and_checkpoint(&mut cx, client_slot);
                            server_offer_stays_pending(&mut cx, server_slot);
                        });
                    });
                }
            );
        },
    );
}

#[test]
fn admin_reply_then_snapshot_reply_right_path_survives_next_iteration() {
    run_offer_regression_test(
        "admin_reply_then_snapshot_reply_right_path_survives_next_iteration",
        || {
            type LoopContinueMsg = Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
                crate::control::cap::resource_kinds::LoopContinueKind,
            >;
            type LoopBreakMsg = Msg<
                { TEST_LOOP_BREAK_LOGICAL },
                GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
                crate::control::cap::resource_kinds::LoopBreakKind,
            >;
            type SessionRequestWireMsg = Msg<0x10, u8>;
            type AdminReplyMsg = Msg<0x50, u8>;
            type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
            type CheckpointMsg = Msg<
                { SNAPSHOT_CONTROL_LOGICAL },
                GenericCapToken<SnapshotControl>,
                SnapshotControl,
            >;
            type StaticRouteLeftMsg = Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >;
            type StaticRouteRightMsg = Msg<
                ROUTE_HINT_RIGHT_LABEL,
                GenericCapToken<RouteHintRightKind>,
                RouteHintRightKind,
            >;
            type ReplyDecisionLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
            >;
            type SnapshotReplyPathSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                    SeqSteps<
                        SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
                        SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                    >,
                >,
            >;
            type ReplyDecisionRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                SnapshotReplyPathSteps,
            >;
            type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
            type RequestExchangeSteps =
                SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
            type ContinueArmSteps =
                SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
            type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
            type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

            let reply_decision: g::Program<ReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                            g::seq(
                                g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                                g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                            ),
                        ),
                    ),
                ),
            );
            let request_exchange: g::Program<RequestExchangeSteps> = g::seq(
                g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
                reply_decision,
            );
            let loop_program: g::Program<LoopProgramSteps> = g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
                    request_exchange,
                ),
                g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
            );
            let client_program: RoleProgram<0> = project(&loop_program);
            let server_program: RoleProgram<1> = project(&loop_program);
            type ClientEndpoint = CursorEndpoint<
                'static,
                0,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                TestBinding,
            >;
            type ServerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;

            #[inline(never)]
            fn client_send_admin_request(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                {
                    let mut send_continue =
                        core::pin::pin!(CursorSend::<LoopContinueMsg>::run(client, ()));
                    let _ = poll_ready_ok(cx, send_continue.as_mut(), "client continue send");
                }
                {
                    let mut send_request =
                        core::pin::pin!(CursorSend::<SessionRequestWireMsg>::run(client, &1u8));
                    let _ = poll_ready_ok(cx, send_request.as_mut(), "client admin request send");
                }
            }

            #[inline(never)]
            fn server_reply_admin_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                admin_reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut offer_request = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, offer_request.as_mut(), "server admin request offer")
                };
                assert_eq!(
                    branch_label(&branch),
                    0x10,
                    "server must first observe the admin request"
                );
                {
                    let mut decode_request =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let _ =
                        poll_ready_ok(cx, decode_request.as_mut(), "server admin request decode");
                }
                {
                    let mut send_route_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(cx, send_route_left.as_mut(), "admin route-left send");
                }
                {
                    let mut send_reply = core::pin::pin!(CursorSend::<AdminReplyMsg>::run(
                        server,
                        &admin_reply_payload
                    ));
                    let _ = poll_ready_ok(cx, send_reply.as_mut(), "admin reply send");
                }
            }

            #[inline(never)]
            fn client_decode_admin_reply(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let admin_branch = {
                    let mut offer_reply = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(cx, offer_reply.as_mut(), "client admin reply offer")
                };
                assert_eq!(
                    branch_label(&admin_branch),
                    0x50,
                    "client must materialize the admin reply"
                );
                let admin_reply_scope = branch_scope(&admin_branch);
                {
                    let mut decode_reply =
                        core::pin::pin!(CursorDecode::<AdminReplyMsg>::run(client, admin_branch));
                    let _ = poll_ready_ok(cx, decode_reply.as_mut(), "client admin reply decode");
                }
                assert_eq!(
                    client.selected_arm_for_scope(admin_reply_scope),
                    None,
                    "admin reply branch scope must not survive into the next loop iteration"
                );
            }

            #[inline(never)]
            fn drive_admin_round(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                admin_reply_payload: u8,
            ) {
                client_send_admin_request(cx, client_slot);
                server_reply_admin_request(cx, server_slot, admin_reply_payload);
                client_decode_admin_reply(cx, client_slot);
            }

            #[inline(never)]
            fn client_send_snapshot_request(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                {
                    let mut send_continue =
                        core::pin::pin!(CursorSend::<LoopContinueMsg>::run(client, ()));
                    let _ =
                        poll_ready_ok(cx, send_continue.as_mut(), "client snapshot continue send");
                }
                {
                    let mut send_request =
                        core::pin::pin!(CursorSend::<SessionRequestWireMsg>::run(client, &2u8));
                    let _ =
                        poll_ready_ok(cx, send_request.as_mut(), "client snapshot request send");
                }
            }

            #[inline(never)]
            fn server_reply_snapshot_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                snapshot_reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut offer_request = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, offer_request.as_mut(), "server snapshot request offer")
                };
                assert_eq!(
                    branch_label(&branch),
                    0x10,
                    "server must observe the snapshot request"
                );
                {
                    let mut decode_request =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let _ = poll_ready_ok(
                        cx,
                        decode_request.as_mut(),
                        "server snapshot request decode",
                    );
                }
                {
                    let mut send_outer_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "snapshot outer route-right send",
                    );
                }
                {
                    let mut send_category_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_category_left.as_mut(),
                        "snapshot category route-left send",
                    );
                }
                {
                    let mut send_reply_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_reply_left.as_mut(),
                        "snapshot reply route-left send",
                    );
                }
                {
                    let mut send_snapshot_reply =
                        core::pin::pin!(CursorSend::<SnapshotCandidatesReplyMsg>::run(
                            server,
                            &snapshot_reply_payload
                        ));
                    let _ = poll_ready_ok(
                        cx,
                        send_snapshot_reply.as_mut(),
                        "snapshot candidates reply send",
                    );
                }
            }

            #[inline(never)]
            fn client_decode_snapshot_reply_and_checkpoint(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let snapshot_branch = {
                    let mut offer_reply = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(
                        cx,
                        offer_reply.as_mut(),
                        "client snapshot reply offer after admin path",
                    )
                };
                assert_eq!(
                    branch_label(&snapshot_branch),
                    0x51,
                    "snapshot reply must still materialize after an earlier admin-left iteration"
                );
                {
                    let mut decode_reply = core::pin::pin!(CursorDecode::<
                        SnapshotCandidatesReplyMsg,
                    >::run(
                        client, snapshot_branch
                    ));
                    let _ = poll_ready_ok(
                        cx,
                        decode_reply.as_mut(),
                        "client snapshot reply decode after admin path",
                    );
                }
                {
                    let mut send_checkpoint =
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_checkpoint.as_mut(),
                        "client snapshot checkpoint send after admin path",
                    );
                }
            }

            #[inline(never)]
            fn drive_snapshot_round(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                snapshot_reply_payload: u8,
            ) {
                client_send_snapshot_request(cx, client_slot);
                server_reply_snapshot_request(cx, server_slot, snapshot_reply_payload);
                client_decode_snapshot_reply_and_checkpoint(cx, client_slot);
            }

            offer_fixture!(4096, clock, config);
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(ClientEndpoint, client_slot, {
                        with_offer_value_slot!(ServerEndpoint, server_slot, {
                            let transport = HintOnlyTransport::new(HINT_NONE);
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1010);
                            let admin_reply_payload = 0x50u8;
                            let snapshot_reply_payload = 0x51u8;
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        client_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &client_program,
                                        TestBinding::with_incoming_and_payloads(
                                            &[],
                                            &[&[admin_reply_payload], &[snapshot_reply_payload]],
                                        ),
                                    )
                                    .expect("attach client endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        server_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &server_program,
                                        NoBinding,
                                    )
                                    .expect("attach server endpoint");
                            }
                            {
                                let client = client_slot.borrow_mut();
                                let admin_reply_frame =
                                    frame_label_for_cursor_label(&client.cursor, 0x50);
                                let snapshot_reply_frame =
                                    frame_label_for_cursor_label(&client.cursor, 0x51);
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(admin_reply_frame),
                                    instance: 21,
                                    has_fin: false,
                                    channel: Channel::new(13),
                                });
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(snapshot_reply_frame),
                                    instance: 22,
                                    has_fin: false,
                                    channel: Channel::new(14),
                                });
                            }
                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);
                            drive_admin_round(
                                &mut cx,
                                client_slot,
                                server_slot,
                                admin_reply_payload,
                            );
                            drive_snapshot_round(
                                &mut cx,
                                client_slot,
                                server_slot,
                                snapshot_reply_payload,
                            );
                        });
                    });
                }
            );
        },
    );
}

#[test]
fn snapshot_then_commit_final_reply_survives_next_iteration() {
    run_offer_regression_test(
        "snapshot_then_commit_final_reply_survives_next_iteration",
        || {
            type LoopContinueMsg = Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
                crate::control::cap::resource_kinds::LoopContinueKind,
            >;
            type LoopBreakMsg = Msg<
                { TEST_LOOP_BREAK_LOGICAL },
                GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
                crate::control::cap::resource_kinds::LoopBreakKind,
            >;
            type SessionRequestWireMsg = Msg<0x10, u8>;
            type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
            type CommitCandidatesReplyMsg = Msg<0x53, u8>;
            type CommitRejectedReplyMsg = Msg<0x54, u8>;
            type CommitFinalReplyMsg = Msg<0x55, u8>;
            type CheckpointMsg = Msg<
                { SNAPSHOT_CONTROL_LOGICAL },
                GenericCapToken<SnapshotControl>,
                SnapshotControl,
            >;
            type SessionCancelControlMsg =
                Msg<{ ABORT_CONTROL_LOGICAL }, GenericCapToken<AbortControl>, AbortControl>;
            type StaticRouteLeftMsg = Msg<
                { TEST_ROUTE_DECISION_LOGICAL },
                GenericCapToken<RouteDecisionKind>,
                RouteDecisionKind,
            >;
            type StaticRouteRightMsg = Msg<
                ROUTE_HINT_RIGHT_LABEL,
                GenericCapToken<RouteHintRightKind>,
                RouteHintRightKind,
            >;
            type SnapshotRejectedReplyMsg = Msg<0x52, u8>;
            type AdminReplyMsg = Msg<0x50, u8>;
            type SnapshotReplyLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                >,
            >;
            type SnapshotReplyRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, SnapshotRejectedReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
                >,
            >;
            type SnapshotReplyDecisionSteps =
                BranchSteps<SnapshotReplyLeftSteps, SnapshotReplyRightSteps>;
            type CommitRejectedBranchSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, CommitRejectedReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
                >,
            >;
            type CommitFinalBranchSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, CommitFinalReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, SessionCancelControlMsg>,
                >,
            >;
            type CommitNestedDecisionSteps =
                BranchSteps<CommitRejectedBranchSteps, CommitFinalBranchSteps>;
            type CommitReplyLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, CommitCandidatesReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                >,
            >;
            type CommitReplyRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                CommitNestedDecisionSteps,
            >;
            type CommitReplyDecisionSteps =
                BranchSteps<CommitReplyLeftSteps, CommitReplyRightSteps>;
            type ReplyDecisionLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
            >;
            type ReplyDecisionNestedLeftSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SnapshotReplyDecisionSteps,
            >;
            type ReplyDecisionNestedRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                CommitReplyDecisionSteps,
            >;
            type ReplyDecisionNestedSteps =
                BranchSteps<ReplyDecisionNestedLeftSteps, ReplyDecisionNestedRightSteps>;
            type ReplyDecisionRightSteps = SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>,
                ReplyDecisionNestedSteps,
            >;
            type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
            type RequestExchangeSteps =
                SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
            type ContinueArmSteps =
                SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
            type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
            type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

            let snapshot_reply_decision: g::Program<SnapshotReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                    ),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, Msg<0x52, u8>, 3>(),
                        g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                    ),
                ),
            );
            let commit_reply_decision: g::Program<CommitReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, CommitCandidatesReplyMsg, 3>(),
                        g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                    ),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::route(
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                            g::seq(
                                g::send::<Role<1>, Role<0>, CommitRejectedReplyMsg, 3>(),
                                g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                            ),
                        ),
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                            g::seq(
                                g::send::<Role<1>, Role<0>, CommitFinalReplyMsg, 3>(),
                                g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                            ),
                        ),
                    ),
                ),
            );
            let reply_decision: g::Program<ReplyDecisionSteps> = g::route(
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::send::<Role<1>, Role<0>, Msg<0x50, u8>, 3>(),
                ),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                    g::route(
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                            snapshot_reply_decision,
                        ),
                        g::seq(
                            g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                            commit_reply_decision,
                        ),
                    ),
                ),
            );
            let request_exchange: g::Program<RequestExchangeSteps> = g::seq(
                g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
                reply_decision,
            );
            let loop_program: g::Program<LoopProgramSteps> = g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
                    request_exchange,
                ),
                g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
            );
            let client_program: RoleProgram<0> = project(&loop_program);
            let server_program: RoleProgram<1> = project(&loop_program);
            type ClientEndpoint = CursorEndpoint<
                'static,
                0,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                TestBinding,
            >;
            type ServerEndpoint = CursorEndpoint<
                'static,
                1,
                HintOnlyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
                4,
                crate::control::cap::mint::MintConfig,
                NoBinding,
            >;

            #[inline(never)]
            fn client_send_request(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
                payload: u8,
                continue_context: &str,
                request_context: &str,
            ) {
                let client = client_slot.borrow_mut();
                {
                    let mut continue_send =
                        core::pin::pin!(CursorSend::<LoopContinueMsg>::run(client, ()));
                    let _ = poll_ready_ok(cx, continue_send.as_mut(), continue_context);
                }
                {
                    let mut request_send =
                        core::pin::pin!(CursorSend::<SessionRequestWireMsg>::run(client, &payload));
                    let _ = poll_ready_ok(cx, request_send.as_mut(), request_context);
                }
            }

            #[inline(never)]
            fn server_reply_snapshot_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut server_offer = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, server_offer.as_mut(), "server first request offer")
                };
                assert_eq!(branch_label(&branch), 0x10);
                {
                    let mut server_decode =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let request =
                        poll_ready_ok(cx, server_decode.as_mut(), "server first request decode");
                    core::hint::black_box(request);
                }
                {
                    let mut send_outer_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "first outer route-right send",
                    );
                }
                {
                    let mut send_category_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_category_left.as_mut(),
                        "first category route-left send",
                    );
                }
                {
                    let mut send_snapshot_left =
                        core::pin::pin!(CursorSend::<StaticRouteLeftMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_snapshot_left.as_mut(),
                        "first snapshot route-left send",
                    );
                }
                {
                    let mut reply_send = core::pin::pin!(
                        CursorSend::<SnapshotCandidatesReplyMsg>::run(server, &reply_payload)
                    );
                    let _ = poll_ready_ok(cx, reply_send.as_mut(), "first snapshot reply send");
                }
            }

            #[inline(never)]
            fn client_decode_snapshot_reply_and_checkpoint(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let branch = {
                    let mut client_offer = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(cx, client_offer.as_mut(), "client first offer")
                };
                assert_eq!(branch_label(&branch), 0x51);
                let branch_scope = branch_scope(&branch);
                {
                    let mut client_decode =
                        core::pin::pin!(CursorDecode::<SnapshotCandidatesReplyMsg>::run(
                            client, branch
                        ));
                    let reply = poll_ready_ok(cx, client_decode.as_mut(), "client first decode");
                    core::hint::black_box(reply);
                }
                {
                    let mut checkpoint_send =
                        core::pin::pin!(CursorSend::<CheckpointMsg>::run(client, ()));
                    let _ = poll_ready_ok(cx, checkpoint_send.as_mut(), "snapshot checkpoint send");
                }
                assert_eq!(
                    client.selected_arm_for_scope(branch_scope),
                    None,
                    "completed snapshot branch scope must not survive into the next iteration"
                );
            }

            #[inline(never)]
            fn server_reply_commit_final_request(
                cx: &mut Context<'_>,
                server_slot: &mut OfferValueSlotGuard<'_, ServerEndpoint>,
                reply_payload: u8,
            ) {
                let server = server_slot.borrow_mut();
                let branch = {
                    let mut server_offer = core::pin::pin!(cursor_offer(server));
                    poll_ready_ok(cx, server_offer.as_mut(), "server second request offer")
                };
                assert_eq!(branch_label(&branch), 0x10);
                {
                    let mut server_decode =
                        core::pin::pin!(CursorDecode::<SessionRequestWireMsg>::run(server, branch));
                    let request =
                        poll_ready_ok(cx, server_decode.as_mut(), "server second request decode");
                    core::hint::black_box(request);
                }
                {
                    let mut send_outer_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_outer_right.as_mut(),
                        "second outer route-right send",
                    );
                }
                {
                    let mut send_category_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_category_right.as_mut(),
                        "second category route-right send",
                    );
                }
                {
                    let mut send_commit_tail_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_commit_tail_right.as_mut(),
                        "second commit tail route-right send",
                    );
                }
                {
                    let mut send_commit_final_right =
                        core::pin::pin!(CursorSend::<StaticRouteRightMsg>::run(server, ()));
                    let _ = poll_ready_ok(
                        cx,
                        send_commit_final_right.as_mut(),
                        "second commit final route-right send",
                    );
                }
                {
                    let mut reply_send = core::pin::pin!(CursorSend::<CommitFinalReplyMsg>::run(
                        server,
                        &reply_payload
                    ));
                    let _ =
                        poll_ready_ok(cx, reply_send.as_mut(), "second commit final reply send");
                }
            }

            #[inline(never)]
            fn client_decode_commit_final_reply_and_cancel(
                cx: &mut Context<'_>,
                client_slot: &mut OfferValueSlotGuard<'_, ClientEndpoint>,
            ) {
                let client = client_slot.borrow_mut();
                let branch = {
                    let mut client_offer = core::pin::pin!(cursor_offer(client));
                    poll_ready_ok(cx, client_offer.as_mut(), "client second offer")
                };
                assert_eq!(branch_label(&branch), 0x55);
                {
                    let mut client_decode =
                        core::pin::pin!(CursorDecode::<CommitFinalReplyMsg>::run(client, branch));
                    let reply = poll_ready_ok(cx, client_decode.as_mut(), "client second decode");
                    core::hint::black_box(reply);
                }
                {
                    let mut cancel_send =
                        core::pin::pin!(CursorSend::<SessionCancelControlMsg>::run(client, ()));
                    let _ = poll_ready_ok(cx, cancel_send.as_mut(), "commit final cancel send");
                }
            }

            offer_fixture!(4096, clock, config);
            with_offer_cluster!(
                clock,
                SessionCluster<'static, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
                cluster_ref,
                {
                    with_offer_value_slot!(ClientEndpoint, client_slot, {
                        with_offer_value_slot!(ServerEndpoint, server_slot, {
                            let transport = HintOnlyTransport::new(HINT_NONE);
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1012);
                            let snapshot_reply_payload = 0x51u8;
                            let commit_final_payload = 0x55u8;
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        client_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &client_program,
                                        TestBinding::with_incoming_and_payloads(
                                            &[],
                                            &[&[snapshot_reply_payload], &[commit_final_payload]],
                                        ),
                                    )
                                    .expect("attach client endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        server_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &server_program,
                                        NoBinding,
                                    )
                                    .expect("attach server endpoint");
                            }
                            {
                                let client = client_slot.borrow_mut();
                                let snapshot_reply_frame =
                                    frame_label_for_cursor_label(&client.cursor, 0x51);
                                let commit_final_frame =
                                    frame_label_for_cursor_label(&client.cursor, 0x55);
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(snapshot_reply_frame),
                                    instance: 41,
                                    has_fin: false,
                                    channel: Channel::new(17),
                                });
                                client.binding.incoming.push_back(IngressEvidence {
                                    frame_label: FrameLabel::new(commit_final_frame),
                                    instance: 42,
                                    has_fin: false,
                                    channel: Channel::new(18),
                                });
                            }

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);

                            client_send_request(
                                &mut cx,
                                client_slot,
                                1,
                                "first continue send",
                                "first request send",
                            );
                            server_reply_snapshot_request(
                                &mut cx,
                                server_slot,
                                snapshot_reply_payload,
                            );
                            client_decode_snapshot_reply_and_checkpoint(&mut cx, client_slot);

                            client_send_request(
                                &mut cx,
                                client_slot,
                                2,
                                "second continue send",
                                "second request send",
                            );
                            server_reply_commit_final_request(
                                &mut cx,
                                server_slot,
                                commit_final_payload,
                            );
                            client_decode_commit_final_reply_and_cancel(&mut cx, client_slot);
                        });
                    });
                }
            );
        },
    );
}

#[test]
fn dropping_pending_decode_future_preserves_preview_branch_state() {
    run_offer_regression_test(
        "dropping_pending_decode_future_preserves_preview_branch_state",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, HintPendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(HintPendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(HintPendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport =
                                HintPendingTransport::new(pending_state, HINT_LEFT_DATA_FRAME);
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(905);
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        controller_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &HINT_CONTROLLER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach controller endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        worker_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &HINT_WORKER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach worker endpoint");
                            }

                            let scope = {
                                let worker = worker_slot.borrow_mut();
                                let scope = worker.cursor.node_scope_id();
                                assert!(!scope.is_none(), "worker must start at route scope");
                                scope
                            };

                            {
                                let controller = controller_slot.borrow_mut();
                                controller.port_for_lane(0).record_route_decision(scope, 0);
                            }

                            let worker = worker_slot.borrow_mut();
                            let before_idx = worker.cursor.index();

                            let mut cx = Context::from_waker(noop_waker_ref());
                            let branch = {
                                let mut offer = pin!(cursor_offer(worker));
                                poll_ready_ok(&mut cx, offer.as_mut(), "preview branch offer")
                            };
                            assert_eq!(
                                branch_label(&branch),
                                HINT_LEFT_DATA_LABEL,
                                "offer must preview the hinted recv branch before decode"
                            );
                            let preview_ready_mask = worker.scope_ready_arm_mask(scope);
                            let preview_ack = worker.peek_scope_ack(scope);

                            {
                                let mut decode =
                                    pin!(CursorDecode::<Msg<100, u8>>::run(worker, branch));
                                assert!(
                                    matches!(decode.as_mut().poll(&mut cx), Poll::Pending),
                                    "decode should wait on transport recv before commit"
                                );
                                drop(decode);
                            }

                            assert_eq!(
                                worker.cursor.index(),
                                before_idx,
                                "dropping a pending decode future must not advance the cursor"
                            );
                            assert_eq!(
                                worker.peek_scope_ack(scope),
                                preview_ack,
                                "dropping a pending decode future must not consume ACK authority"
                            );
                            assert_eq!(
                                worker.scope_ready_arm_mask(scope),
                                preview_ready_mask,
                                "dropping a pending decode future must not clear ready-arm evidence"
                            );
                            assert!(
                                worker.selected_arm_for_scope(scope).is_none(),
                                "dropping a pending decode future must not commit route progress"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn restoring_public_preview_branch_clears_cached_arm_slot() {
    run_offer_regression_test(
        "restoring_public_preview_branch_clears_cached_arm_slot",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, HintPendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(HintPendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(HintPendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport =
                                HintPendingTransport::new(pending_state, HINT_LEFT_DATA_FRAME);
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(906);
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        controller_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &HINT_CONTROLLER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach controller endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        worker_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &HINT_WORKER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach worker endpoint");
                            }

                            let scope = {
                                let worker = worker_slot.borrow_mut();
                                let scope = worker.cursor.node_scope_id();
                                assert!(!scope.is_none(), "worker must start at route scope");
                                scope
                            };

                            {
                                let controller = controller_slot.borrow_mut();
                                controller.port_for_lane(0).record_route_decision(scope, 0);
                            }

                            let worker = worker_slot.borrow_mut();
                            let mut cx = Context::from_waker(noop_waker_ref());

                            let label = match worker.poll_public_offer(&mut cx) {
                                Poll::Ready(Ok(label)) => label,
                                Poll::Ready(Err(err)) => {
                                    panic!("public offer must materialize preview branch: {err:?}")
                                }
                                Poll::Pending => {
                                    panic!(
                                        "public offer must not pend once the hinted arm is ready"
                                    )
                                }
                            };
                            assert_eq!(
                                label, HINT_LEFT_DATA_LABEL,
                                "public offer must cache the hinted preview branch"
                            );
                            assert!(
                                worker.public_route_branch.is_some(),
                                "public offer must park the materialized branch until decode or drop"
                            );

                            worker.restore_public_route_branch();

                            assert!(
                                worker.public_route_branch.is_none(),
                                "restoring the preview branch must clear the cached public arm slot"
                            );

                            let label = match worker.poll_public_offer(&mut cx) {
                                Poll::Ready(Ok(label)) => label,
                                Poll::Ready(Err(err)) => panic!(
                                    "re-offer after restore must rematerialize the branch: {err:?}"
                                ),
                                Poll::Pending => {
                                    panic!("re-offer after restore must not pend")
                                }
                            };
                            assert_eq!(
                                label, HINT_LEFT_DATA_LABEL,
                                "re-offer after restore must rematerialize the same branch from restored state"
                            );
                            assert!(
                                worker.public_route_branch.is_some(),
                                "re-offer after restore must park a fresh preview branch"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn static_passive_offer_with_known_arm_waits_on_transport_without_busy_restart() {
    run_offer_regression_test(
        "static_passive_offer_with_known_arm_waits_on_transport_without_busy_restart",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, PendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(PendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(PendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport = PendingTransport::new(pending_state);
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1201);
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        controller_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &ENTRY_CONTROLLER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach controller endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        worker_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &ENTRY_WORKER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach worker endpoint");
                            }
                            let controller = controller_slot.borrow_mut();
                            let worker = worker_slot.borrow_mut();
                            let scope = worker.cursor.node_scope_id();
                            controller.port_for_lane(0).record_route_decision(scope, 1);

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);
                            let mut offer = pin!(cursor_offer(worker));
                            match offer.as_mut().poll(&mut cx) {
                                Poll::Ready(Ok(branch)) => {
                                    panic!(
                                        "offer must not materialize before transport ingress: {}",
                                        branch_label(&branch)
                                    )
                                }
                                Poll::Ready(Err(err)) => {
                                    panic!("offer must wait for transport ingress: {err:?}")
                                }
                                Poll::Pending => {}
                            }
                            assert_eq!(
                                transport_probe.poll_count(),
                                1,
                                "known static passive arm must park on transport once instead of frontier-restarting"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn parked_passive_offer_does_not_drain_hint_from_same_lane() {
    run_offer_regression_test(
        "parked_passive_offer_does_not_drain_hint_from_same_lane",
        || {
            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, HintPendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(HintPendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(HintPendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            pending_state
                                .panic_on_hint_drain_while_recv_parked
                                .set(true);
                            let transport = HintPendingTransport::new(
                                pending_state,
                                <Msg<86, u8> as MessageSpec>::LOGICAL_LABEL,
                            );
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1203);
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        controller_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &ENTRY_CONTROLLER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach controller endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        worker_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &ENTRY_WORKER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach worker endpoint");
                            }
                            let controller = controller_slot.borrow_mut();
                            let worker = worker_slot.borrow_mut();
                            let scope = worker.cursor.node_scope_id();
                            controller.port_for_lane(0).record_route_decision(scope, 1);

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);
                            let mut offer = pin!(cursor_offer(worker));

                            assert!(
                                matches!(offer.as_mut().poll(&mut cx), Poll::Pending),
                                "first offer poll must park on transport recv"
                            );
                            assert!(
                                matches!(offer.as_mut().poll(&mut cx), Poll::Pending),
                                "second offer poll must continue parked recv without draining hints"
                            );
                            transport_probe.assert_no_hint_drain_while_recv_parked();
                            assert_eq!(
                                transport_probe.poll_count(),
                                2,
                                "second offer poll must re-poll the same parked recv future"
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn decode_branch_commit_clears_stale_route_hints_on_selected_lane() {
    run_offer_regression_test(
        "decode_branch_commit_clears_stale_route_hints_on_selected_lane",
        || {
            const STALE_FRAME_HINT: u8 = 77;
            let stale_frame_hint_mask = FrameLabelMask::from_frame_label(STALE_FRAME_HINT);

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
                with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                        let transport = HintOnlyTransport::new(HINT_NONE);
                        let rv_id = cluster_ref
                            .add_rendezvous_from_config(config, transport)
                            .expect("register rendezvous");
                        let sid = SessionId::new(1204);
                        unsafe {
                            cluster_ref
                                .attach_endpoint_into::<0, _, _, _>(
                                    controller_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_CONTROLLER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach controller endpoint");
                            cluster_ref
                                .attach_endpoint_into::<1, _, _, _>(
                                    worker_slot.ptr(),
                                    rv_id,
                                    sid,
                                    &HINT_WORKER_PROGRAM(),
                                    NoBinding,
                                )
                                .expect("attach worker endpoint");
                        }

                        let controller = controller_slot.borrow_mut();
                        let worker = worker_slot.borrow_mut();
                        let scope = worker.cursor.node_scope_id();
                        controller.port_for_lane(0).record_route_decision(scope, 1);

                        let offer_lane = worker.port_for_lane(0).lane();
                        assert!(
                            !worker
                                .port_for_lane(0)
                                .route_table()
                                .has_pending_frame_hint_for_lane(offer_lane, stale_frame_hint_mask),
                            "test must start without stale route hints on the selected lane"
                        );
                        worker
                            .port_for_lane(0)
                            .route_table()
                            .update_pending_frame_hint_mask_for_lane(
                                offer_lane,
                                FrameLabelMask::EMPTY,
                                stale_frame_hint_mask,
                            );
                        assert!(
                            worker
                                .port_for_lane(0)
                                .route_table()
                                .has_pending_frame_hint_for_lane(offer_lane, stale_frame_hint_mask),
                            "stale route hint must be staged before route-branch commit"
                        );

                        let mut cx = Context::from_waker(noop_waker_ref());
                        let branch = {
                            let mut offer = pin!(cursor_offer(worker));
                            poll_ready_ok(&mut cx, offer.as_mut(), "offer selected right branch")
                        };
                        assert_eq!(
                            branch_label(&branch),
                            HINT_RIGHT_DATA_LABEL,
                            "offer must still materialize the selected right branch"
                        );

                        {
                            let mut decode = pin!(
                                CursorDecode::<Msg<HINT_RIGHT_DATA_LABEL, u8>>::run(worker, branch)
                            );
                            let _ = poll_ready_ok(
                                &mut cx,
                                decode.as_mut(),
                                "decode right branch and commit route preview",
                            );
                        }
                        assert!(
                            !worker
                                .port_for_lane(0)
                                .route_table()
                                .has_pending_frame_hint_for_lane(offer_lane, stale_frame_hint_mask),
                            "decode commit must clear offer-scoped route hints on the selected lane"
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn nested_dispatch_arm_counts_as_recv_for_known_passive_route() {
    run_offer_regression_test(
        "nested_dispatch_arm_counts_as_recv_for_known_passive_route",
        || {
            #[inline(never)]
            fn assert_known_passive_route_waits_for_ingress(
                cx: &mut Context<'_>,
                worker_slot: &mut OfferValueSlotGuard<'_, PendingWorkerEndpoint>,
                transport_probe: &PendingTransport,
            ) {
                let worker = worker_slot.borrow_mut();
                let scope = worker.cursor.node_scope_id();
                assert!(
                    worker.arm_has_recv(scope, 1),
                    "nested first-recv dispatch must count as recv-bearing arm"
                );

                let mut offer = pin!(cursor_offer(worker));
                match offer.as_mut().poll(cx) {
                    Poll::Ready(Ok(branch)) => panic!(
                        "known passive route with nested dispatch recv must wait for wire ingress, got {}",
                        branch_label(&branch)
                    ),
                    Poll::Ready(Err(err)) => {
                        panic!(
                            "known passive route with nested dispatch recv must not fail: {err:?}"
                        )
                    }
                    Poll::Pending => {}
                }
                assert_eq!(
                    transport_probe.poll_count(),
                    1,
                    "known passive route with nested dispatch recv must still poll transport once"
                );
            }

            offer_fixture!(2048, clock, config);
            with_offer_cluster!(clock, PendingOfferCluster, cluster_ref, {
                with_offer_value_slot!(PendingControllerEndpoint, controller_slot, {
                    with_offer_value_slot!(PendingWorkerEndpoint, worker_slot, {
                        with_offer_value_slot!(PendingTransportState, pending_state_slot, {
                            pending_state_slot.store(PendingTransportState::default());
                            let pending_state: &'static PendingTransportState =
                                unsafe { &*pending_state_slot.ptr() };
                            let transport = PendingTransport::new(pending_state);
                            let transport_probe = transport;
                            let rv_id = cluster_ref
                                .add_rendezvous_from_config(config, transport)
                                .expect("register rendezvous");
                            let sid = SessionId::new(1202);
                            unsafe {
                                cluster_ref
                                    .attach_endpoint_into::<0, _, _, _>(
                                        controller_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &NESTED_DISPATCH_CONTROLLER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach controller endpoint");
                                cluster_ref
                                    .attach_endpoint_into::<1, _, _, _>(
                                        worker_slot.ptr(),
                                        rv_id,
                                        sid,
                                        &NESTED_DISPATCH_WORKER_PROGRAM(),
                                        NoBinding,
                                    )
                                    .expect("attach worker endpoint");
                            }

                            let scope = {
                                let worker = worker_slot.borrow_mut();
                                worker.cursor.node_scope_id()
                            };
                            {
                                let controller = controller_slot.borrow_mut();
                                controller.port_for_lane(0).record_route_decision(scope, 1);
                            }

                            let waker = noop_waker_ref();
                            let mut cx = Context::from_waker(waker);
                            assert_known_passive_route_waits_for_ingress(
                                &mut cx,
                                worker_slot,
                                &transport_probe,
                            );
                        });
                    });
                });
            });
        },
    );
}

#[test]
fn scope_local_label_mapping_never_uses_global_scan() {
    run_offer_regression_test("scope_local_label_mapping_never_uses_global_scan", || {
        offer_fixture!(2048, clock, config);
        with_offer_cluster!(clock, OfferHintCluster, cluster_ref, {
            with_offer_value_slot!(OfferHintControllerEndpoint, controller_slot, {
                with_offer_value_slot!(OfferHintWorkerEndpoint, worker_slot, {
                    let transport = HintOnlyTransport::new(HINT_NONE);
                    let rv_id = cluster_ref
                        .add_rendezvous_from_config(config, transport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(992);
                    unsafe {
                        cluster_ref
                            .attach_endpoint_into::<0, _, _, _>(
                                controller_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_CONTROLLER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach controller endpoint");
                        cluster_ref
                            .attach_endpoint_into::<1, _, _, _>(
                                worker_slot.ptr(),
                                rv_id,
                                sid,
                                &HINT_WORKER_PROGRAM(),
                                NoBinding,
                            )
                            .expect("attach worker endpoint");
                    }
                    let controller_borrow = controller_slot.borrow_mut();
                    core::hint::black_box(&controller_borrow);
                    let worker = worker_slot.borrow_mut();
                    let scope = worker.cursor.node_scope_id();
                    assert!(!scope.is_none(), "worker must start at route scope");

                    let foreign_frame_label = (1u8..=u8::MAX).find(|frame_label| {
                        !matches!(
                            *frame_label,
                            TEST_LOOP_CONTINUE_FRAME | TEST_LOOP_BREAK_FRAME
                        ) && worker
                            .cursor
                            .first_recv_target_for_lane_frame_label(scope, 0, *frame_label)
                            .is_none()
                            && worker
                                .cursor
                                .find_arm_for_recv_lane_frame_label(0, *frame_label)
                                .is_some()
                    });
                    let Some(foreign_frame_label) = foreign_frame_label else {
                        // FIRST-recv dispatch can fully cover this scope; no entry-only
                        // entry-only dispatch evidence remains to probe.
                        return;
                    };

                    let frame_label_meta =
                        endpoint_scope_frame_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
                    worker.ingest_binding_scope_evidence(
                        scope,
                        0,
                        foreign_frame_label,
                        false,
                        frame_label_meta,
                    );

                    assert!(
                        !worker.scope_has_ready_arm_evidence(scope),
                        "foreign frame label {} must not become scope-local arm-ready evidence: hint={} arm={:?} evidence={:?} ready_mask=0b{:02b} controller={}",
                        foreign_frame_label,
                        frame_label_meta.matches_frame_hint(foreign_frame_label),
                        frame_label_meta.arm_for_frame_label(foreign_frame_label),
                        frame_label_meta.evidence_arm_for_frame_label(foreign_frame_label),
                        worker.scope_ready_arm_mask(scope),
                        worker.cursor.is_route_controller(scope)
                    );
                    assert!(
                        worker.peek_scope_ack(scope).is_none(),
                        "foreign label must not mint route authority"
                    );
                });
            });
        });
    });
}

#[test]
fn payload_staging_is_selected_scope_lane_stable() {
    let mut scratch = [0u8; 8];
    let src = [9u8, 8, 7, 6];
    let len = stage_transport_payload(&mut scratch, &src).expect("stage payload");
    assert_eq!(len, src.len());
    assert_eq!(&scratch[..len], &src);
}
