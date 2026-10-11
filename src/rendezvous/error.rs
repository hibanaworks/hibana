//! Rendezvous error types.

use crate::session::types::{Lane, SessionId};

/// Rendezvous errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RendezvousError {
    /// Lane out of configured range.
    LaneOutOfRange { lane: Lane },
    /// Lane already in use.
    LaneBusy { lane: Lane },
    /// Lane attachment reference count is saturated.
    LaneAttachOverflow { lane: Lane },
    /// Session generation has already faulted and cannot accept more progress.
    SessionPoisoned { sid: SessionId },
}
