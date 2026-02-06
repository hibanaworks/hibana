#![cfg_attr(not(feature = "std"), no_std)]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(unexpected_cfgs)]

//! Hibana — Type-safe async MPST for `no_std` environments
//!
//! Hibana follows a global-first workflow: author a **global**
//! choreography with native Rust combinators, project it to role-local programs,
//! and attach the resulting typestate cursors to the runtime control plane.
//! The crate is intentionally minimalist—there are no compat layers or builder
//! shims. The public surface consists of the top-level modules re-exported
//! from this crate root.
//!
//! ## Top-Level Modules
//!
//! | Module | Responsibility |
//! | ------ | -------------- |
//! | [`g`] (also re-exported as `hibana::g`) | Global protocol DSL, const effect synthesis, role projection. |
//! | [`runtime`] | `SessionCluster` facade, rendezvous lifecycle, distributed splice orchestration. |
//! | [`transport`] | Non-polling frame transport abstraction plus wire/forward helpers. |
//! | [`endpoint`] | Cursor-driven session endpoints (send/recv/delegate/loop control). |
//! | [`observe`] | 16-byte tap ring and normalization utilities (when `std` is enabled). |
//! | [`control`] | Control-plane mini-kernel (CpEffect, capability brokering, routing policies). |
//! | [`binding`] | Transport binding traits and channel stores for no-alloc binders. |
//! | [`epf`] | Effect Policy Filter (bytecode VM for control-plane policies). |
//!
//! Internal implementation details such as the rendezvous core and effect graph
//! helpers remain available behind `#[doc(hidden)]` for integration tests but
//! are not part of the supported API.
//!
//! ## Global → Local Workflow
//!
//! 1. **Author the protocol** as a const [`g::Program`] using the provided
//!    combinators (`send`, `seq`, `par`, `route`).
//! 2. **Project** the protocol to a role-local [`g::RoleProgram`] at compile
//!    time with [`g::project`]. This materialises the typestate graph and the
//!    local send/recv transitions (including control metadata such as loop
//!    scopes and capability shots).
//! 3. **Attach** a cursor endpoint through [`runtime::SessionCluster::attach_cursor`]
//!    to start interacting with the session.
//!
//! ```rust,ignore
//! use hibana::{g, global::const_dsl::{DynamicMeta, HandlePlan}};
//!
//! type Controller = g::Role<0>;
//! type Worker = g::Role<1>;
//!
//! const ROUTE_POLICY_ID: u16 = 7;
//! const ROUTE_PLAN_META: DynamicMeta = DynamicMeta::new();
//!
//! // Define the global protocol as a const program. Each fragment is assembled
//! // from other const programs, so the entire choreography is a single source of truth.
//! const PROTOCOL: g::Program<_> = g::seq(
//!     g::send::<Controller, Worker, g::Msg<1, u32>>(),
//!     g::route(
//!         g::route_chain::<1, 0>(g::with_control_plan(
//!             g::send::<Worker, Controller, g::Msg<2, ()>>(),
//!             HandlePlan::route_dynamic(ROUTE_POLICY_ID, ROUTE_PLAN_META),
//!         ))
//!         .and(g::with_control_plan(
//!             g::send::<Worker, Controller, g::Msg<3, ()>>(),
//!             HandlePlan::route_dynamic(ROUTE_POLICY_ID, ROUTE_PLAN_META),
//!         )),
//!     ),
//! );
//!
//! // Projection computes the role-local typelist and typestate entirely at
//! // compile time. The resulting value is `Copy + 'static`.
//! const CONTROLLER: g::RoleProgram<'static, 0, _> = g::project::<0, _, _>(&PROTOCOL);
//! const WORKER: g::RoleProgram<'static, 1, _> = g::project::<1, _, _>(&PROTOCOL);
//!
//! # fn attach<'cfg>() -> hibana::endpoint::RecvResult<()> {
//! # use hibana::runtime::{SessionCluster, config::Config, consts::{DefaultLabelUniverse, RING_EVENTS}};
//! # use hibana::transport::Transport;
//! # struct Dummy; impl Transport for Dummy {
//! #     type Error = hibana::transport::TransportError;
//! #     type Tx<'a> = () where Self: 'a;
//! #     type Rx<'a> = () where Self: 'a;
//! #     type Send<'a> = core::future::Ready<Result<(), Self::Error>> where Self: 'a;
//! #     type Recv<'a> = core::future::Ready<Result<hibana::transport::wire::Payload<'a>, Self::Error>> where Self: 'a;
//! #     type Metrics = hibana::transport::NoopMetrics;
//! #     fn open<'a>(&'a self, _local_role: u8) -> (Self::Tx<'a>, Self::Rx<'a>) { ((), ()) }
//! #     fn prepare_tap_frame<'a>(&'a self, _tx: &'a mut Self::Tx<'a>, _meta: hibana::transport::trace::TapFrameMeta, _dest: u8) {}
//! #     fn send<'a, 'f>(&'a self, _tx: &'a mut Self::Tx<'a>, _payload: hibana::transport::wire::Payload<'f>, _dest: u8) -> Self::Send<'a> where 'a: 'f {
//! #         core::future::ready(Ok(()))
//! #     }
//! #     fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
//! #         core::future::ready(Err(hibana::transport::TransportError::Offline))
//! #     }
//! # }
//! # let mut tap = [hibana::observe::TapEvent::default(); RING_EVENTS];
//! # let mut slab = [0u8; 256];
//! # let rendezvous = hibana::rendezvous::Rendezvous::from_config(
//! #     Config::new(&mut tap, &mut slab),
//! #     Dummy,
//! # );
//! # let mut cluster: SessionCluster<'_, Dummy, DefaultLabelUniverse, hibana::runtime::config::CounterClock, 1> =
//! #     SessionCluster::new();
//! # cluster.register_local(&rendezvous).unwrap();
//! # let rv = hibana::control::types::RendezvousId::new(0);
//! # let sid = hibana::control::types::SessionId::new(1);
//! # let lane = hibana::control::types::LaneId::new(0);
//! // At runtime, attach the const programs to obtain typed cursor endpoints.
//! let controller = cluster.attach_cursor::<0, _>(rv, sid, &CONTROLLER)?;
//! let worker = cluster.attach_cursor::<1, _>(rv, sid, &WORKER)?;
//! # let _ = (controller, worker);
//! # Ok(())
//! # }
//! ```
//!
//! Run the canonical validation suite after integrating a protocol to ensure
//! rendezvous/cursor invariants stay intact:
//! `HIBANA_CANCEL_CASES=2048 HIBANA_ROLLBACK_CASES=2048 cargo test --tests --all-features`.
//! A step-by-step walkthrough of this flow lives in the "Quick Start" section of
//! `README.md`.
//!
//! ## Design Tenets
//!
//! - **Global-first**: the const DSL is the sole source of truth. Value-level
//!   effect lists (`EffList`) and metadata (scope/control markers) are derived
//!   automatically during const evaluation.
//! - **Type-driven safety**: role-projection rejects payload mismatches,
//!   missing route arms, unguarded recursion, and lane-role conflicts at compile
//!   time (see the trybuild suite under `tests/ui`).
//! - **No polling**: rendezvous control integrates with arbitrary async runtimes
//!   supplied by the embedding environment; the crate itself does not ship a
//!   scheduler.
//! - **Observability by construction**: every control decision (loop continue,
//!   cancel pair, checkpoint/rollback, splice) is annotated in the synthesized
//!   effect list so the runtime can seed tap events deterministically.
//! - **Capability discipline**: control messages carry capability tokens whose
//!   shot and permissions are embedded in the const metadata, enabling both
//!   static validation and EPF policy enforcement.
//!
//! # Cargo Features
//!
//! - `std` — Enables transport/testing utilities and observability normalisers.

