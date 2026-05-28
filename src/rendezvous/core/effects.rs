use super::{
    Clock, ControlOp, CpError, EffIndex, EffectContext, EffectError, EffectResult, GenError,
    Generation, GenerationRecord, IncreasingGen, LabelUniverse, Lane, LocalTopologyInvariant,
    NoopTap, One, PendingTopology, PolicyMode, RawEvent, Rendezvous, ResourceScope, SessionId,
    SnapshotFinalization, StateRestoreError, StateRestoreOk, TapEvent, TopologyError, Transport,
    TxAbortError, TxCommitError, Txn, control_op_tap_event_id, emit,
};

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    pub(crate) fn register_policy(
        &mut self,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        policy: PolicyMode,
    ) -> Result<(), CpError> {
        if policy.is_dynamic() && self.ensure_policy_table_storage().is_none() {
            return Err(CpError::resource_exhausted(ResourceScope::PolicyTable));
        }
        self.policies
            .register(lane, eff_index, tag, policy)
            .map_err(|_| CpError::resource_exhausted(ResourceScope::PolicyTable))
    }

    pub(crate) fn policy(&self, lane: Lane, eff_index: EffIndex, tag: u8) -> Option<PolicyMode> {
        self.policies.get(lane, eff_index, tag)
    }

    pub(crate) fn reset_policy(&self, lane: Lane) {
        self.policies.reset_lane(lane);
    }

    pub(in crate::rendezvous::core) fn emit_effect(
        &self,
        effect: ControlOp,
        sid: SessionId,
        lane: Lane,
        arg: u32,
    ) {
        let event_id = control_op_tap_event_id(effect);
        let causal = TapEvent::make_causal_key(lane.as_wire(), 1);
        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), event_id)
                .with_causal_key(causal)
                .with_arg0(sid.raw())
                .with_arg1(arg),
        );
    }

    pub(crate) fn emit_topology_ack(
        &self,
        sid: SessionId,
        from_lane: Lane,
        to_lane: Lane,
        generation: Generation,
    ) {
        let packed = ((from_lane.as_wire() as u32) & 0xFF)
            | (((to_lane.as_wire() as u32) & 0xFF) << 8)
            | ((generation.0 as u32) << 16);
        emit(
            self.tap(),
            RawEvent::new(self.clock.now32(), crate::observe::ids::TOPOLOGY_ACK)
                .with_arg0(packed)
                .with_arg1(sid.raw()),
        );
    }

    pub(in crate::rendezvous::core) fn eval_effect(
        &self,
        effect: ControlOp,
        ctx: EffectContext,
    ) -> Result<EffectResult, EffectError> {
        match effect {
            ControlOp::TopologyBegin => {
                self.ensure_associated_session_lane(ctx.sid, ctx.lane)
                    .map_err(EffectError::Topology)?;
                let target = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let mut prev = self.r#gen.last(ctx.lane);
                if prev.is_none() {
                    let _ = self.r#gen.check_and_update(ctx.lane, Generation(0));
                    prev = Some(Generation(0));
                }
                let prev = prev.unwrap_or(Generation(0));

                self.validate_topology_generation(ctx.lane, target)
                    .map_err(EffectError::Topology)?;

                let txn: Txn<LocalTopologyInvariant, IncreasingGen, One> =
                    /* SAFETY: the topology owner has validated the lane/generation transition before minting this typestate transaction witness. */ unsafe { Txn::new(ctx.lane, prev) };
                let mut tap = NoopTap;
                let in_begin = txn.begin(&mut tap);
                let in_acked = in_begin.ack(&mut tap);

                let expected_ack = ctx.expected_topology_ack.ok_or(EffectError::Topology(
                    TopologyError::NoPending { lane: ctx.lane },
                ))?;
                let pending = PendingTopology::source_prepare(
                    ctx.sid,
                    ctx.lane,
                    Some(prev),
                    target,
                    in_acked,
                    ctx.fences,
                    expected_ack,
                );

                self.topology
                    .begin(ctx.lane, pending)
                    .map_err(EffectError::Topology)?;

                let packed = ((ctx.lane.as_wire() as u32) & 0xFF) | ((target.0 as u32) << 16);
                self.emit_effect(effect, ctx.sid, ctx.lane, packed);
                Ok(EffectResult::Generation)
            }
            ControlOp::TopologyAck => Ok(EffectResult::None),
            ControlOp::TopologyCommit => {
                let pending = self.topology.take(ctx.lane).ok_or(EffectError::Topology(
                    TopologyError::NoPending { lane: ctx.lane },
                ))?;

                let parts = pending.into_parts();

                if parts.sid != ctx.sid {
                    // Reinsert to preserve state before returning error.
                    let _ = self.topology.begin(
                        parts.lane,
                        PendingTopology::source_prepare(
                            parts.sid,
                            parts.lane,
                            parts.previous_generation,
                            parts.target,
                            parts
                                .state
                                .expect("topology commit reinsert requires a pending transaction"),
                            parts.fences,
                            parts
                                .expected_ack
                                .expect("source topology reinsert requires an expected ack"),
                        ),
                    );
                    return Err(EffectError::Topology(TopologyError::UnknownSession {
                        sid: ctx.sid,
                    }));
                }

                self.validate_topology_generation(ctx.lane, parts.target)
                    .map_err(EffectError::Topology)?;

                if let Err(err) = self.r#gen.check_and_update(ctx.lane, parts.target) {
                    let _ = self.topology.begin(
                        parts.lane,
                        PendingTopology::source_prepare(
                            parts.sid,
                            parts.lane,
                            parts.previous_generation,
                            parts.target,
                            parts
                                .state
                                .expect("topology commit reinsert requires a pending transaction"),
                            parts.fences,
                            parts
                                .expected_ack
                                .expect("source topology reinsert requires an expected ack"),
                        ),
                    );
                    let topology_err = match err {
                        GenError::StaleOrDuplicate(GenerationRecord { lane, last, new }) => {
                            TopologyError::StaleGeneration { lane, last, new }
                        }
                        GenError::Overflow { lane, last } => {
                            TopologyError::GenerationOverflow { lane, last }
                        }
                        GenError::InvalidInitial { lane, new } => {
                            TopologyError::InvalidInitial { lane, new }
                        }
                    };
                    return Err(EffectError::Topology(topology_err));
                }
                let _ = (parts.lease_state, parts.fences, parts.expected_ack);

                let mut tap = NoopTap;
                parts
                    .state
                    .expect("topology commit requires a pending transaction")
                    .commit(&mut tap);

                let packed = ((ctx.lane.as_wire() as u32) & 0xFF) | ((parts.target.0 as u32) << 16);
                self.emit_effect(effect, ctx.sid, ctx.lane, packed);
                Ok(EffectResult::Generation)
            }
            ControlOp::TxCommit => {
                let generation = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let snapshot =
                    self.state_snapshots
                        .last_snapshot(ctx.lane)
                        .ok_or(EffectError::TxCommit(TxCommitError::NoStateSnapshot {
                            sid: ctx.sid,
                        }))?;

                if !matches!(
                    self.state_snapshots.finalization(ctx.lane),
                    None | Some(SnapshotFinalization::Available)
                ) {
                    return Err(EffectError::TxCommit(TxCommitError::AlreadyFinalized {
                        sid: ctx.sid,
                    }));
                }

                if snapshot != generation {
                    return Err(EffectError::TxCommit(TxCommitError::GenerationMismatch {
                        sid: ctx.sid,
                        expected: snapshot,
                        got: generation,
                    }));
                }

                self.state_snapshots.mark_committed(ctx.lane);
                self.caps.discard_released_lane_entries(ctx.lane);
                self.emit_effect(effect, ctx.sid, ctx.lane, generation.0 as u32);
                Ok(EffectResult::Generation)
            }
            ControlOp::AbortBegin => {
                self.emit_effect(effect, ctx.sid, ctx.lane, ctx.lane.as_wire() as u32);
                Ok(EffectResult::None)
            }
            ControlOp::AbortAck => {
                let generation = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                self.emit_effect(effect, ctx.sid, ctx.lane, generation.0 as u32);
                Ok(EffectResult::None)
            }
            ControlOp::StateSnapshot => {
                let epoch = self.r#gen.last(ctx.lane).unwrap_or(Generation(0));
                self.caps.discard_released_lane_entries(ctx.lane);
                self.state_snapshots
                    .record_snapshot(ctx.lane, epoch, self.cap_revision.get());
                self.emit_effect(effect, ctx.sid, ctx.lane, epoch.0 as u32);
                Ok(EffectResult::Generation)
            }
            ControlOp::StateRestore => {
                let requested = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let current = self.r#gen.last(ctx.lane).unwrap_or(Generation(0));
                let snapshot = self.state_snapshots.last_snapshot(ctx.lane).ok_or({
                    EffectError::StateRestore(StateRestoreError::NoStateSnapshot { sid: ctx.sid })
                })?;

                if !matches!(
                    self.state_snapshots.finalization(ctx.lane),
                    None | Some(SnapshotFinalization::Available)
                ) {
                    return Err(EffectError::StateRestore(
                        StateRestoreError::AlreadyFinalized { sid: ctx.sid },
                    ));
                }

                if requested != snapshot {
                    return Err(EffectError::StateRestore(
                        StateRestoreError::StaleStateSnapshot {
                            sid: ctx.sid,
                            requested,
                            current: snapshot,
                        },
                    ));
                }

                if current.raw() < requested.raw() {
                    return Err(EffectError::StateRestore(
                        StateRestoreError::EpochMismatch {
                            expected: current,
                            got: requested,
                        },
                    ));
                }

                let snapshot_cap_revision =
                    self.state_snapshots.last_cap_revision(ctx.lane).ok_or({
                        EffectError::StateRestore(StateRestoreError::NoStateSnapshot {
                            sid: ctx.sid,
                        })
                    })?;

                self.r#gen.restore_to(ctx.lane, requested).map_err(|_| {
                    EffectError::StateRestore(StateRestoreError::EpochMismatch {
                        expected: current,
                        got: requested,
                    })
                })?;
                self.restore_lane_runtime_state(ctx.lane, snapshot_cap_revision);
                self.state_snapshots.mark_restored(ctx.lane);

                self.emit_effect(effect, ctx.sid, ctx.lane, requested.0 as u32);
                emit(
                    self.tap(),
                    StateRestoreOk::new(self.clock.now32(), ctx.sid.raw(), requested.0 as u32),
                );

                Ok(EffectResult::Generation)
            }
            ControlOp::TxAbort => {
                let requested = ctx.generation.ok_or(EffectError::MissingGeneration)?;
                let current = self.r#gen.last(ctx.lane).unwrap_or(Generation(0));
                let snapshot = self.state_snapshots.last_snapshot(ctx.lane).ok_or({
                    EffectError::TxAbort(TxAbortError::NoStateSnapshot { sid: ctx.sid })
                })?;

                if !matches!(
                    self.state_snapshots.finalization(ctx.lane),
                    None | Some(SnapshotFinalization::Available)
                ) {
                    return Err(EffectError::TxAbort(TxAbortError::AlreadyFinalized {
                        sid: ctx.sid,
                    }));
                }

                if requested != snapshot {
                    return Err(EffectError::TxAbort(TxAbortError::StaleStateSnapshot {
                        sid: ctx.sid,
                        requested,
                        current: snapshot,
                    }));
                }

                if current.raw() < requested.raw() {
                    return Err(EffectError::TxAbort(TxAbortError::GenerationMismatch {
                        sid: ctx.sid,
                        expected: current,
                        got: requested,
                    }));
                }

                let snapshot_cap_revision =
                    self.state_snapshots.last_cap_revision(ctx.lane).ok_or({
                        EffectError::TxAbort(TxAbortError::NoStateSnapshot { sid: ctx.sid })
                    })?;

                self.r#gen.restore_to(ctx.lane, requested).map_err(|_| {
                    EffectError::TxAbort(TxAbortError::GenerationMismatch {
                        sid: ctx.sid,
                        expected: current,
                        got: requested,
                    })
                })?;
                self.restore_lane_runtime_state(ctx.lane, snapshot_cap_revision);
                self.state_snapshots.mark_restored(ctx.lane);

                self.emit_effect(effect, ctx.sid, ctx.lane, requested.0 as u32);
                Ok(EffectResult::Generation)
            }
            _ => Err(EffectError::Unsupported),
        }
    }
}
