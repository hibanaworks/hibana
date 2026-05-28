use super::{
    Clock, ControlOp, GenError, Generation, GenerationRecord, IncreasingGen, LabelUniverse, Lane,
    LocalTopologyInvariant, NoopTap, One, PendingTopology, PreparedStateRestoreEffect, Rendezvous,
    RendezvousId, SessionId, SnapshotFinalization, SnapshotFinalizeTarget, StateRestoreError,
    TopologyAck, TopologyError, TopologyIntent, TopologyLeaseState, Transport, Txn,
};
impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    pub(crate) fn process_topology_intent(
        &self,
        intent: &TopologyIntent,
    ) -> Result<TopologyAck, TopologyError> {
        let dst_rv: RendezvousId = intent.dst_rv;
        let dst_lane: Lane = intent.dst_lane;
        let new_gen: Generation = intent.new_gen;

        // Validate this RV is the intended destination
        if dst_rv != self.id {
            return Err(TopologyError::RendezvousIdMismatch {
                expected: dst_rv,
                got: self.id,
            });
        }

        // Validate lane is in range
        if !self.lane_range.contains(&dst_lane.raw()) {
            return Err(TopologyError::LaneOutOfRange { lane: dst_lane });
        }

        // Check lane is available
        if self.assoc.is_active(dst_lane) {
            return Err(TopologyError::LaneMismatch {
                expected: dst_lane,
                provided: dst_lane,
            });
        }

        // Validate destination-lane generation monotonicity.
        let last_gen = self.r#gen.last(dst_lane).unwrap_or(Generation(0));
        self.validate_topology_generation(dst_lane, new_gen)?;

        // Begin local topology transition using typestate transaction (ack immediately for local state).
        let txn: Txn<LocalTopologyInvariant, IncreasingGen, One> =
            /* SAFETY: the topology owner has validated the lane/generation transition before minting this typestate transaction witness. */ unsafe { Txn::new(dst_lane, last_gen) };
        let mut tap = NoopTap;
        let in_begin = txn.begin(&mut tap);
        let in_acked = in_begin.ack(&mut tap);

        let pending = PendingTopology::destination_prepare(
            SessionId(intent.sid),
            dst_lane,
            self.r#gen.last(dst_lane),
            new_gen,
            in_acked,
            Some((intent.seq_tx, intent.seq_rx)),
        );
        let begin_result = self.topology.begin(dst_lane, pending);
        begin_result?;

        let ack = TopologyAck {
            src_rv: intent.src_rv,
            dst_rv: self.id,
            sid: intent.sid,
            new_gen,
            src_lane: intent.src_lane,
            new_lane: dst_lane,
            seq_tx: intent.seq_tx,
            seq_rx: intent.seq_rx,
        };

        Ok(ack)
    }

    #[cfg(test)]
    pub(crate) fn acknowledge_topology_intent(
        &self,
        intent: &TopologyIntent,
    ) -> Result<TopologyAck, TopologyError> {
        let ack = self.process_topology_intent(intent)?;
        self.emit_topology_ack(
            SessionId::new(intent.sid),
            intent.src_lane,
            Lane::new(intent.dst_lane.raw()),
            ack.new_gen,
        );
        Ok(ack)
    }

    pub(crate) fn restore_topology_generation(
        &self,
        lane: Lane,
        previous_generation: Option<Generation>,
    ) -> Result<(), TopologyError> {
        self.r#gen.reset_lane(lane);
        let Some(previous) = previous_generation else {
            return Ok(());
        };
        self.r#gen
            .check_and_update(lane, Generation::ZERO)
            .map_err(|err| match err {
                GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                    TopologyError::StaleGeneration { lane, last, new }
                }
                GenError::Overflow { lane, last } => {
                    TopologyError::GenerationOverflow { lane, last }
                }
                GenError::InvalidInitial { lane, new } => {
                    TopologyError::InvalidInitial { lane, new }
                }
            })?;
        if previous != Generation::ZERO {
            self.r#gen
                .restore_to(lane, previous)
                .map_err(|err| match err {
                    GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                        TopologyError::StaleGeneration { lane, last, new }
                    }
                    GenError::Overflow { lane, last } => {
                        TopologyError::GenerationOverflow { lane, last }
                    }
                    GenError::InvalidInitial { lane, new } => {
                        TopologyError::InvalidInitial { lane, new }
                    }
                })?;
        }
        Ok(())
    }

    pub(crate) fn abort_topology_state(&self, sid: SessionId) -> Result<bool, TopologyError> {
        let Some(pending) = self.topology.take_pending_for_sid(sid) else {
            return Ok(false);
        };
        let parts = pending.into_parts();
        let _ = (
            parts.sid,
            parts.target,
            parts.state,
            parts.fences,
            parts.expected_ack,
        );
        self.topology.reset_lane(parts.lane);
        if !matches!(parts.lease_state, TopologyLeaseState::DestinationPrepared) {
            self.restore_topology_generation(parts.lane, parts.previous_generation)?;
        }
        Ok(true)
    }

    #[inline]
    pub(crate) fn prepare_state_restore_effect(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<PreparedStateRestoreEffect, StateRestoreError> {
        self.ensure_associated_session_lane(sid, lane)
            .map_err(|_| StateRestoreError::UnknownSession { sid })?;
        let current = self.r#gen.last(lane).unwrap_or(Generation(0));
        let snapshot = self
            .state_snapshots
            .last_snapshot(lane)
            .ok_or(StateRestoreError::NoStateSnapshot { sid })?;
        if !matches!(
            self.state_snapshots.finalization(lane),
            None | Some(SnapshotFinalization::Available)
        ) {
            return Err(StateRestoreError::AlreadyFinalized { sid });
        }
        if generation != snapshot {
            return Err(StateRestoreError::StaleStateSnapshot {
                sid,
                requested: generation,
                current: snapshot,
            });
        }
        if current.raw() < generation.raw() {
            return Err(StateRestoreError::EpochMismatch {
                expected: current,
                got: generation,
            });
        }
        let cap_revision = self
            .state_snapshots
            .last_cap_revision(lane)
            .ok_or(StateRestoreError::NoStateSnapshot { sid })?;
        let reservation = self
            .state_snapshots
            .reserve_finalization(lane, generation, SnapshotFinalizeTarget::Restore)
            .ok_or(StateRestoreError::AlreadyFinalized { sid })?;
        debug_assert_eq!(reservation.cap_revision(), cap_revision);
        Ok(PreparedStateRestoreEffect { sid, reservation })
    }

    #[inline]
    pub(crate) fn publish_prepared_state_restore_effect(&self, proof: PreparedStateRestoreEffect) {
        let sid = proof.sid();
        let finalization = self
            .state_snapshots
            .publish_finalization_reserved(proof.into_reservation());
        let lane = finalization.lane();
        let generation = finalization.generation();
        let cap_revision = finalization.cap_revision();
        self.r#gen.publish_prepared(lane, generation);
        self.restore_lane_runtime_state(lane, cap_revision);
        self.emit_effect(ControlOp::StateRestore, sid, lane, generation.0 as u32);
        super::emit(
            self.tap(),
            super::StateRestoreOk::new(self.clock.now32(), sid.raw(), generation.0 as u32),
        );
    }

    #[inline]
    pub(crate) fn rollback_prepared_state_restore_effect(&self, proof: PreparedStateRestoreEffect) {
        self.state_snapshots
            .rollback_finalization_reserved(proof.into_reservation());
    }

    pub(crate) fn validate_topology_generation(
        &self,
        lane: Lane,
        new_gen: Generation,
    ) -> Result<(), TopologyError> {
        match self.r#gen.last(lane) {
            None => {
                if new_gen.0 >= 1 {
                    Ok(())
                } else {
                    Err(TopologyError::InvalidInitial { lane, new: new_gen })
                }
            }
            Some(prev) if prev.0 == u16::MAX => {
                Err(TopologyError::GenerationOverflow { lane, last: prev })
            }
            Some(prev) if new_gen.0 > prev.0 => Ok(()),
            Some(prev) => Err(TopologyError::StaleGeneration {
                lane,
                last: prev,
                new: new_gen,
            }),
        }
    }
}
