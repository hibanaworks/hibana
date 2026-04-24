//! Unified error types for control-plane operations.
//!
//! This module consolidates all control-plane errors into a single `CpError` enum,
//! making replay detection and rendezvous-ID mismatches part of a unified error surface.

/// Errors raised while attaching cursor endpoints to the control cluster.
#[derive(Debug)]
pub enum AttachError {
    Control(CpError),
    Rendezvous(crate::rendezvous::error::RendezvousError),
}

impl From<CpError> for AttachError {
    fn from(err: CpError) -> Self {
        Self::Control(err)
    }
}

impl From<crate::rendezvous::error::RendezvousError> for AttachError {
    fn from(err: crate::rendezvous::error::RendezvousError) -> Self {
        Self::Rendezvous(err)
    }
}

/// Unified control-plane error type.
///
/// All control-plane operations (topology, delegation, abort, state snapshot,
/// state restore, and transaction commit) return errors of this type, enabling
/// uniform error handling and tap integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpError {
    /// Topology-related errors.
    Topology(TopologyError),

    /// Abort-related errors.
    Abort(AbortError),

    /// State snapshot-related errors.
    StateSnapshot(StateSnapshotError),

    /// State restore-related errors.
    StateRestore(StateRestoreError),

    /// Transaction commit-related errors.
    TxCommit(TxCommitError),

    /// Transaction abort-related errors.
    TxAbort(TxAbortError),

    /// Delegation-related errors
    Delegation(DelegationError),

    /// Rendezvous ID mismatch (distributed operations)
    RendezvousMismatch { expected: u16, actual: u16 },

    /// Replay detection (duplicate operation)
    ReplayDetected { operation: u8, nonce: u32 },

    /// Generation ordering violation
    GenerationViolation { expected: u16, actual: u16 },

    /// Resource exhaustion (table full, etc.)
    ResourceExhausted,

    /// Capability check failed (operation not permitted for this lane).
    Authorisation { operation: u8 },

    /// Effect not supported by the target control plane.
    UnsupportedEffect(u8),

    /// Policy VM requested that the operation be aborted.
    PolicyAbort { reason: u16 },

    /// Resource kind mismatch in typed token pipeline.
    ResourceMismatch { expected: u8, actual: u8 },
}

/// Topology operation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopologyError {
    /// Invalid session ID
    InvalidSession,

    /// Invalid lane ID
    InvalidLane,

    /// Session not in topology-transitionable state
    InvalidState,

    /// Generation mismatch
    GenerationMismatch,

    /// Distributed topology transition: ACK timeout
    AckTimeout,

    /// Distributed topology transition: commit failed
    CommitFailed,

    /// Lane out of range
    LaneOutOfRange,

    /// Lane mismatch
    LaneMismatch,

    /// Topology transition already in progress
    InProgress,

    /// No pending topology transition
    NoPending,

    /// Stale generation
    StaleGeneration,

    /// Generation overflow
    GenerationOverflow,

    /// Invalid initial generation
    InvalidInitial,

    /// Rendezvous ID mismatch
    RendezvousIdMismatch,

    /// Sequence number mismatch
    SeqnoMismatch,

    /// Pending topology table full
    PendingTableFull,
}

impl From<crate::rendezvous::error::TopologyError> for TopologyError {
    fn from(err: crate::rendezvous::error::TopologyError) -> Self {
        match err {
            crate::rendezvous::error::TopologyError::LaneOutOfRange { .. } => {
                TopologyError::LaneOutOfRange
            }
            crate::rendezvous::error::TopologyError::UnknownSession { .. } => {
                TopologyError::InvalidSession
            }
            crate::rendezvous::error::TopologyError::LaneMismatch { .. } => {
                TopologyError::LaneMismatch
            }
            crate::rendezvous::error::TopologyError::InProgress { .. } => TopologyError::InProgress,
            crate::rendezvous::error::TopologyError::NoPending { .. } => TopologyError::NoPending,
            crate::rendezvous::error::TopologyError::StaleGeneration { .. } => {
                TopologyError::StaleGeneration
            }
            crate::rendezvous::error::TopologyError::GenerationOverflow { .. } => {
                TopologyError::GenerationOverflow
            }
            crate::rendezvous::error::TopologyError::InvalidInitial { .. } => {
                TopologyError::InvalidInitial
            }
            crate::rendezvous::error::TopologyError::RemoteRendezvousMismatch { .. }
            | crate::rendezvous::error::TopologyError::RendezvousIdMismatch { .. } => {
                TopologyError::RendezvousIdMismatch
            }
            crate::rendezvous::error::TopologyError::SeqnoMismatch { .. } => {
                TopologyError::SeqnoMismatch
            }
            crate::rendezvous::error::TopologyError::PendingTableFull => {
                TopologyError::PendingTableFull
            }
        }
    }
}

