// Offer-path kernel regression tests split by behavior owner.
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use super::super::*;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::binding::EndpointSlot;

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::binding::{
    Channel, IngressEvidence, BindingError,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::control::cap::mint::{
    ControlOp, LocalControlKind,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::control::cap::resource_kinds::RouteDecisionKind;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::control::cluster::core::SessionCluster;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::endpoint::kernel::{
    lane_port,
    offer::{FrontierObservationDomain, LaneIngressEvidence},
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::endpoint::kernel::core::MaterializedRouteBranch;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::g::{
    self, Msg,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::global::role_program::{
    RoleProgram, project,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::observe::core::TapEvent;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::runtime::config::{
    Config, CounterClock,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::runtime::consts::{
    DefaultLabelUniverse, LabelUniverse, RING_EVENTS,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::transport::{
    FrameLabel, FrameLabelMask, Transport, TransportError, wire::Payload,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::test_support::{
    abort_control::{ABORT_CONTROL_LOGICAL, AbortControl},
    route_control_kinds::RouteControl,
    snapshot_control::{SNAPSHOT_CONTROL_LOGICAL, SnapshotControl},
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use core::{
    cell::{Cell, UnsafeCell},
    future::Future,
    marker::PhantomData,
    mem::{MaybeUninit, align_of, size_of},
    pin::pin,
    task::{Context, Poll},
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use futures::task::noop_waker_ref;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use std::{
    task::Waker, thread_local,
};

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) trait OfferEvidenceTestExt {
    fn ingest_scope_evidence_for_offer_lanes(
        &mut self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lanes: LaneSetView,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    );
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B> OfferEvidenceTestExt
    for crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: crate::transport::Transport + 'r,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::EndpointSlot + 'r,
{
    fn ingest_scope_evidence_for_offer_lanes(
        &mut self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lanes: LaneSetView,
        suppress_hint: bool,
        frame_label_meta: ScopeFrameLabelMeta,
    ) {
        let pending_recv = lane_port::PendingRecv::new();
        self.ingest_scope_evidence_for_offer(
            &pending_recv,
            scope_id,
            summary_lane_idx,
            offer_lanes,
            suppress_hint,
            frame_label_meta,
        );
    }
}

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) trait ReceivedFrameFixtureExt<
    'r,
>
{
    fn received_transport_frame(
        &self,
        lane_idx: u8,
        payload: Payload<'r>,
    ) -> lane_port::ReceivedFrame<'r>;

    fn staged_transport_payload(&self, lane_idx: u8, payload: Payload<'r>) -> StagedPayload<'r>;
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B> ReceivedFrameFixtureExt<'r>
    for crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: crate::transport::Transport + 'r,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
    Mint: crate::control::cap::mint::MintConfigMarker,
    B: crate::binding::EndpointSlot + 'r,
{
    fn received_transport_frame(
        &self,
        lane_idx: u8,
        payload: Payload<'r>,
    ) -> lane_port::ReceivedFrame<'r> {
        lane_port::ReceivedFrame::from_port(self.port_for_lane(lane_idx as usize), payload)
    }

    fn staged_transport_payload(&self, lane_idx: u8, payload: Payload<'r>) -> StagedPayload<'r> {
        StagedPayload::Transport {
            frame: self.received_transport_frame(lane_idx, payload),
        }
    }
}

#[macro_use]
mod fixture_storage;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use fixture_storage::*;

mod compact_and_helpers;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use compact_and_helpers::*;

mod hint_route_fixture;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use hint_route_fixture::*;

mod attach_and_basic_offer;
mod frontier_observation;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use frontier_observation::*;

mod binding_frame_mismatch;
mod controller_binding_masks;
mod hint_conflict_recovery;
mod iterative_route_replies;
mod passive_hint_and_terminal_faults;
mod rollback_and_nested_offer;
mod static_linger_routes;
