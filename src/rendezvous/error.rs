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
    /// Capability table is full.
    TableFull,
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

/// Topology operation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopologyError {
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
    /// Remote rendezvous mismatch.
    RemoteRendezvousMismatch {
        expected: RendezvousId,
        got: RendezvousId,
    },
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
