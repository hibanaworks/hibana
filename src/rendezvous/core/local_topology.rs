use super::{
    Clock, ControlOp, Generation, IncreasingGen, LabelUniverse, Lane, LocalTopologyInvariant,
    NoopTap, One, PendingTopology, RawEvent, Rendezvous, SessionId, TopologyAck, TopologyError,
    TopologyIntent, TopologyOperands, Transport, Txn, classify_topology_ack_mismatch, emit,
};

mod prepared_commit;

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    /// Begin a local topology operation for the cluster-owned topology automaton.
    #[cfg(test)]
    pub(crate) fn topology_begin(
        &self,
        sid: SessionId,
        lane: Lane,
        fences: Option<(u32, u32)>,
        generation: Generation,
        expected_ack: Option<TopologyAck>,
    ) -> Result<(), TopologyError> {
        self.ensure_associated_session_lane(sid, lane)?;
        let mut previous = self.r#gen.last(lane);
        if previous.is_none() {
            let _ = self.r#gen.check_and_update(lane, Generation::ZERO);
            previous = Some(Generation::ZERO);
        }
        let previous = previous.unwrap_or(Generation::ZERO);
        self.validate_topology_generation(lane, generation)?;
        let expected_ack = expected_ack.ok_or(TopologyError::NoPending { lane })?;

        let txn: Txn<LocalTopologyInvariant, IncreasingGen, One> =
            /* SAFETY: the topology owner has validated the lane/generation transition before minting this typestate transaction witness. */ unsafe { Txn::new(lane, previous) };
        let mut tap = NoopTap;
        let in_begin = txn.begin(&mut tap);
        let in_acked = in_begin.ack(&mut tap);
        let pending = PendingTopology::source_prepare(
            sid,
            lane,
            Some(previous),
            generation,
            in_acked,
            fences,
            expected_ack,
        );
        self.topology.begin(lane, pending)?;
        self.publish_prepared_topology_begin(sid, lane, generation);
        Ok(())
    }

    #[cfg(test)]
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

    pub(crate) fn prepare_topology_begin_from_intent(
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
        let mut previous = self.r#gen.last(lane);
        if previous.is_none() {
            let _ = self.r#gen.check_and_update(lane, Generation::ZERO);
            previous = Some(Generation::ZERO);
        }
        let previous = previous.unwrap_or(Generation::ZERO);
        if previous != intent.old_gen {
            return Err(TopologyError::StaleGeneration {
                lane,
                last: previous,
                new: intent.new_gen,
            });
        }
        self.validate_topology_generation(lane, intent.new_gen)?;

        let txn: Txn<LocalTopologyInvariant, IncreasingGen, One> =
            /* SAFETY: the topology owner has validated the lane/generation transition before minting this typestate transaction witness. */ unsafe { Txn::new(lane, previous) };
        let mut tap = NoopTap;
        let in_begin = txn.begin(&mut tap);
        let in_acked = in_begin.ack(&mut tap);
        let fences =
            (intent.seq_tx != 0 || intent.seq_rx != 0).then_some((intent.seq_tx, intent.seq_rx));
        let pending = PendingTopology::source_prepare(
            sid,
            lane,
            Some(previous),
            intent.new_gen,
            in_acked,
            fences,
            TopologyAck::from_intent(&intent),
        );
        self.topology.begin(lane, pending)
    }

    pub(crate) fn publish_prepared_topology_begin(
        &self,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) {
        let packed = ((lane.as_wire() as u32) & 0xFF) | ((generation.0 as u32) << 16);
        let causal = crate::observe::core::TapEvent::make_causal_key(lane.as_wire(), 1);
        emit(
            self.tap(),
            RawEvent::new(
                self.clock.now32(),
                crate::control::cluster::effects::control_op_tap_event_id(ControlOp::TopologyBegin),
            )
            .with_causal_key(causal)
            .with_arg0(sid.raw())
            .with_arg1(packed),
        );
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

    pub(crate) unsafe fn revoke_public_endpoints_for_session_raw(
        this: *mut Self,
        sid: SessionId,
        mut rollback_terminal: impl FnMut(crate::endpoint::kernel::SendDescriptorTerminal<'cfg>),
    ) {
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
            let mut descriptor_terminal = None;
            let mut waiter_lane = None;
            let released = /* SAFETY: topology state owns the pending transition slot and reaches this raw access through its exclusive transition path. */ unsafe {
                (ops.revoke_for_session)(
                    header.cast(),
                    sid,
                    released_lanes.as_mut_ptr(),
                    released_lanes.len(),
                    core::ptr::from_mut(&mut descriptor_terminal).cast(),
                    core::ptr::from_mut(&mut waiter_lane).cast(),
                )
            };
            if let Some(lane) = waiter_lane {
                /* SAFETY: revocation runs through the source rendezvous transition owner, so waiter cleanup uses the active owner directly and never re-enters the cluster mutation API. */
                unsafe {
                    (*this).clear_session_waiter(sid, lane);
                }
            }
            if let Some(terminal) = descriptor_terminal {
                rollback_terminal(terminal);
            }
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

    pub(crate) unsafe fn retire_session_lanes_raw(this: *const Self, sid: SessionId) {
        while let Some(lane) =
            /* SAFETY: caller owns the terminal topology transition for this rendezvous while retiring lanes. */
            unsafe { (*this).assoc.find_lane(sid) }
        {
            /* SAFETY: caller owns the terminal topology transition for this rendezvous while retiring lanes. */
            unsafe {
                (*this).retire_session_lane(sid, lane);
            }
        }
    }
}
