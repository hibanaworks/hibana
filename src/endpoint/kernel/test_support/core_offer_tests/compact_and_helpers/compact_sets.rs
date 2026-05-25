use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
// Offer-path kernel regression tests.

use super::*;

use crate::binding::{Channel, IngressEvidence, TransportOpsError};
use crate::control::cap::mint::{ControlOp, GenericCapToken, ResourceKind};
use crate::control::cap::resource_kinds::RouteDecisionKind;
use crate::control::cluster::core::SessionCluster;
use crate::endpoint::kernel::{lane_port, offer::LaneIngressEvidence};
use crate::g::{self, Msg, Role};
use crate::global::role_program::{
    DenseLaneOrdinal, LaneSet, LaneSetView, LaneWord, RoleProgram, lane_word_count, project,
};
use crate::global::steps::{PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
use crate::observe::core::TapEvent;
use crate::runtime::config::{Config, CounterClock};
use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse, RING_EVENTS};
use crate::transport::{FrameLabel, FrameLabelMask, Transport, TransportError, wire::Payload};
use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    marker::PhantomData,
    mem::{MaybeUninit, align_of, size_of},
    pin::pin,
    task::{Context, Poll},
};
use futures::task::noop_waker_ref;
use std::{task::Waker, thread_local};

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type SendOnly<
    const LANE: u8,
    S,
    D,
    M,
> = StepCons<SendStep<S, D, M, LANE>, StepNil>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type BranchSteps<L, R> =
    RouteSteps<L, R>;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const PICO_OFFER_FIXTURE_SLAB_CAPACITY: usize = 64 * 1024;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const LARGE_HOST_OFFER_FIXTURE_SLAB_CAPACITY: usize = 1_048_576;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const OFFER_FIXTURE_SLAB_CAPACITY: usize = LARGE_HOST_OFFER_FIXTURE_SLAB_CAPACITY;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const TEST_LOOP_BREAK_LOGICAL: u8 = 0xA2;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const TEST_ROUTE_DECISION_LOGICAL: u8 = 0xA3;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const TEST_LOOP_CONTINUE_FRAME: u8 = 2;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const TEST_LOOP_BREAK_FRAME:
    u8 = 3;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const ROUTE_HINT_RIGHT_LABEL: u8 = 122;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) type RouteHintRightKind =
    RouteControl<0>;

#[test]
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn offer_entry_state_stays_compact_resident_frontier_header()
 {
    const WORD: usize = size_of::<usize>();
    assert!(
        size_of::<OfferEntryState>() <= 4 * WORD,
        "OfferEntryState must remain a compact runtime header, not a cached descriptor/meta blob"
    );
    assert!(
        size_of::<OfferEntrySlot>() <= 5 * WORD,
        "offer-entry runtime slots must not cache heavy resident descriptor metadata"
    );
    assert!(
        size_of::<OfferEntryState>() < size_of::<ScopeArmMaterializationMeta>(),
        "ScopeArmMaterializationMeta is descriptor-derived materialization data and must not live inside OfferEntryState"
    );
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn frame_label_for_cursor_label(
    cursor: &PhaseCursor,
    label: u8,
) -> u8 {
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn overwrite_global_active_entries_fixture<
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn overwrite_global_frontier_observed_fixture<
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn overwrite_global_frontier_observed_key_fixture<
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct CursorSend<M>(
    PhantomData<M>,
);

impl<M> CursorSend<M>
where
    M: MessageSpec + SendableLabel,
{
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn run<
        'a,
        'r,
        A,
        const ROLE: u8,
        T,
        U,
        C,
        E,
        const MAX_RV: usize,
        Mint,
        B,
    >(
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
        let logical_label = <M as MessageSpec>::LOGICAL_LABEL;
        let mut preview = Some(endpoint.preview_flow_meta(logical_label));
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
                let descriptor = crate::endpoint::flow::send_runtime_desc::<M>(FrameLabel::new(
                    meta.frame_label,
                ));
                state = Some(SendState::Init {
                    descriptor,
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

    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn run_with_meta<
        'a,
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
        let mut state = SendState::Init {
            descriptor: crate::endpoint::flow::send_runtime_desc::<M>(FrameLabel::new(
                meta.frame_label,
            )),
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct CursorOffer<
    'a,
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B,
> where
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

impl<'a, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B> Drop
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
    fn drop(&mut self) {
        self.endpoint.restore_detached_offer_state(&mut self.state);
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn cursor_offer<
    'a,
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn branch_label(
    branch: &MaterializedRouteBranch<'_>,
) -> u8 {
    branch.label
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn branch_scope(
    branch: &MaterializedRouteBranch<'_>,
) -> ScopeId {
    branch.branch_meta.scope_id
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn branch_has_staged_payload(
    branch: &MaterializedRouteBranch<'_>,
) -> bool {
    branch.staged_payload.is_some()
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn branch_has_transport_payload(
    branch: &MaterializedRouteBranch<'_>,
) -> bool {
    matches!(branch.staged_payload, Some(StagedPayload::Transport { .. }))
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct CursorDecode<M>(
    PhantomData<M>,
);

impl<M> CursorDecode<M>
where
    M: MessageSpec,
    M::Payload: crate::transport::wire::WirePayload,
{
    pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) fn run<
        'a,
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
            state: crate::endpoint::kernel::decode::DecodeState::new(branch),
            _borrow: PhantomData,
            _msg: PhantomData,
        }
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) struct CursorDecodeFuture<
    'a,
    'r,
    const ROLE: u8,
    T,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B,
    M,
> where
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
    state: crate::endpoint::kernel::decode::DecodeState<'r>,
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
        match endpoint.poll_decode_state(
            <M as MessageSpec>::LOGICAL_LABEL,
            <M::ControlKind as crate::global::ControlPayloadKind>::IS_CONTROL,
            |payload| {
                <M::Payload as crate::transport::wire::WirePayload>::validate_payload(payload)
            },
            |scratch| {
                <M::Payload as crate::transport::wire::WirePayload>::synthetic_payload(scratch)
            },
            &mut this.state,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload)) => {
                let payload = unsafe {
                    // SAFETY: test decode futures use the same endpoint-resident
                    // payload storage discipline as the public decode future.
                    super::super::lane_port::endpoint_resident_payload(payload)
                };
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

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const fn max_usize(
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
