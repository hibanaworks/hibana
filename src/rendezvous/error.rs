//! Error types for ra module.

use crate::control::types::{Generation, Lane, RendezvousId, SessionId};

/// Generation update record for error reporting.
#[cfg(all(test, hibana_repo_tests))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GenerationRecord {
    pub lane: Lane,
    pub last: Generation,
    pub new: Generation,
}

/// Generation table errors.
#[cfg(all(test, hibana_repo_tests))]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RendezvousError {
    /// Lane out of configured range.
    LaneOutOfRange { lane: Lane },
    /// Lane already in use.
    LaneBusy { lane: Lane },
    /// Session generation has already faulted and cannot accept more progress.
    SessionPoisoned { sid: SessionId },
}

/// Topology operation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TopologyError {
    /// Lane out of range.
    LaneOutOfRange { lane: Lane },
    /// Unknown session ID.
    UnknownSession { sid: SessionId },
    /// Lane mismatch.
    LaneMismatch { expected: Lane, provided: Lane },
    /// Topology transition already in progress.
    InProgress { lane: Lane },
    /// No pending topology transition.
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
    /// Rendezvous ID mismatch (distributed topology transition).
    RendezvousIdMismatch {
        expected: RendezvousId,
        got: RendezvousId,
    },
    /// Sequence number mismatch.
    SeqnoMismatch { seq_tx: u32, seq_rx: u32 },
    /// Pending topology table full.
    PendingTableFull,
}

/// State restore operation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateRestoreError {
    /// Session is not associated with the target lane.
    UnknownSession { sid: SessionId },
    /// No state snapshot found (cannot restore without a prior snapshot).
    NoStateSnapshot { sid: SessionId },
    /// Stale state snapshot (requested epoch doesn't match current snapshot).
    StaleStateSnapshot {
        sid: SessionId,
        requested: Generation,
        current: Generation,
    },
    /// State snapshot already finalized by a prior restore or commit.
    AlreadyFinalized { sid: SessionId },
    /// Epoch mismatch (requested epoch doesn't match current generation).
    EpochMismatch {
        expected: Generation,
        got: Generation,
    },
}

/// Transaction commit operation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TxCommitError {
    /// Session is not associated with the target lane.
    UnknownSession { sid: SessionId },
    /// No state snapshot recorded for the session.
    NoStateSnapshot { sid: SessionId },
    /// State snapshot already finalized by a prior restore or commit.
    AlreadyFinalized { sid: SessionId },
    /// Provided generation mismatched the recorded state snapshot.
    GenerationMismatch {
        sid: SessionId,
        expected: Generation,
        got: Generation,
    },
}

/// Transaction abort operation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TxAbortError {
    /// Session is not associated with the target lane.
    UnknownSession { sid: SessionId },
    /// No state snapshot recorded for the session.
    NoStateSnapshot { sid: SessionId },
    /// Requested snapshot generation mismatched the recorded state snapshot.
    StaleStateSnapshot {
        sid: SessionId,
        requested: Generation,
        current: Generation,
    },
    /// State snapshot already finalized by a prior restore, abort, or commit.
    AlreadyFinalized { sid: SessionId },
    /// Requested generation is newer than the current lane generation.
    GenerationMismatch {
        sid: SessionId,
        expected: Generation,
        got: Generation,
    },
}
