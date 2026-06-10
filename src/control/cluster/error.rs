//! Control-plane error types.
//!
//! `CpError` is the internal control-plane failure catalogue. Public attach
//! failures use `AttachError`, which records the public attach operation
//! callsite so protocol integrations can propagate attach errors with `?`
//! without adding an extra context type at every call site.

use core::{fmt, panic::Location};

mod debug;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ErrorLocation {
    location: &'static Location<'static>,
}

impl ErrorLocation {
    #[inline]
    #[track_caller]
    pub(crate) fn caller() -> Self {
        Self {
            location: Location::caller(),
        }
    }

    #[inline]
    const fn file(self) -> &'static str {
        self.location.file()
    }

    #[inline]
    const fn line(self) -> u32 {
        self.location.line()
    }

    #[inline]
    const fn column(self) -> u32 {
        self.location.column()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttachOp {
    Internal,
    Rendezvous,
    Enter,
}

/// Errors raised while attaching cursor endpoints to the control cluster.
///
/// Attach failures are public evidence for rendezvous/endpoint setup. They are
/// intentionally separate from endpoint progress errors so `?` can preserve the
/// failing boundary without a wide crate-level error enum.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AttachError {
    op: AttachOp,
    location: ErrorLocation,
    kind: AttachErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttachErrorKind {
    Control(CpError),
    Rendezvous(crate::rendezvous::error::RendezvousError),
}

impl fmt::Debug for AttachError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AttachError")
            .field("operation", &self.operation())
            .field("file", &self.file())
            .field("line", &self.line())
            .field("column", &self.column())
            .field("kind", &self.kind)
            .finish()
    }
}

impl AttachError {
    #[inline]
    #[track_caller]
    pub(crate) fn control(error: CpError) -> Self {
        Self {
            op: AttachOp::Internal,
            location: ErrorLocation::caller(),
            kind: AttachErrorKind::Control(error),
        }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn rendezvous(error: crate::rendezvous::error::RendezvousError) -> Self {
        Self {
            op: AttachOp::Internal,
            location: ErrorLocation::caller(),
            kind: AttachErrorKind::Rendezvous(error),
        }
    }

    #[inline]
    pub(crate) const fn with_operation(mut self, op: AttachOp, location: ErrorLocation) -> Self {
        self.op = op;
        self.location = location;
        self
    }

    #[inline]
    pub(crate) const fn control_cause(&self) -> Option<CpError> {
        match self.kind {
            AttachErrorKind::Control(error) => Some(error),
            AttachErrorKind::Rendezvous(_) => None,
        }
    }

    #[inline]
    pub const fn operation(&self) -> &'static str {
        match self.op {
            AttachOp::Internal => "attach",
            AttachOp::Rendezvous => "rendezvous",
            AttachOp::Enter => "enter",
        }
    }

    #[inline]
    pub const fn file(&self) -> &'static str {
        self.location.file()
    }

    #[inline]
    pub const fn line(&self) -> u32 {
        self.location.line()
    }

    #[inline]
    pub const fn column(&self) -> u32 {
        self.location.column()
    }
}

impl From<CpError> for AttachError {
    #[inline]
    #[track_caller]
    fn from(err: CpError) -> Self {
        Self::control(err)
    }
}

impl From<crate::rendezvous::error::RendezvousError> for AttachError {
    #[inline]
    #[track_caller]
    fn from(err: crate::rendezvous::error::RendezvousError) -> Self {
        Self::rendezvous(err)
    }
}

/// Unified control-plane error type.
///
/// All control-plane operations (topology, state restore, and transaction
/// commit) return errors of this type, enabling uniform error handling and tap
/// integration.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CpError {
    /// Topology-related errors.
    Topology(TopologyError),

    /// State restore-related errors.
    StateRestore(StateRestoreError),

    /// Transaction commit-related errors.
    TxCommit(TxCommitError),

    /// Transaction abort-related errors.
    TxAbort(TxAbortError),

    /// Rendezvous ID mismatch (distributed operations)
    RendezvousMismatch { expected: u16, actual: u16 },

    /// Requested rendezvous was not registered in this control cluster.
    RendezvousMissing { id: u16 },

    /// Rendezvous exists but is currently protected by an affine lease.
    RendezvousBusy { id: u16 },

    /// Replay detection (duplicate operation)
    ReplayDetected { operation: u8, nonce: u32 },

    /// Generation ordering violation
    GenerationViolation { expected: u16, actual: u16 },

    /// Resource exhaustion in a specific control-plane storage area.
    ResourceExhausted { resource: ResourceScope },

    /// Capability check failed (operation not permitted for this lane).
    Authorisation { operation: u8 },

    /// Effect not supported by the target control plane.
    UnsupportedEffect(u8),

    /// Program label exceeds the rendezvous label universe.
    LabelOutOfUniverse { max: u8, actual: u8 },

    /// Policy VM requested that the operation be aborted.
    PolicyAbort { reason: u16 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResourceScope {
    RendezvousTable,
    TopologyTable,
    LaneStorage,
    ResolverTable,
    PolicyTable,
    RouteTable,
    LoopTable,
    CapTable,
    EndpointLease,
    EndpointBounds,
    EndpointMark,
    EndpointHeader,
}

impl ResourceScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RendezvousTable => "rv",
            Self::TopologyTable => "topo",
            Self::LaneStorage => "lane",
            Self::ResolverTable => "resolver",
            Self::PolicyTable => "policy",
            Self::RouteTable => "route",
            Self::LoopTable => "loop",
            Self::CapTable => "cap",
            Self::EndpointLease => "ep-lease",
            Self::EndpointBounds => "ep-bounds",
            Self::EndpointMark => "ep-mark",
            Self::EndpointHeader => "ep-header",
        }
    }
}

impl CpError {
    #[inline]
    pub const fn resource_exhausted(resource: ResourceScope) -> Self {
        Self::ResourceExhausted { resource }
    }
}

/// Topology operation errors.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TopologyError {
    /// Invalid session ID
    InvalidSession,

    /// Session not in topology-transitionable state
    InvalidState,

    /// Generation mismatch
    GenerationMismatch,

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
            crate::rendezvous::error::TopologyError::RendezvousIdMismatch { .. } => {
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

/// State restore errors.
#[derive(Clone, Copy, PartialEq, Eq)]
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
#[derive(Clone, Copy, PartialEq, Eq)]
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
#[derive(Clone, Copy, PartialEq, Eq)]
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

#[cfg(feature = "std")]
impl std::error::Error for CpError {}

// Conversions for ergonomic error propagation
impl From<TopologyError> for CpError {
    fn from(e: TopologyError) -> Self {
        Self::Topology(e)
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
