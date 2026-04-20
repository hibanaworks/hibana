#![cfg_attr(not(feature = "std"), no_std)]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(unexpected_cfgs)]
#![recursion_limit = "256"]

//! Hibana — type-safe async MPST for `no_std` environments.
//!
//! The crate has two intended faces:
//!
//! - **App surface**: [`g`] plus [`Endpoint`] and its localside core API
//!   (`flow().send()`, `recv()`, `offer()`, `decode()`).
//! - **Substrate surface**: [`g::advanced`] plus [`substrate`] for protocol
//!   implementors that need projection, attach/enter, binding, resolver, and
//!   policy seams.
//!
//! Application code should stay on `hibana::g` and `hibana::Endpoint`.
//! Protocol crates should stay on `hibana::g::advanced` and `hibana::substrate`.
//! The crate root stays intentionally small; protocol seams live under
//! `hibana::g::advanced` and `hibana::substrate`.
//! Inside [`substrate`], everyday protocol-implementor owners stay at the
//! module root plus `runtime`, `binding`, `policy`, `wire`, and `transport`;
//! heavier detail buckets stay under `substrate::cap::advanced` and
//! `substrate::policy::core`.
//!
//! ```rust,ignore
//! use hibana::g;
//!
//! // App authors stay on the choreography DSL.
//! let app = g::seq(
//!     g::send::<g::Role<0>, g::Role<1>, g::Msg<1, u32>, 0>(),
//!     g::send::<g::Role<1>, g::Role<0>, g::Msg<2, ()>, 0>(),
//! );
//!
//! // A protocol crate composes transport/appkit prefixes internally and returns
//! // an attached endpoint. From there, stay on localside core API only.
//! let _send = endpoint.flow::<g::Msg<1, u32>>()?.send(&42).await?;
//! let received = endpoint.recv::<g::Msg<2, ()>>().await?;
//! let () = received;
//! ```
//!
//! Protocol implementors use [`g::advanced`] and [`substrate`] to compose
//! [`g::Program`] values, project them, and enter attached endpoints. That
//! lower-level flow is documented in `README.md`; it is not the primary
//! app-author path.
//!
//! Run the verification flow documented in `AGENTS.md` after integrating a
//! protocol to ensure rendezvous and localside invariants stay intact. The same
//! canonical validation flow is listed in `README.md`.
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
//! - **Local-only runtime**: attached runtime owners are intentionally
//!   `!Send`/`!Sync`; `hibana` assumes single-core, non-ISR, non-reentrant
//!   execution with owner-centralized mutation.
//! - **Observability by construction**: every control decision (loop continue,
//!   cancel pair, checkpoint/rollback, splice) is annotated in the synthesized
//!   effect list so the runtime can seed tap events deterministically.
//! - **Capability discipline**: control messages carry capability tokens whose
//!   shot and permissions are embedded in the const metadata, enabling both
//!   static validation and EPF policy enforcement.
//!
//! # Cargo Features
//!
//! - `std` — Enables transport/testing utilities and observability
//!   normalisers. The runtime remains slab-backed and `no_alloc` oriented in
//!   both modes; `std` does not switch the core localside path to heap-backed
//!   ownership.

#[cfg(test)]
extern crate std;

// ============================================================================
// Public modules (application-facing)
// ============================================================================

pub mod g;
/// Global-to-Local projection (MPST theory layer)
mod global;
/// Protocol-neutral substrate surface for protocol implementors.
pub mod substrate;

mod control;

mod runtime;

mod transport;

mod local;

/// Session endpoints (affine-typed consuming futures)
mod endpoint;

mod observe;

mod policy_runtime;

/// Transport binding layer.
mod binding;

// ============================================================================
// Internal modules (NOT for direct user access)
// ============================================================================

mod eff;

/// Rendezvous (internal state machine, evaluates CpEffect)
///
/// **INTERNAL IMPLEMENTATION - DO NOT USE DIRECTLY**
///
/// This module contains the internal implementation of the Rendezvous state machine.
/// It evaluates `CpEffect` operations and manages local state (lane/gen/cap/splice).
///
/// **For application code**, use:
/// - [`Endpoint`] for localside choreography execution
/// - [`substrate::SessionKit`] for Rendezvous coordination
///
/// This module stays internal; tests reach it through crate-private coverage,
/// not through a third public face.
mod rendezvous;

// ============================================================================
// Re-exports (curated public API)
// ============================================================================

// Endpoint facade
pub use endpoint::{Endpoint, RecvError, RecvResult, RouteBranch, SendError, SendResult};
