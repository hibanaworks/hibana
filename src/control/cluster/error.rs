//! Unified error types for control-plane operations.
//!
//! This module consolidates all control-plane errors into a single `CpError` enum,
//! making replay detection and rendezvous-ID mismatches part of a unified error surface.

use crate::control::cluster::effects::CpEffect;

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
/// All control-plane operations (splice, delegation, cancellation, checkpoint, etc.)
/// return errors of this type, enabling uniform error handling and tap integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpError {
    /// Splice-related errors
    Splice(SpliceError),

    /// Cancellation-related errors
    Cancel(CancelError),

    /// Checkpoint-related errors
    Checkpoint(CheckpointError),

    /// Rollback-related errors
    Rollback(RollbackError),

    /// Commit-related errors
    Commit(CommitError),

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

    /// Capability check failed (effect not permitted for this lane).
    Authorisation { effect: CpEffect },

    /// Effect not supported by the target control plane.
    UnsupportedEffect(u8),

    /// Policy VM requested that the operation be aborted.
    PolicyAbort { reason: u16 },

    /// Resource kind mismatch in typed token pipeline.
    ResourceMismatch { expected: u8, actual: u8 },
}

/// Splice operation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpliceError {
    /// Invalid session ID
    InvalidSession,

    /// Invalid lane ID
    InvalidLane,

    /// Session not in spliceable state
    InvalidState,

    /// Generation mismatch
    GenerationMismatch,

    /// Distributed splice: ACK timeout
    AckTimeout,

    /// Distributed splice: commit failed
    CommitFailed,

    /// Lane out of range
    LaneOutOfRange,

    /// Lane mismatch
    LaneMismatch,

    /// Splice already in progress
    InProgress,

    /// No pending splice
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

    /// Pending splice table full
    PendingTableFull,
}

impl From<crate::rendezvous::error::SpliceError> for SpliceError {
    fn from(err: crate::rendezvous::error::SpliceError) -> Self {
        match err {
            crate::rendezvous::error::SpliceError::LaneOutOfRange { .. } => {
                SpliceError::LaneOutOfRange
            }
            crate::rendezvous::error::SpliceError::UnknownSession { .. } => {
                SpliceError::InvalidSession
            }
            crate::rendezvous::error::SpliceError::LaneMismatch { .. } => SpliceError::LaneMismatch,
            crate::rendezvous::error::SpliceError::InProgress { .. } => SpliceError::InProgress,
            crate::rendezvous::error::SpliceError::NoPending { .. } => SpliceError::NoPending,
            crate::rendezvous::error::SpliceError::StaleGeneration { .. } => {
                SpliceError::StaleGeneration
            }
            crate::rendezvous::error::SpliceError::GenerationOverflow { .. } => {
                SpliceError::GenerationOverflow
            }
            crate::rendezvous::error::SpliceError::InvalidInitial { .. } => {
                SpliceError::InvalidInitial
            }
            crate::rendezvous::error::SpliceError::RemoteRendezvousMismatch { .. }
            | crate::rendezvous::error::SpliceError::RendezvousIdMismatch { .. } => {
                SpliceError::RendezvousIdMismatch
            }
            crate::rendezvous::error::SpliceError::SeqnoMismatch { .. } => {
                SpliceError::SeqnoMismatch
            }
            crate::rendezvous::error::SpliceError::PendingTableFull => {
                SpliceError::PendingTableFull
            }
        }
    }
}

/// Cancellation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelError {
    /// Session not found
    SessionNotFound,

    /// Generation mismatch in ACK
    GenerationMismatch,

    /// Already cancelled
    AlreadyCancelled,
}

/// Checkpoint errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckpointError {
    /// Session not found
    SessionNotFound,

    /// Checkpoint table full
    TableFull,

    /// Invalid session state for checkpoint
    InvalidState,
}

/// Rollback errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollbackError {
    /// Session not found
    SessionNotFound,

    /// Epoch (generation) not found
    EpochNotFound,

    /// Epoch mismatch (not aligned)
    EpochMismatch,

    /// Cannot rollback after commit
    AfterCommit,
}

/// Commit errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitError {
    /// Session not found
    SessionNotFound,

    /// No checkpoint available to commit
    NoCheckpoint,

    /// Checkpoint already committed
    AlreadyCommitted,

    /// Provided generation does not match recorded checkpoint
    GenerationMismatch,
}

/// Delegation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelegationError {
    /// Invalid capability token
    InvalidToken,

    /// Capability already claimed
    AlreadyClaimed,

    /// Capability exhausted (one-shot)
    Exhausted,

    /// Shot discipline violation
    ShotMismatch,
}

impl core::fmt::Display for CpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Splice(e) => write!(f, "Splice error: {:?}", e),
            Self::Cancel(e) => write!(f, "Cancel error: {:?}", e),
            Self::Checkpoint(e) => write!(f, "Checkpoint error: {:?}", e),
            Self::Rollback(e) => write!(f, "Rollback error: {:?}", e),
            Self::Commit(e) => write!(f, "Commit error: {:?}", e),
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
            Self::Authorisation { effect } => {
                write!(f, "Effect not authorised: {:?}", effect)
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
impl From<SpliceError> for CpError {
    fn from(e: SpliceError) -> Self {
        Self::Splice(e)
    }
}

impl From<CancelError> for CpError {
    fn from(e: CancelError) -> Self {
        Self::Cancel(e)
    }
}

impl From<CheckpointError> for CpError {
    fn from(e: CheckpointError) -> Self {
        Self::Checkpoint(e)
    }
}

impl From<RollbackError> for CpError {
    fn from(e: RollbackError) -> Self {
        Self::Rollback(e)
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
        let splice_err: CpError = SpliceError::InvalidSession.into();
        assert!(matches!(splice_err, CpError::Splice(_)));

        let cancel_err: CpError = CancelError::SessionNotFound.into();
        assert!(matches!(cancel_err, CpError::Cancel(_)));
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
