use super::{Generation, Lane, PreparedSnapshotFinalization, PreparedSnapshotRecord, SessionId};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PreparedAbortBeginEffect {
    pub(in crate::rendezvous::core) sid: SessionId,
    pub(in crate::rendezvous::core) lane: Lane,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PreparedAbortAckEffect {
    pub(in crate::rendezvous::core) sid: SessionId,
    pub(in crate::rendezvous::core) lane: Lane,
    pub(in crate::rendezvous::core) generation: Generation,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PreparedStateSnapshotEffect {
    pub(in crate::rendezvous::core) sid: SessionId,
    pub(in crate::rendezvous::core) reservation: PreparedSnapshotRecord,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PreparedTxCommitEffect {
    pub(in crate::rendezvous::core) sid: SessionId,
    pub(in crate::rendezvous::core) reservation: PreparedSnapshotFinalization,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PreparedStateRestoreEffect {
    pub(in crate::rendezvous::core) sid: SessionId,
    pub(in crate::rendezvous::core) reservation: PreparedSnapshotFinalization,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PreparedTxAbortEffect {
    pub(in crate::rendezvous::core) sid: SessionId,
    pub(in crate::rendezvous::core) reservation: PreparedSnapshotFinalization,
}

impl PreparedAbortBeginEffect {
    #[inline]
    pub(crate) const fn sid(&self) -> SessionId {
        self.sid
    }

    #[inline]
    pub(crate) const fn lane(&self) -> Lane {
        self.lane
    }
}

impl PreparedAbortAckEffect {
    #[inline]
    pub(crate) const fn sid(&self) -> SessionId {
        self.sid
    }

    #[inline]
    pub(crate) const fn lane(&self) -> Lane {
        self.lane
    }

    #[inline]
    pub(crate) const fn generation(&self) -> Generation {
        self.generation
    }
}

impl PreparedStateSnapshotEffect {
    #[inline]
    pub(crate) const fn sid(&self) -> SessionId {
        self.sid
    }

    #[inline]
    pub(in crate::rendezvous::core) fn into_reservation(self) -> PreparedSnapshotRecord {
        self.reservation
    }
}

impl PreparedTxCommitEffect {
    #[inline]
    pub(crate) const fn sid(&self) -> SessionId {
        self.sid
    }

    #[inline]
    pub(in crate::rendezvous::core) fn into_reservation(self) -> PreparedSnapshotFinalization {
        self.reservation
    }
}

impl PreparedStateRestoreEffect {
    #[inline]
    pub(crate) const fn sid(&self) -> SessionId {
        self.sid
    }

    #[inline]
    pub(in crate::rendezvous::core) fn into_reservation(self) -> PreparedSnapshotFinalization {
        self.reservation
    }
}

impl PreparedTxAbortEffect {
    #[inline]
    pub(crate) const fn sid(&self) -> SessionId {
        self.sid
    }

    #[inline]
    pub(in crate::rendezvous::core) fn into_reservation(self) -> PreparedSnapshotFinalization {
        self.reservation
    }
}
