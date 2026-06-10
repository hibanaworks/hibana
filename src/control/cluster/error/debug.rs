use core::fmt;

use super::{
    CpError, ResourceScope, StateRestoreError, TopologyError, TxAbortError, TxCommitError,
};

impl TopologyError {
    const fn code(self) -> &'static str {
        match self {
            Self::InvalidSession => "bad-sid",
            Self::InvalidState => "bad-state",
            Self::GenerationMismatch => "gen",
            Self::CommitFailed => "commit",
            Self::LaneOutOfRange => "lane-range",
            Self::LaneMismatch => "lane",
            Self::InProgress => "busy",
            Self::NoPending => "none",
            Self::StaleGeneration => "stale-gen",
            Self::GenerationOverflow => "gen-overflow",
            Self::InvalidInitial => "bad-init",
            Self::RendezvousIdMismatch => "rv",
            Self::SeqnoMismatch => "seqno",
            Self::PendingTableFull => "pending-full",
        }
    }
}

impl fmt::Debug for TopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl StateRestoreError {
    const fn code(self) -> &'static str {
        match self {
            Self::SessionNotFound => "sid",
            Self::EpochNotFound => "epoch",
            Self::EpochMismatch => "epoch-mismatch",
            Self::AlreadyFinalized => "final",
        }
    }
}

impl fmt::Debug for StateRestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl TxCommitError {
    const fn code(self) -> &'static str {
        match self {
            Self::SessionNotFound => "sid",
            Self::NoStateSnapshot => "snapshot",
            Self::AlreadyFinalized => "final",
            Self::GenerationMismatch => "gen",
        }
    }
}

impl fmt::Debug for TxCommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl TxAbortError {
    const fn code(self) -> &'static str {
        match self {
            Self::SessionNotFound => "sid",
            Self::NoStateSnapshot => "snapshot",
            Self::AlreadyFinalized => "final",
            Self::GenerationMismatch => "gen",
        }
    }
}

impl fmt::Debug for TxAbortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl fmt::Display for CpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Topology(e) => write!(f, "top:{}", e.code()),
            Self::StateRestore(e) => write!(f, "restore:{}", e.code()),
            Self::TxCommit(e) => write!(f, "tx-commit:{}", e.code()),
            Self::TxAbort(e) => write!(f, "tx-abort:{}", e.code()),
            Self::RendezvousMismatch { expected, actual } => {
                write!(f, "rv-mismatch expected {} got {}", expected, actual)
            }
            Self::RendezvousMissing { id } => write!(f, "rv-missing {}", id),
            Self::RendezvousBusy { id } => write!(f, "rv-busy {}", id),
            Self::ReplayDetected { operation, nonce } => {
                write!(f, "replay op {} nonce {}", operation, nonce)
            }
            Self::GenerationViolation { expected, actual } => {
                write!(f, "gen expected {} got {}", expected, actual)
            }
            Self::ResourceExhausted { resource } => write!(f, "exhausted {}", resource.as_str()),
            Self::Authorisation { operation } => write!(f, "auth {}", operation),
            Self::UnsupportedEffect(op) => write!(f, "effect {}", op),
            Self::LabelOutOfUniverse { max, actual } => {
                write!(f, "label {} > rv-label {}", actual, max)
            }
            Self::PolicyAbort { reason } => write!(f, "policy-abort {}", reason),
        }
    }
}

impl fmt::Debug for CpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Debug for ResourceScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
