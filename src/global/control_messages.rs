//! Constants and documentation for control messages used in route.case arms.
//!
//! Control-plane operations (loop decisions, checkpoint/rollback, etc.) are
//! expressed as typed messages carrying `CapToken<ResourceKind>` payloads.
//! This eliminates the need for bespoke loop combinators while preserving
//! observability via ResourceKind→CpEffect mapping.
//!
//! # Usage
//!
//! In `route.case` arms, specify the label and `CapToken<K>` payload type:
//!
//! ```ignore
//! use hibana::{g, control::cap::CapToken, control::cap::resource_kinds::LoopContinueKind};
//! use hibana::runtime::consts::*;
//!
//! type Controller = g::Role<0>;
//! type Target = g::Role<1>;
//! type ContinueArm = g::StepCons<
//!     g::SendStep<
//!         Target,
//!         Controller,
//!         g::Msg<LABEL_LOOP_CONTINUE, CapToken<LoopContinueKind>>
//!     >,
//!     g::StepNil,
//! >;
//! type BreakArm = g::StepCons<
//!     g::SendStep<
//!         Target,
//!         Controller,
//!         g::Msg<LABEL_LOOP_BREAK, CapToken<LoopBreakKind>>
//!     >,
//!     g::StepNil,
//! >;
//!
//! const LOOP_ROUTE: g::Program<
//!     <ContinueArm as g::StepConcat<BreakArm>>::Output
//! > = g::route(
//!     g::route_chain::<1, 0, ContinueArm>(
//!         g::send::<Target, Controller, g::Msg<LABEL_LOOP_CONTINUE, CapToken<LoopContinueKind>>>(),
//!     )
//!     .and(g::send::<Target, Controller, g::Msg<LABEL_LOOP_BREAK, CapToken<LoopBreakKind>>>()),
//! );
//! ```
//!
//! # Reserved Label Ranges
//!
//! - `48-57`: Control messages (loop, splice, reroute, policy, SLO)
//! - `59-63`: Existing control labels (crash, cancel, checkpoint, commit, rollback)
//!
//! See [`crate::runtime::consts`] for specific label assignments.

use crate::control::{
    cap::GenericCapToken,
    cap::resource_kinds::{RerouteKind, SpliceAckKind, SpliceIntentKind},
};
use crate::global::{self as g};
use crate::runtime::consts::{LABEL_REROUTE, LABEL_SPLICE_ACK, LABEL_SPLICE_INTENT};

type SingleSendProgram<From, To, Msg, const LANE: u8 = 0> =
    g::Program<g::StepCons<g::SendStep<From, To, Msg, LANE>, g::StepNil>>;

/// Message type for SpliceIntent with ExternalControl (cross-role allowed, wire transmission).
///
/// SpliceIntent uses ExternalControl to allow cross-role communication for distributed splice
/// scenarios (e.g., between different Rendezvous instances). Tokens are auto-minted via
/// `AUTO_MINT_EXTERNAL = true`.
pub type SpliceIntentMsg = g::Msg<
    LABEL_SPLICE_INTENT,
    GenericCapToken<SpliceIntentKind>,
    crate::g::ExternalControl<SpliceIntentKind>,
>;

/// Message type for SpliceAck with ExternalControl (cross-role allowed, wire transmission).
///
/// SpliceAck uses ExternalControl to allow cross-role communication for distributed splice
/// scenarios. Tokens are auto-minted via `AUTO_MINT_EXTERNAL = true`.
pub type SpliceAckMsg = g::Msg<
    LABEL_SPLICE_ACK,
    GenericCapToken<SpliceAckKind>,
    crate::g::ExternalControl<SpliceAckKind>,
>;

/// Message type for Reroute with CanonicalControl.
pub type RerouteMsg =
    g::Msg<LABEL_REROUTE, GenericCapToken<RerouteKind>, crate::g::CanonicalControl<RerouteKind>>;

// =============================================================================
// Local splice helpers (self-send convenience)
// =============================================================================
//
// These helpers are provided for convenience with specific role indices.
// While SpliceIntentMsg/SpliceAckMsg use ExternalControl (allowing cross-role),
// the `_local_` variants specify From == To for self-send scenarios.

/// Helper that delegates a `SpliceIntent` message to a local lane for Role<0>.
pub const fn splice_intent_local_0<const DST_LANE: u16>()
-> SingleSendProgram<g::Role<0>, g::Role<0>, SpliceIntentMsg> {
    g::send::<g::Role<0>, g::Role<0>, SpliceIntentMsg, 0>()
        .with_control_plan(crate::global::const_dsl::splice_local_plan::<DST_LANE>())
}

/// Helper that delegates a `SpliceIntent` message to a local lane for Role<1>.
pub const fn splice_intent_local_1<const DST_LANE: u16>()
-> SingleSendProgram<g::Role<1>, g::Role<1>, SpliceIntentMsg> {
    g::send::<g::Role<1>, g::Role<1>, SpliceIntentMsg, 0>()
        .with_control_plan(crate::global::const_dsl::splice_local_plan::<DST_LANE>())
}

/// Helper that embeds local lane information into a `SpliceAck` message for Role<0>.
pub const fn splice_ack_local_0<const DST_LANE: u16>()
-> SingleSendProgram<g::Role<0>, g::Role<0>, SpliceAckMsg> {
    g::send::<g::Role<0>, g::Role<0>, SpliceAckMsg, 0>()
        .with_control_plan(crate::global::const_dsl::splice_local_plan::<DST_LANE>())
}

/// Helper that embeds local lane information into a `SpliceAck` message for Role<1>.
pub const fn splice_ack_local_1<const DST_LANE: u16>()
-> SingleSendProgram<g::Role<1>, g::Role<1>, SpliceAckMsg> {
    g::send::<g::Role<1>, g::Role<1>, SpliceAckMsg, 0>()
        .with_control_plan(crate::global::const_dsl::splice_local_plan::<DST_LANE>())
}

/// Helper that embeds local lane/shard information into a `Reroute` message for Role<0>.
pub const fn reroute_local_0<const DST_LANE: u16, const SHARD: u32>()
-> SingleSendProgram<g::Role<0>, g::Role<0>, RerouteMsg> {
    g::send::<g::Role<0>, g::Role<0>, RerouteMsg, 0>().with_control_plan(
        crate::global::const_dsl::reroute_local_plan::<DST_LANE, SHARD>(),
    )
}

/// Helper that embeds local lane/shard information into a `Reroute` message for Role<1>.
pub const fn reroute_local_1<const DST_LANE: u16, const SHARD: u32>()
-> SingleSendProgram<g::Role<1>, g::Role<1>, RerouteMsg> {
    g::send::<g::Role<1>, g::Role<1>, RerouteMsg, 0>().with_control_plan(
        crate::global::const_dsl::reroute_local_plan::<DST_LANE, SHARD>(),
    )
}