#[cfg(test)]
extern crate std;

// ============================================================================
// Public modules (application-facing)
// ============================================================================

/// Global-to-Local projection (MPST theory layer)
pub mod global;
pub use global as g;

/// Control Plane (TRUE public API for control operations)
pub mod control;

/// Runtime facade (SessionCluster, user-facing orchestration)
pub mod runtime;

/// Transport abstraction (data plane)
pub mod transport;

/// Session endpoints (affine-typed consuming futures)
pub mod endpoint;

/// Observability (tap events, normalization)
pub mod observe;

/// Transport binding layer (PlanTable, TransportOps, FlowBinder)
pub mod binding;

/// EPF Effect Policy Filter (bytecode VM for control-plane policies).
pub mod epf;

// ============================================================================
// Internal modules (NOT for direct user access)
// ============================================================================

#[doc(hidden)]
pub mod eff;

/// Rendezvous (internal state machine, evaluates CpEffect)
///
/// **INTERNAL IMPLEMENTATION - DO NOT USE DIRECTLY**
///
/// This module contains the internal implementation of the Rendezvous state machine.
/// It evaluates `CpEffect` operations and manages local state (lane/gen/cap/splice).
///
/// **For application code**, use:
/// - [`control`] module for control plane operations
/// - [`runtime::SessionCluster`] for Rendezvous coordination
///
/// This module is `pub` only for integration tests. It will become `pub(crate)`
/// once tests are migrated to unit tests or rewritten to use the public API.
#[doc(hidden)]
pub mod rendezvous;

