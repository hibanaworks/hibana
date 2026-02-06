//! Type aliases for ra module.
//!
//! All types use `control::types` directly to avoid duplication.
//! No duplication, no From conversions.

// Re-export control::types as rendezvous::types for backward compatibility during migration
pub use crate::control::types::{Gen as Generation, LaneId as Lane, RendezvousId, SessionId};

use crate::control::types::{AtMostOnceCommit, NoCrossLaneAliasing};

/// Invariant marker for local splice transactions evaluated inside a rendezvous.
///
/// Guarantees that lane ownership is unique (no cross-lane aliasing) and that
/// commits happen at most once per transaction.
pub struct LocalSpliceInvariant;

impl NoCrossLaneAliasing for LocalSpliceInvariant {}
impl AtMostOnceCommit for LocalSpliceInvariant {}

/// Invariant marker for cancellation transactions (begin → ack).
///
/// Cancellation also obeys the same aliasing and at-most-once guarantees.
pub struct CancelInvariant;

impl NoCrossLaneAliasing for CancelInvariant {}
impl AtMostOnceCommit for CancelInvariant {}
