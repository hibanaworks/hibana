use super::{Generation, Lane, SnapshotFinalization, SnapshotRecord, StateSnapshotTable};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SnapshotFinalizeTarget {
    Restore,
    Commit,
}

impl SnapshotFinalizeTarget {
    #[inline]
    const fn reserved_state(self) -> SnapshotFinalization {
        match self {
            Self::Restore => SnapshotFinalization::RestoreReserved,
            Self::Commit => SnapshotFinalization::CommitReserved,
        }
    }

    #[inline]
    const fn published_state(self) -> SnapshotFinalization {
        match self {
            Self::Restore => SnapshotFinalization::Restored,
            Self::Commit => SnapshotFinalization::Committed,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PreparedSnapshotRecord {
    slot: u16,
    lane: Lane,
    snapshot: Generation,
    cap_revision: u64,
    previous_finalization: SnapshotFinalization,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PreparedSnapshotFinalization {
    slot: u16,
    lane: Lane,
    snapshot: Generation,
    cap_revision: u64,
    target: SnapshotFinalizeTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PublishedSnapshotRecord {
    lane: Lane,
    snapshot: Generation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PublishedSnapshotFinalization {
    lane: Lane,
    snapshot: Generation,
    cap_revision: u64,
}

impl PublishedSnapshotRecord {
    #[inline]
    pub(crate) const fn lane(self) -> Lane {
        self.lane
    }

    #[inline]
    pub(crate) const fn generation(self) -> Generation {
        self.snapshot
    }
}

impl PublishedSnapshotFinalization {
    #[inline]
    pub(crate) const fn lane(self) -> Lane {
        self.lane
    }

    #[inline]
    pub(crate) const fn generation(self) -> Generation {
        self.snapshot
    }

    #[inline]
    pub(crate) const fn cap_revision(self) -> u64 {
        self.cap_revision
    }
}

impl PreparedSnapshotFinalization {
    #[inline]
    pub(crate) const fn cap_revision(&self) -> u64 {
        self.cap_revision
    }
}

impl StateSnapshotTable {
    #[inline]
    pub(crate) fn reserve_record(
        &self,
        lane: Lane,
        snapshot: Generation,
        cap_revision: u64,
    ) -> Option<PreparedSnapshotRecord> {
        let slot = self
            .lane_slot(lane)
            .expect("state snapshot record reservation missing lane storage");
        let previous = self.read_record(slot);
        let previous_finalization = SnapshotFinalization::from_u8(previous.finalization);
        if matches!(
            previous_finalization,
            SnapshotFinalization::RecordReserved
                | SnapshotFinalization::RestoreReserved
                | SnapshotFinalization::CommitReserved
        ) {
            return None;
        }
        self.write_record(
            slot,
            SnapshotRecord {
                snapshot: previous.snapshot,
                cap_revision: previous.cap_revision,
                present: previous.present,
                finalization: SnapshotFinalization::RecordReserved as u8,
            },
        );
        Some(PreparedSnapshotRecord {
            slot: slot as u16,
            lane,
            snapshot,
            cap_revision,
            previous_finalization,
        })
    }

    #[inline]
    pub(crate) fn rollback_record_reserved(&self, ticket: PreparedSnapshotRecord) {
        let slot = ticket.slot as usize;
        let record = self.read_record(slot);
        assert_eq!(
            SnapshotFinalization::from_u8(record.finalization),
            SnapshotFinalization::RecordReserved
        );
        self.write_record(
            slot,
            SnapshotRecord {
                snapshot: record.snapshot,
                cap_revision: record.cap_revision,
                present: record.present,
                finalization: ticket.previous_finalization as u8,
            },
        );
    }

    #[inline]
    pub(crate) fn publish_record_reserved(
        &self,
        ticket: PreparedSnapshotRecord,
    ) -> PublishedSnapshotRecord {
        let slot = ticket.slot as usize;
        let record = self.read_record(slot);
        assert_eq!(
            SnapshotFinalization::from_u8(record.finalization),
            SnapshotFinalization::RecordReserved
        );
        self.write_record(
            slot,
            SnapshotRecord {
                snapshot: ticket.snapshot.raw(),
                cap_revision: ticket.cap_revision,
                present: 1,
                finalization: SnapshotFinalization::Available as u8,
            },
        );
        PublishedSnapshotRecord {
            lane: ticket.lane,
            snapshot: ticket.snapshot,
        }
    }

    #[inline]
    pub(crate) fn reserve_finalization(
        &self,
        lane: Lane,
        snapshot: Generation,
        target: SnapshotFinalizeTarget,
    ) -> Option<PreparedSnapshotFinalization> {
        let slot = self.lane_slot(lane)?;
        let record = self.read_record(slot);
        if record.present == 0
            || Generation::new(record.snapshot) != snapshot
            || SnapshotFinalization::from_u8(record.finalization) != SnapshotFinalization::Available
        {
            return None;
        }
        self.write_record(
            slot,
            SnapshotRecord {
                snapshot: record.snapshot,
                cap_revision: record.cap_revision,
                present: record.present,
                finalization: target.reserved_state() as u8,
            },
        );
        Some(PreparedSnapshotFinalization {
            slot: slot as u16,
            lane,
            snapshot,
            cap_revision: record.cap_revision,
            target,
        })
    }

    #[inline]
    pub(crate) fn rollback_finalization_reserved(&self, ticket: PreparedSnapshotFinalization) {
        let slot = ticket.slot as usize;
        let record = self.read_record(slot);
        assert_eq!(record.snapshot, ticket.snapshot.raw());
        assert_eq!(record.cap_revision, ticket.cap_revision);
        assert_eq!(
            SnapshotFinalization::from_u8(record.finalization),
            ticket.target.reserved_state()
        );
        self.write_record(
            slot,
            SnapshotRecord {
                snapshot: ticket.snapshot.raw(),
                cap_revision: ticket.cap_revision,
                present: 1,
                finalization: SnapshotFinalization::Available as u8,
            },
        );
    }

    #[inline]
    pub(crate) fn publish_finalization_reserved(
        &self,
        ticket: PreparedSnapshotFinalization,
    ) -> PublishedSnapshotFinalization {
        let slot = ticket.slot as usize;
        let record = self.read_record(slot);
        assert_eq!(record.snapshot, ticket.snapshot.raw());
        assert_eq!(record.cap_revision, ticket.cap_revision);
        assert_eq!(
            SnapshotFinalization::from_u8(record.finalization),
            ticket.target.reserved_state()
        );
        self.write_record(
            slot,
            SnapshotRecord {
                snapshot: ticket.snapshot.raw(),
                cap_revision: ticket.cap_revision,
                present: 1,
                finalization: ticket.target.published_state() as u8,
            },
        );
        PublishedSnapshotFinalization {
            lane: ticket.lane,
            snapshot: ticket.snapshot,
            cap_revision: ticket.cap_revision,
        }
    }
}