/// Abort errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbortError {
    /// Session not found
    SessionNotFound,

    /// Generation mismatch in ACK
    GenerationMismatch,
}

/// State snapshot errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateSnapshotError {
    /// Session not found
    SessionNotFound,
}

/// State restore errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateRestoreError {
    /// Session not found
    SessionNotFound,

    /// State snapshot epoch not found
    EpochNotFound,

    /// Epoch mismatch (not aligned)
    EpochMismatch,

    /// State snapshot already finalized by a prior restore or commit
    AlreadyFinalized,
}

/// Transaction commit errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxCommitError {
    /// Session not found
    SessionNotFound,

    /// No state snapshot available to commit
    NoStateSnapshot,

    /// State snapshot already finalized by a prior restore or commit
    AlreadyFinalized,

    /// Provided generation does not match the recorded state snapshot
    GenerationMismatch,
}

/// Transaction abort errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxAbortError {
    /// Session not found
    SessionNotFound,

    /// No state snapshot available to abort back to
    NoStateSnapshot,

    /// State snapshot already finalized by a prior restore, abort, or commit
    AlreadyFinalized,

    /// Provided generation does not match the recorded state snapshot
    GenerationMismatch,
}

/// Delegation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelegationError {
    /// Invalid capability token
    InvalidToken,

    /// Capability exhausted (one-shot)
    Exhausted,

    /// Shot discipline violation
    ShotMismatch,
}

impl core::fmt::Display for CpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Topology(e) => write!(f, "Topology error: {:?}", e),
            Self::Abort(e) => write!(f, "Abort error: {:?}", e),
            Self::StateSnapshot(e) => write!(f, "StateSnapshot error: {:?}", e),
            Self::StateRestore(e) => write!(f, "StateRestore error: {:?}", e),
            Self::TxCommit(e) => write!(f, "TxCommit error: {:?}", e),
            Self::TxAbort(e) => write!(f, "TxAbort error: {:?}", e),
            Self::Delegation(e) => write!(f, "Delegation error: {:?}", e),
            Self::RendezvousMismatch { expected, actual } => {
                write!(
                    f,
                    "Rendezvous ID mismatch: expected {}, got {}",
                    expected, actual
                )
            }
            Self::ReplayDetected { operation, nonce } => {
                write!(
                    f,
                    "Replay detected: operation {}, nonce {}",
                    operation, nonce
                )
            }
            Self::GenerationViolation { expected, actual } => {
                write!(
                    f,
                    "Generation ordering violation: expected {}, got {}",
                    expected, actual
                )
            }
            Self::ResourceExhausted => write!(f, "Resource exhausted"),
            Self::Authorisation { operation } => {
                write!(f, "Operation not authorised: {}", operation)
            }
            Self::UnsupportedEffect(op) => write!(f, "Unsupported effect: {}", op),
            Self::PolicyAbort { reason } => write!(f, "Policy abort requested (reason {})", reason),
            Self::ResourceMismatch { expected, actual } => {
                write!(
                    f,
                    "Resource kind mismatch: expected tag {}, got {}",
                    expected, actual
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CpError {}

// Conversions for ergonomic error propagation
impl From<TopologyError> for CpError {
    fn from(e: TopologyError) -> Self {
        Self::Topology(e)
    }
}

impl From<AbortError> for CpError {
    fn from(e: AbortError) -> Self {
        Self::Abort(e)
    }
}

impl From<StateSnapshotError> for CpError {
    fn from(e: StateSnapshotError) -> Self {
        Self::StateSnapshot(e)
    }
}

impl From<StateRestoreError> for CpError {
    fn from(e: StateRestoreError) -> Self {
        Self::StateRestore(e)
    }
}

impl From<TxCommitError> for CpError {
    fn from(e: TxCommitError) -> Self {
        Self::TxCommit(e)
    }
}

impl From<TxAbortError> for CpError {
    fn from(e: TxAbortError) -> Self {
        Self::TxAbort(e)
    }
}

impl From<DelegationError> for CpError {
    fn from(e: DelegationError) -> Self {
        Self::Delegation(e)
    }
}

// Tests use `format!` which requires `alloc`/`std`. Gate them behind `std` so
// that rust-analyzer (no_std default) doesn't flag errors, while CI runs them
// under `--all-features` (which enables `std`).
#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_error_conversions() {
        let topology_err: CpError = TopologyError::InvalidSession.into();
        assert!(matches!(topology_err, CpError::Topology(_)));

        let abort_err: CpError = AbortError::SessionNotFound.into();
        assert!(matches!(abort_err, CpError::Abort(_)));
    }

    #[test]
    fn test_error_display() {
        let err = CpError::RendezvousMismatch {
            expected: 1,
            actual: 2,
        };
        let s = format!("{}", err);
        assert!(s.contains("expected 1"));
        assert!(s.contains("got 2"));
    }
}
