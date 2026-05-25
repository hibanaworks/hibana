// Offer-path kernel regression tests split by behavior owner.
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use super::super::*;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::binding::BindingSlot;

pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::binding::{
    Channel, IngressEvidence, TransportOpsError,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::control::cap::mint::{
    ControlOp, GenericCapToken, ResourceKind,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::control::cap::resource_kinds::RouteDecisionKind;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::control::cluster::core::SessionCluster;
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::endpoint::kernel::{
    lane_port,
    offer::{FrontierObservationDomain, LaneIngressEvidence},
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::g::{
    self, Msg, Role,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::global::role_program::{
    DenseLaneOrdinal, LaneSet, LaneSetView, LaneWord, RoleProgram, lane_word_count, project,
};
pub(in crate::endpoint::kernel::core::offer_regression_tests::cases) use crate::global::steps::{
    PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil,
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
