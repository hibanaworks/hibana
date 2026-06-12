use super::super::{
    ControlOp, Generation, LabelUniverse, Lane, PreparedStateSnapshotEffect, PreparedTxAbortEffect,
    PreparedTxCommitEffect, Rendezvous, SessionId, SnapshotFinalization, SnapshotFinalizeTarget,
    TopologyError, Transport, TxAbortError, TxCommitError,
};

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: super::super::Clock, E>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline]
    pub(crate) fn prepare_state_snapshot_effect(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<PreparedStateSnapshotEffect, TopologyError> {
        self.ensure_associated_session_lane(sid, lane)?;
        let current = self.lane_generation(lane);
        if current != generation {
            return Err(TopologyError::StaleGeneration {
                lane,
                last: current,
                new: generation,
            });
        }
        let reservation = self
            .state_snapshots
            .reserve_record(lane, generation, self.cap_revision.get())
            .ok_or(TopologyError::InProgress { lane })?;
        Ok(PreparedStateSnapshotEffect { sid, reservation })
    }

    #[inline]
    pub(crate) fn publish_prepared_state_snapshot_effect(
        &self,
        proof: PreparedStateSnapshotEffect,
    ) {
        let sid = proof.sid();
        let snapshot = self
            .state_snapshots
            .publish_record_reserved(proof.into_reservation());
        let lane = snapshot.lane();
        let generation = snapshot.generation();
        self.caps.discard_released_lane_entries(lane);
        self.emit_effect(ControlOp::StateSnapshot, sid, lane, generation.0 as u32);
    }

    #[inline]
    pub(crate) fn rollback_prepared_state_snapshot_effect(
        &self,
        proof: PreparedStateSnapshotEffect,
    ) {
        self.state_snapshots
            .rollback_record_reserved(proof.into_reservation());
    }

    #[inline]
    pub(crate) fn prepare_tx_commit_effect(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<PreparedTxCommitEffect, TxCommitError> {
        self.ensure_associated_session_lane(sid, lane)
            .map_err(|_| TxCommitError::UnknownSession { sid })?;
        let snapshot = self
            .state_snapshots
            .last_snapshot(lane)
            .ok_or(TxCommitError::NoStateSnapshot { sid })?;
        if !matches!(
            self.state_snapshots.finalization(lane),
            None | Some(SnapshotFinalization::Available)
        ) {
            return Err(TxCommitError::AlreadyFinalized { sid });
        }
        if snapshot != generation {
            return Err(TxCommitError::GenerationMismatch {
                sid,
                expected: snapshot,
                got: generation,
            });
        }
        let reservation = self
            .state_snapshots
            .reserve_finalization(lane, generation, SnapshotFinalizeTarget::Commit)
            .ok_or(TxCommitError::AlreadyFinalized { sid })?;
        Ok(PreparedTxCommitEffect { sid, reservation })
    }

    #[inline]
    pub(crate) fn publish_prepared_tx_commit_effect(&self, proof: PreparedTxCommitEffect) {
        let sid = proof.sid();
        let finalization = self
            .state_snapshots
            .publish_finalization_reserved(proof.into_reservation());
        let lane = finalization.lane();
        let generation = finalization.generation();
        self.caps.discard_released_lane_entries(lane);
        self.emit_effect(ControlOp::TxCommit, sid, lane, generation.0 as u32);
    }

    #[inline]
    pub(crate) fn rollback_prepared_tx_commit_effect(&self, proof: PreparedTxCommitEffect) {
        self.state_snapshots
            .rollback_finalization_reserved(proof.into_reservation());
    }

    #[inline]
    pub(crate) fn prepare_tx_abort_effect(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<PreparedTxAbortEffect, TxAbortError> {
        self.ensure_associated_session_lane(sid, lane)
            .map_err(|_| TxAbortError::UnknownSession { sid })?;
        let current = self.r#gen.last(lane).unwrap_or(Generation(0));
        let snapshot = self
            .state_snapshots
            .last_snapshot(lane)
            .ok_or(TxAbortError::NoStateSnapshot { sid })?;
        if !matches!(
            self.state_snapshots.finalization(lane),
            None | Some(SnapshotFinalization::Available)
        ) {
            return Err(TxAbortError::AlreadyFinalized { sid });
        }
        if generation != snapshot {
            return Err(TxAbortError::StaleStateSnapshot {
                sid,
                requested: generation,
                current: snapshot,
            });
        }
        if current.raw() < generation.raw() {
            return Err(TxAbortError::GenerationMismatch {
                sid,
                expected: current,
                got: generation,
            });
        }
        let cap_revision = self
            .state_snapshots
            .last_cap_revision(lane)
            .ok_or(TxAbortError::NoStateSnapshot { sid })?;
        let reservation = self
            .state_snapshots
            .reserve_finalization(lane, generation, SnapshotFinalizeTarget::Restore)
            .ok_or(TxAbortError::AlreadyFinalized { sid })?;
        if reservation.cap_revision() != cap_revision {
            crate::invariant();
        }
        Ok(PreparedTxAbortEffect { sid, reservation })
    }

    #[inline]
    pub(crate) fn publish_prepared_tx_abort_effect(&self, proof: PreparedTxAbortEffect) {
        let sid = proof.sid();
        let finalization = self
            .state_snapshots
            .publish_finalization_reserved(proof.into_reservation());
        let lane = finalization.lane();
        let generation = finalization.generation();
        let cap_revision = finalization.cap_revision();
        self.r#gen.publish_prepared(lane, generation);
        self.restore_lane_runtime_state(lane, cap_revision);
        self.emit_effect(ControlOp::TxAbort, sid, lane, generation.0 as u32);
    }

    #[inline]
    pub(crate) fn rollback_prepared_tx_abort_effect(&self, proof: PreparedTxAbortEffect) {
        self.state_snapshots
            .rollback_finalization_reserved(proof.into_reservation());
    }
}
