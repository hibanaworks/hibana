use super::*;

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

    pub(crate) fn commit_prepared_destination_generation(
        &self,
        lane: Lane,
        previous_generation: Option<Generation>,
        target: Generation,
    ) -> Result<(), TopologyError> {
        let current = self.r#gen.last(lane);
        if current != previous_generation {
            return Err(match current {
                Some(last) => TopologyError::StaleGeneration {
                    lane,
                    last,
                    new: target,
                },
                None => TopologyError::StaleGeneration {
                    lane,
                    last: Generation::ZERO,
                    new: target,
                },
            });
        }
        if current.is_none() {
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
        }
        self.r#gen
            .check_and_update(lane, target)
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
            })
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

    pub(crate) fn state_restore_at_lane(
        &self,
        sid: SessionId,
        lane: Lane,
        epoch: Generation,
    ) -> Result<(), StateRestoreError> {
        match self.eval_effect(
            ControlOp::StateRestore,
            EffectContext::new(sid, lane).with_generation(epoch),
        ) {
            Ok(_) => Ok(()),
            Err(EffectError::StateRestore(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Topology(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::TxAbort(_))
            | Err(EffectError::TxCommit(_)) => {
                unreachable!("state restore effect failure is fully covered")
            }
        }
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
