use super::*;

// ============================================================================
// Local topology operations (used by EffectRunner)
// ============================================================================

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    /// Begin a local topology operation for the cluster-owned topology automaton.
    pub(crate) fn topology_begin(
        &self,
        sid: SessionId,
        lane: Lane,
        fences: Option<(u32, u32)>,
        generation: Generation,
        expected_ack: Option<TopologyAck>,
    ) -> Result<(), TopologyError> {
        let ctx = EffectContext::new(sid, lane)
            .with_generation(generation)
            .with_fences(fences)
            .with_expected_topology_ack(expected_ack);

        match self.eval_effect(ControlOp::TopologyBegin, ctx) {
            Ok(_) => Ok(()),
            Err(EffectError::Topology(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::TxCommit(_))
            | Err(EffectError::TxAbort(_))
            | Err(EffectError::Delegation(_))
            | Err(EffectError::StateRestore(_)) => {
                unreachable!("topology begin effect failure is fully covered")
            }
        }
    }

    pub(crate) fn topology_begin_from_intent(
        &self,
        intent: TopologyIntent,
    ) -> Result<(), TopologyError> {
        if self.id != intent.src_rv {
            return Err(TopologyError::RendezvousIdMismatch {
                expected: intent.src_rv,
                got: self.id,
            });
        }

        let sid = SessionId(intent.sid);
        let lane = intent.src_lane;
        self.ensure_associated_session_lane(sid, lane)?;
        let current = self.r#gen.last(lane).unwrap_or(Generation::ZERO);
        if current != intent.old_gen {
            return Err(TopologyError::StaleGeneration {
                lane,
                last: current,
                new: intent.new_gen,
            });
        }

        let fences =
            (intent.seq_tx != 0 || intent.seq_rx != 0).then_some((intent.seq_tx, intent.seq_rx));
        self.topology_begin(
            sid,
            lane,
            fences,
            intent.new_gen,
            Some(TopologyAck::from_intent(&intent)),
        )
    }

    pub(crate) fn validate_topology_commit_operands(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<Lane, TopologyError> {
        let expected = self.expected_topology_ack(sid)?;
        let got = operands.ack(sid);
        if got != expected {
            return Err(classify_topology_ack_mismatch(expected, got));
        }
        Ok(expected.src_lane)
    }

    pub(crate) fn preflight_destination_topology_commit(
        &self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), TopologyError> {
        if self.assoc.is_active(lane) {
            return Err(TopologyError::InProgress { lane });
        }
        self.topology.preflight_commit(lane, sid)
    }

    pub(crate) fn finalize_destination_topology_commit(
        &mut self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), TopologyError> {
        self.preflight_destination_topology_commit(sid, lane)?;
        let (previous_generation, target) =
            self.topology.prepared_destination_generation(lane, sid)?;
        self.commit_prepared_destination_generation(lane, previous_generation, target)?;
        if let Err(err) = self.topology.finalize_destination(lane, sid) {
            self.restore_topology_generation(lane, previous_generation)?;
            return Err(err);
        }
        Ok(())
    }

    fn revoke_public_endpoints_for_session(&mut self, sid: SessionId) {
        let this = self as *mut Self;
        let mut released_lanes = [Lane::new(0); u8::MAX as usize + 1];
        let lease_capacity = /* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */ unsafe { usize::from((*this).endpoint_lease_capacity()) };
        let mut idx = 0usize;
        while idx < lease_capacity {
            let Some((slot, generation)) = (/* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */unsafe { (*this).public_endpoint_lease_by_index(idx) })
            else {
                idx += 1;
                continue;
            };
            let Some((offset, len)) = (/* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */unsafe { (*this).endpoint_lease_storage(slot, generation) })
            else {
                idx += 1;
                continue;
            };
            let (slab_ptr, slab_len) = /* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */ unsafe { (*this).slab_ptr_and_len() };
            idx += 1;
            if len == 0 || offset + len > slab_len {
                continue;
            }

            let Some(header) = core::ptr::NonNull::new(
                /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                unsafe {
                    slab_ptr
                        .add(offset)
                        .cast::<crate::endpoint::carrier::KernelEndpointHeader<'cfg>>()
                },
            ) else {
                continue;
            };
            let ops = /* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */ unsafe { header.as_ref().ops() };
            let released = /* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */ unsafe {
                (ops.revoke_for_session)(
                    header.cast(),
                    sid,
                    released_lanes.as_mut_ptr(),
                    released_lanes.len(),
                )
            };
            if released != 0 {
                /* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */
                unsafe {
                    (*this).release_endpoint_lease(slot, generation);
                }
                let mut released_idx = 0usize;
                while released_idx < released {
                    let owned_lane = released_lanes[released_idx];
                    if let Some(released_sid) =
                        /* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */
                        unsafe { (*this).release_lane(owned_lane) }
                    {
                        /* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */
                        unsafe {
                            (*this).emit_lane_release(released_sid, owned_lane);
                        }
                    }
                    released_idx += 1;
                }
            }
        }
    }

    fn retire_session_lane(&self, sid: SessionId, lane: Lane) {
        while self.assoc.get_sid(lane) == Some(sid) {
            if let Some(released_sid) = self.release_lane(lane) {
                self.emit_lane_release(released_sid, lane);
                break;
            }
        }
    }

    fn retire_session_lanes(&self, sid: SessionId) {
        while let Some(lane) = self.assoc.find_lane(sid) {
            self.retire_session_lane(sid, lane);
        }
    }

    /// Commit a local topology operation after cluster-owned source/destination preflight.
    pub(crate) fn topology_commit(
        &mut self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<(), TopologyError> {
        let ctx = EffectContext::new(sid, lane);
        match self.eval_effect(ControlOp::TopologyCommit, ctx) {
            Ok(_) => {
                self.revoke_public_endpoints_for_session(sid);
                self.retire_session_lanes(sid);
                Ok(())
            }
            Err(EffectError::Topology(err)) => Err(err),
            Err(EffectError::MissingGeneration)
            | Err(EffectError::Unsupported)
            | Err(EffectError::Delegation(_))
            | Err(EffectError::StateRestore(_))
            | Err(EffectError::TxAbort(_))
            | Err(EffectError::TxCommit(_)) => {
                unreachable!("topology commit failure is fully covered")
            }
        }
    }

    /// Drain transport telemetry and emit tap events for downstream observers.
    pub(crate) fn flush_transport_events(&self) -> Option<crate::transport::TransportEvent> {
        let tap = self.tap();
        let clock = &self.clock;
        let mut last_loss = None;
        let mut emit_event = |event: crate::transport::TransportEvent| {
            let (arg0, arg1) = event.encode_tap_args();
            if matches!(event.kind(), TransportEventKind::Loss) {
                last_loss = Some(event);
            }
            emit(
                tap,
                crate::observe::events::TransportEvent::new(clock.now32(), arg0, arg1),
            );
        };
        self.transport.drain_events(&mut emit_event);
        let metrics_attrs = self.transport.metrics().attrs();
        let snapshot = crate::transport::TransportSnapshot::from_policy_attrs(&metrics_attrs);
        if let Some(payload) = snapshot.encode_tap_metrics() {
            let (arg0, arg1) = payload.primary();
            emit(
                tap,
                crate::observe::events::TransportMetrics::new(clock.now32(), arg0, arg1),
            );
            if let Some((ext0, ext1)) = payload.extension() {
                emit(
                    tap,
                    crate::observe::events::TransportMetricsExt::new(clock.now32(), ext0, ext1),
                );
            }
        }
        last_loss
    }
}
