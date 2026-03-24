//! Error types for ra module.

use crate::control::types::{Generation, Lane, RendezvousId, SessionId};

/// Capability token errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapError {
    /// Token not found in table.
    UnknownToken,
    /// Session ID or lane mismatch.
    WrongSessionOrLane,
    /// One-shot token already exhausted.
    Exhausted,
    /// Token found but field mismatch (kind/shot/sid/lane).
    Mismatch,
}

/// Generation update record for error reporting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GenerationRecord {
    pub lane: Lane,
    pub last: Generation,
    pub new: Generation,
}

/// Generation table errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GenError {
    /// Stale or duplicate generation number.
    StaleOrDuplicate(GenerationRecord),
    /// Generation overflow.
    Overflow { lane: Lane, last: Generation },
    /// Invalid initial generation (not zero).
    InvalidInitial { lane: Lane, new: Generation },
}

/// Rendezvous errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RendezvousError {
    /// Lane out of configured range.
    LaneOutOfRange { lane: Lane },
    /// Lane already in use.
    LaneBusy { lane: Lane },
    /// Session already registered on different lane.
    SessionAlreadyRegistered { sid: SessionId, lane: Lane },
    /// Cluster coordination error.
    ClusterError(crate::control::cluster::error::CpError),
}

/// Splice operation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpliceError {
    /// Lane out of range.
    LaneOutOfRange { lane: Lane },
    /// Unknown session ID.
    UnknownSession { sid: SessionId },
    /// Lane mismatch.
    LaneMismatch { expected: Lane, provided: Lane },
    /// Splice already in progress.
    InProgress { lane: Lane },
    /// No pending splice.
    NoPending { lane: Lane },
    /// Stale generation.
    StaleGeneration {
        lane: Lane,
        last: Generation,
        new: Generation,
    },
    /// Generation overflow.
    GenerationOverflow { lane: Lane, last: Generation },
    /// Invalid initial generation.
    InvalidInitial { lane: Lane, new: Generation },
    /// Remote rendezvous mismatch.
    RemoteRendezvousMismatch {
        expected: RendezvousId,
        got: RendezvousId,
    },
    /// Rendezvous ID mismatch (distributed splice).
    RendezvousIdMismatch {
        expected: RendezvousId,
        got: RendezvousId,
    },
    /// Sequence number mismatch.
    SeqnoMismatch { seq_tx: u32, seq_rx: u32 },
    /// Pending splice table full.
    PendingTableFull,
}

/// Cancel operation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CancelError {
    /// Unknown session ID.
    UnknownSession { sid: SessionId },
}

/// Checkpoint operation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckpointError {
    /// Unknown session ID.
    UnknownSession { sid: SessionId },
}

/// Rollback operation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RollbackError {
    /// Unknown session ID.
    UnknownSession { sid: SessionId },
    /// No checkpoint found (cannot rollback without a checkpoint).
    NoCheckpoint { sid: SessionId },
    /// Stale checkpoint (requested epoch doesn't match current checkpoint).
    StaleCheckpoint {
        sid: SessionId,
        requested: Generation,
        current: Generation,
    },
    /// Checkpoint already consumed (idempotent rollback rejected).
    AlreadyConsumed { sid: SessionId },
    /// Epoch mismatch (requested epoch doesn't match current generation).
    EpochMismatch {
        expected: Generation,
        got: Generation,
    },
}

/// Commit operation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitError {
    /// No checkpoint recorded for the session.
    NoCheckpoint { sid: SessionId },
    /// Checkpoint already committed.
    AlreadyCommitted { sid: SessionId },
    /// Provided generation mismatched the recorded checkpoint.
    GenerationMismatch {
        sid: SessionId,
        expected: Generation,
        got: Generation,
    },
}
