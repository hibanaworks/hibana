use crate::endpoint::kernel::core::offer_regression_tests::cases::*;
// Offer-path kernel regression tests.

use crate::global::role_program::{
    DenseLaneOrdinal, LaneSet, LaneSetView, LaneWord, lane_word_count,
};
use crate::global::steps::{RouteSteps, SendStep, StepCons, StepNil};
use crate::runtime::config::CounterClock;
use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse};
use crate::transport::{FrameLabel, FrameLabelMask, Transport};
use core::{
    future::Future,
    marker::PhantomData,
    mem::{MaybeUninit, align_of, size_of},
    task::{Context, Poll},
};

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
        payload_ref: &'a M::Payload,
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
        let logical_label = <M as MessageSpec>::LOGICAL_LABEL;
        let mut preview = Some(endpoint.preview_flow_meta(logical_label));
        let mut payload = Some(crate::endpoint::kernel::RawSendPayload::from_typed::<
            M::Payload,
        >(payload_ref));
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
                Poll::Ready(Ok(outcome)) => {
                    outcome.descriptor.publish();
                    Poll::Ready(Ok(()))
                }
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
            Poll::Ready(Ok(outcome)) => {
                outcome.descriptor.publish();
                Poll::Ready(Ok(()))
            }
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

mod frontier_helpers;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use frontier_helpers::*;

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) const fn max_usize(
    values: &[usize],
) -> usize {
    frontier_helpers::frontier_max_usize(values)
}
