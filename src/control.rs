//! Control-plane substrate.
//!
//! This module contains the crate-private control owners used by session
//! management. Control operations are expressed as small effects and reservation
//! proofs; typestate witnesses are minted only by owners that have already
//! validated the relevant lane, generation, or terminal reservation.
//!
//! ## Architecture
//!
//! - **effects**: Primitive control-plane operations (Open, TopologyBegin, TxCommit, StateSnapshot, etc.)
//!   - 14 primitive effects covering all control operations
//!   - Idempotency, generation bump, and history modification properties
//! - **error**: Unified error handling (`CpError`, `TopologyError`, etc.)
//!   - Consolidates all control-plane errors for uniform handling
//!   - Includes replay detection and RID mismatch errors
//! - **types**: Witness markers and compact identifiers
//!   - Marker traits used by crate-private typestate witnesses
//!   - Newtypes for Lane, Generation, RendezvousId
//! - **txn**: Typestate-based transaction protocol
//!   - Linear state transitions: Txn → InBegin → InAcked → Closed
//!   - Single-use terminal transitions
//!
//! ## Invariant Registry
//!
//! - **NoCrossLaneAliasing**: carried only by owner-minted typestate witnesses
//! - **AtMostOnceCommit**: terminal transition consumes the witness by value
//! - **IncreasingGen**: generation transition validated by rendezvous owners
//! - **One**: one-shot transition marker consumed by terminal phases
//!
//! ## Design Principles
//!
//! 1. **Effect-based decomposition**: All operations map to `ControlOp` enum
//! 2. **Typed owner phases**: marker traits name checks made by the owner
//! 3. **Single control kernel**: All external effects collapse into `ControlOp`
//!
//! ## Architecture Notes
//!
//! - Effect decomposition: All 14 control operations mapped to `ControlOp`
//! - Typed witness markers: NoCrossLaneAliasing, AtMostOnceCommit, Shot discipline
//! - Unified errors: `CpError` consolidates all control-plane errors
//! - Tap integration for distributed topology/cap/deleg events
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::control::automaton::txn::Txn;
//! use crate::control::types::{IncreasingGen, One};
//!
//! let txn: Txn<MyInv, IncreasingGen, One> = /* ... */;
//! let in_begin = txn.begin();
//! let in_acked = in_begin.ack();
//! let closed = in_acked.commit();
//! ```

/// Typestate automata for control operations.
pub(crate) mod automaton;
pub(crate) mod brand;
/// Capability resources and payloads.
pub(crate) mod cap;
/// Control-plane cluster coordination.
pub(crate) mod cluster;
/// Lease planning and capacity checks.
pub(crate) mod lease;
/// Control-plane types and invariants.
pub(crate) mod types;