// ============================================================================
// Re-exports (curated public API)
// ============================================================================

// Global protocol combinators
pub use global::{
    ControlLabelSpec, ControlMessage, ControlMessageKind, LabelMarker, LabelTag, Message,
    MessageControlSpec, MessageSpec, Msg, ParChainBuilder, Role, RoleMarker, RouteChainBuilder,
    const_dsl::EffList,
    par, par_chain,
    program::Program,
    project, route, route_chain, send, seq,
    typestate::{LoopMetadata, LoopRole, PassiveArmNavigation, PhaseCursor, RoleTypestate, ScopeMetadata},
};

// Control plane (THE primary API)
pub use control::CpEffect;
pub use control::automaton::txn::{Closed, InAcked, InBegin, Txn};
pub use control::types::{Gen, LaneId, RendezvousId, SessionId};

// Runtime
pub use runtime::SessionCluster;

// Resolver context for dynamic policy evaluation
pub use control::cluster::core::{DynamicResolution, ResolverContext};

// Session endpoints
pub use endpoint::{
    CursorEndpoint, LoopDecision, RecvError, RecvGuard, RecvResult, RouteBranch, SendError,
    SendResult,
};

// Transport
pub use transport::Transport;

// Codec (from transport::wire) and tap/trace helpers
pub use transport::trace::{TapFrame, TapFrameMeta};
pub use transport::wire::{CodecError, WireDecode, WireDecodeOwned, WireEncode};

// Config (from runtime::config)
pub use runtime::config::{Clock, CounterClock};

// Consts (from runtime::consts)
pub use runtime::consts::{DEFAULT_LABEL_UNIVERSE, LabelUniverse};

// Capability (from control::cap)
pub use control::cap::{CapShot, ControlResourceKind, GenericCapToken, ResourceKind};
// CapToken is now internal-only. Use GenericCapToken<K> or control::ControlFrame<K>.

// Forward (from transport::forward)
pub use transport::forward::Forward;

// Transport context (from transport::context)
pub use transport::context::{
    ContextKey, ContextSnapshot, ContextValue, NoContext, TransportContextProvider,
};

// Binding layer (from binding)
pub use binding::{
    ArrayChannelStore, BindingSlot, Channel, ChannelDirection, ChannelKey, ChannelStore,
    IncomingClassification, LocalDirection, NoBinding, SendDisposition, SendMetadata,
    TransportOpsError,
};
#[cfg(feature = "std")]
pub use binding::StdChannelStore;

// Resolver (from endpoint::resolver)
pub use endpoint::resolver::{RendezvousHandle, RendezvousResolver};

// Trace normalization (requires std)
#[cfg(feature = "std")]
pub use observe::normalise::{
    DelegationEvent, EndpointEquivalenceKey, EndpointEvent, EndpointEventKind,
    ForwardEquivalenceKey, ForwardEvent, LaneEvent, MgmtPolicySummary, PolicyLaneRecord,
    ScopeCorrelatedTraces, correlate_scope_traces, delegation_trace, endpoint_trace, forward_trace,
    lane_trace, mgmt_policy_summary, mgmt_policy_trace, policy_lane_trace, policy_trace,
};
