//! Control-Plane Mini Kernel
//!
//! This module implements a type-safe, effect-based control-plane for session management.
//! All control operations are decomposed into atomic effects, invariants are encoded in types,
//! and unsafe operations are explicitly documented at their call sites.
//!
//! ## Architecture
//!
//! - **effects**: Atomic control-plane operations (Open, SpliceBegin, Commit, Checkpoint, etc.)
//!   - 13 primitive effects covering all control operations
//!   - Idempotency, generation bump, and history modification properties
//! - **error**: Unified error handling (`CpError`, `SpliceError`, `CancelError`, etc.)
//!   - Consolidates all control-plane errors for uniform handling
//!   - Includes replay detection and RID mismatch errors
//! - **types**: Type-level invariants (NoCrossLaneAliasing, AtMostOnceCommit, etc.)
//!   - Marker traits for compile-time safety
//!   - Newtypes for LaneId, Gen, RendezvousId, UniverseId, DomainId
//! - **txn**: Typestate-based transaction protocol
//!   - Linear state transitions: Txn → InBegin → InAcked → Closed
//!   - Shot discipline enforcement (One vs Many)
//! - **assoc**: UniqueId for cross-lane aliasing prevention
//!   - Guarantees no two lanes share the same identifier
//! - **ffi**: Single FFI boundary for external interfaces
//!   - C-compatible types with `repr(C)` and `repr(transparent)`
//!   - Handshake protocol (Hello message) with version negotiation
//!
//! ## Invariant Registry
//!
//! - **NoCrossLaneAliasing**: Proven by `UniqueId` newtype
//! - **AtMostOnceCommit**: Enforced by typestate machine
//! - **StrictlyIncreasingGen**: Maintained by `Gen::bump()` and rendezvous
//! - **OneShot/MultiShot**: Shot discipline enforced by traits
//!
//! ## Design Principles
//!
//! 1. **Effect-based decomposition**: All operations map to `CpEffect` enum
//! 2. **Type-level invariants**: Marker traits prevent misuse at compile time
//! 3. **Single FFI boundary**: All external types in `ffi.rs`
//!
//! ## Architecture Notes
//!
//! - Effect decomposition: All 13 control operations mapped to `CpEffect`
//! - Type-level invariants: NoCrossLaneAliasing, AtMostOnceCommit, Shot discipline
//! - Unified errors: `CpError` consolidates all control-plane errors
//! - ControlPlane::handshake() on Txn
//! - Tap integration for distributed splice/cap/deleg events
//!
//! ## Usage
//!
//! ```rust,ignore
//! use hibana::control::{Txn, NoopTap, IncreasingGen, One};
//!
//! let mut tap = NoopTap;
//! let txn: Txn<MyInv, IncreasingGen, One> = /* ... */;
//! let in_begin = txn.begin(&mut tap);
//! let in_acked = in_begin.ack(&mut tap);
//! let closed = in_acked.commit(&mut tap);
//! ```

/// Identifier association utilities.
pub mod assoc;
/// Typestate automata for control operations.
pub mod automaton;
pub(crate) mod brand;
/// Capability resources and payloads.
pub mod cap;
/// Control-plane cluster coordination.
pub mod cluster;
/// Control handle definitions.
pub mod handle;
/// Lease planning and capacity checks.
pub mod lease;
/// Control-plane types and invariants.
pub mod types;

// Re-export commonly used types
pub use crate::control::cluster::effects::CpEffect;
pub use crate::control::cluster::error::{
    CancelError, CheckpointError, CommitError, CpError, DelegationError, RollbackError, SpliceError,
};
pub use automaton::txn::{Closed, InAcked, InBegin, NoopTap, Tap, Txn};
pub use types::{
    AtMostOnceCommit, DomainId, Gen, IncreasingGen, LaneId, Many, MultiShot, NoCrossLaneAliasing,
    One, OneShot, RendezvousId, SessionId, StrictlyIncreasingGen, UniverseId,
};

pub use assoc::UniqueId;
pub use automaton::{
    delegation::{
        DelegateClaimAutomaton, DelegateClaimSeed, DelegateMintAutomaton, DelegateMintSeed,
        DelegatedPortWitness, DelegationGraphContext, DelegationLeaseSpec,
    },
    distributed::{DistributedSplice, SpliceAck, SpliceIntent},
    splice::{SpliceBeginAutomaton, SpliceCommitAutomaton, SpliceGraphContext},
};
pub use cap::payload::{
    CancelNotice, CheckpointAck, CheckpointProposal, CommitAck, RollbackIntent,
};
pub use cap::resource_kinds::{
    CancelAckKind, CancelKind, CheckpointKind, CommitKind, LoopBreakKind, LoopContinueKind,
    RerouteKind, RollbackKind, SpliceAckKind, SpliceIntentKind,
};
pub use cap::typed_tokens::{CapFlowToken, CapFrameToken, CapRegisteredToken};
pub use cap::{CapShot, CapsMask, GenericCapToken, HandleView, ResourceKind, VmHandleError};
pub use cluster::ffi::{Hello, ProtocolVersion};
pub use handle::{
    bag::HandleBag,
    frame::ControlFrame,
    spec::{Cons, HandleSpecList, Nil},
};
pub use lease::{
    ControlAutomaton, ControlCore, ControlStep, DelegationDriveError, DriveError, FullAccess,
    FullSpec, LeaseError, RegisterRendezvousError, RendezvousLease, RendezvousSpec,
    bundle::{
        CapsBundleHandle, LeaseBundleContext, LeaseBundleError, LeaseBundleFacet,
        LeaseGraphBundleExt, SlotBundleHandle,
    },
    graph::{LeaseGraph, LeaseGraphError, LeaseSpec},
    planner::{DELEGATION_CHILD_SET_CAPACITY, LeaseFacetNeeds, LeaseGraphBudget},
};
