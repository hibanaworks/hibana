//! Control-Plane Mini Kernel
//!
//! This module implements a type-safe, effect-based control-plane for session management.
//! All control operations are decomposed into atomic effects, invariants are encoded in types,
//! and unsafe operations are explicitly documented at their call sites.
//!
//! ## Architecture
//!
//! - **effects**: Primitive control-plane operations (Open, SpliceBegin, Commit, Checkpoint, etc.)
//!   - 13 primitive effects covering all control operations
//!   - Idempotency, generation bump, and history modification properties
//! - **error**: Unified error handling (`CpError`, `SpliceError`, `CancelError`, etc.)
//!   - Consolidates all control-plane errors for uniform handling
//!   - Includes replay detection and RID mismatch errors
//! - **types**: Type-level invariants (NoCrossLaneAliasing, AtMostOnceCommit, etc.)
//!   - Marker traits for compile-time safety
//!   - Newtypes for Lane, Generation, RendezvousId
//! - **txn**: Typestate-based transaction protocol
//!   - Linear state transitions: Txn → InBegin → InAcked → Closed
//!   - Shot discipline enforcement (One vs Many)
//!
//! ## Invariant Registry
//!
//! - **NoCrossLaneAliasing**: Marked at the type level
//! - **AtMostOnceCommit**: Enforced by typestate machine
//! - **IncreasingGen**: Maintained by `Generation::bump()` and rendezvous
//! - **One**: Single-use shot discipline enforced by the type marker
//!
//! ## Design Principles
//!
//! 1. **Effect-based decomposition**: All operations map to `ControlOp` enum
//! 2. **Type-level invariants**: Marker traits prevent misuse at compile time
//! 3. **Single control kernel**: All external effects collapse into `ControlOp`
//!
//! ## Architecture Notes
//!
//! - Effect decomposition: All 13 control operations mapped to `ControlOp`
//! - Type-level invariants: NoCrossLaneAliasing, AtMostOnceCommit, Shot discipline
//! - Unified errors: `CpError` consolidates all control-plane errors
//! - Tap integration for distributed splice/cap/deleg events
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::control::automaton::txn::{NoopTap, Txn};
//! use crate::control::types::{IncreasingGen, One};
//!
//! let mut tap = NoopTap;
//! let txn: Txn<MyInv, IncreasingGen, One> = /* ... */;
//! let in_begin = txn.begin(&mut tap);
//! let in_acked = in_begin.ack(&mut tap);
//! let closed = in_acked.commit(&mut tap);
//! ```

/// Typestate automata for control operations.
pub(crate) mod automaton;
pub(crate) mod brand;
/// Capability resources and payloads.
pub(crate) mod cap;
/// Control-plane cluster coordination.
pub(crate) mod cluster;
/// Control handle definitions.
#[cfg(test)]
pub(crate) mod handle;
/// Lease planning and capacity checks.
pub(crate) mod lease;
/// Control-plane types and invariants.
pub(crate) mod types;
