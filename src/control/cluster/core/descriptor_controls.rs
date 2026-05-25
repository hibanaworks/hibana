use super::*;

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(crate) fn prepare_topology_operands_from_descriptor(
        &self,
        rv_id: RendezvousId,
        src_lane: Lane,
        desc: ControlDesc,
        descriptor: TopologyDescriptor,
    ) -> Result<TopologyOperands, CpError> {
        if !matches!(desc.op(), ControlOp::TopologyBegin)
            || !matches!(desc.scope_kind(), ControlScopeKind::Topology)
        {
            return Err(CpError::Authorisation {
                operation: ControlOp::TopologyBegin as u8,
            });
        }
        self.validate_topology_begin_operands(rv_id, src_lane, descriptor.operands(), None)
    }

    pub(crate) fn validate_topology_operands_from_descriptor(
        &self,
        rv_id: RendezvousId,
        src_lane: Lane,
        desc: ControlDesc,
        descriptor: TopologyDescriptor,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        let expected = match desc.op() {
            ControlOp::TopologyAck => {
                self.validate_topology_ack_operands(rv_id, src_lane, descriptor.operands(), None)?
            }
            ControlOp::TopologyCommit => self.validate_topology_commit_operands(
                rv_id,
                src_lane,
                descriptor.operands(),
                None,
            )?,
            _ => {
                return Err(CpError::Authorisation {
                    operation: desc.op() as u8,
                });
            }
        };
        if expected != operands {
            return Err(CpError::Authorisation {
                operation: desc.op() as u8,
            });
        }
        Ok(())
    }

    pub(crate) fn prepare_reroute_handle_from_policy(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        op: ControlOp,
        policy: PolicyMode,
        input: [u32; 4],
        attrs: &crate::transport::context::PolicyAttrs,
    ) -> Result<DelegationHandle, CpError> {
        let _ = (eff_index, tag, op, attrs);
        match policy {
            PolicyMode::Static => delegation_handle_from_route_input(rv_id, lane, input),
            PolicyMode::Dynamic { .. } => {
                Err(CpError::UnsupportedEffect(ControlOp::CapDelegate as u8))
            }
        }
    }

    pub(crate) fn take_cached_topology_operands(&self, sid: SessionId) -> Option<TopologyOperands> {
        self.with_control_mut(|core| core.cached_operands_remove(sid))
    }

    pub(crate) fn validate_topology_begin_operands(
        &self,
        rv_id: RendezvousId,
        cp_lane: Lane,
        operands: TopologyOperands,
        generation: Option<Generation>,
    ) -> Result<TopologyOperands, CpError> {
        validate_topology_rendezvous_pair(
            operands.src_rv,
            operands.dst_rv,
            ControlOp::TopologyBegin,
        )?;

        if cp_lane != operands.src_lane {
            return Err(CpError::Authorisation {
                operation: ControlOp::TopologyBegin as u8,
            });
        }

        if let Some(header_gen) = generation
            && header_gen != operands.new_gen
        {
            return Err(CpError::Topology(TopologyError::GenerationMismatch));
        }

        if rv_id != operands.src_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: rv_id.raw(),
            });
        }
        Ok(operands)
    }

    #[cfg(test)]
    pub(crate) fn dispatch_topology_ack_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: TopologyHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        let descriptor = TopologyDescriptor::decode_for(ControlOp::TopologyAck, handle.encode())?;
        let operands =
            self.validate_topology_ack_operands(rv_id, cp_lane, descriptor.operands(), generation)?;
        self.run_effect(operands.dst_rv, CpCommand::topology_ack(cp_sid, operands))
    }

    pub(crate) fn validate_topology_ack_operands(
        &self,
        rv_id: RendezvousId,
        cp_lane: Lane,
        operands: TopologyOperands,
        generation: Option<Generation>,
    ) -> Result<TopologyOperands, CpError> {
        validate_topology_rendezvous_pair(
            operands.src_rv,
            operands.dst_rv,
            ControlOp::TopologyAck,
        )?;

        if cp_lane != operands.dst_lane {
            return Err(CpError::Authorisation {
                operation: ControlOp::TopologyAck as u8,
            });
        }

        if let Some(header_gen) = generation
            && header_gen != operands.new_gen
        {
            return Err(CpError::Topology(TopologyError::GenerationMismatch));
        }

        if rv_id != operands.dst_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.dst_rv.raw(),
                actual: rv_id.raw(),
            });
        }
        Ok(operands)
    }

    pub(crate) fn validate_topology_commit_operands(
        &self,
        rv_id: RendezvousId,
        cp_lane: Lane,
        operands: TopologyOperands,
        generation: Option<Generation>,
    ) -> Result<TopologyOperands, CpError> {
        validate_topology_rendezvous_pair(
            operands.src_rv,
            operands.dst_rv,
            ControlOp::TopologyCommit,
        )?;

        if cp_lane != operands.src_lane {
            return Err(CpError::Authorisation {
                operation: ControlOp::TopologyCommit as u8,
            });
        }

        if let Some(header_gen) = generation
            && header_gen != operands.new_gen
        {
            return Err(CpError::Topology(TopologyError::GenerationMismatch));
        }

        if rv_id != operands.src_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: rv_id.raw(),
            });
        }
        Ok(operands)
    }

    #[inline]
    fn validate_session_lane_handle(
        expected_sid: SessionId,
        expected_lane: Lane,
        handle: SessionLaneHandle,
        operation: ControlOp,
    ) -> Result<(), CpError> {
        let handle_sid = SessionId::new(handle.sid());
        let handle_lane =
            Lane::try_new(u32::from(handle.lane())).ok_or(CpError::Authorisation {
                operation: operation as u8,
            })?;
        if handle_sid != expected_sid || handle_lane != expected_lane {
            return Err(CpError::Authorisation {
                operation: operation as u8,
            });
        }
        Ok(())
    }

    fn dispatch_abort_begin_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::AbortBegin)?;

        self.run_effect(rv_id, CpCommand::abort_begin(cp_sid, cp_lane))?;
        let _ = generation;
        Ok(())
    }

    fn dispatch_abort_ack_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::AbortAck)?;

        let effect_gen = generation.ok_or(CpError::Authorisation {
            operation: ControlOp::AbortAck as u8,
        })?;
        self.require_local_lane_generation(rv_id, cp_lane, effect_gen)?;
        self.run_effect(rv_id, CpCommand::abort_ack(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_state_snapshot_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::StateSnapshot)?;

        if let Some(effect_gen) = generation {
            self.require_local_lane_generation(rv_id, cp_lane, effect_gen)?;
        }
        self.run_effect(rv_id, CpCommand::state_snapshot(cp_sid, cp_lane))
    }

    fn dispatch_tx_commit_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::TxCommit)?;

        let effect_gen = generation.ok_or(CpError::Authorisation {
            operation: ControlOp::TxCommit as u8,
        })?;
        self.run_effect(rv_id, CpCommand::tx_commit(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_state_restore_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::StateRestore)?;

        let effect_gen = generation.ok_or(CpError::Authorisation {
            operation: ControlOp::StateRestore as u8,
        })?;
        self.run_effect(rv_id, CpCommand::state_restore(cp_sid, cp_lane, effect_gen))
    }

    fn dispatch_tx_abort_with_handle(
        &self,
        rv_id: RendezvousId,
        cp_sid: SessionId,
        cp_lane: Lane,
        handle: SessionLaneHandle,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::TxAbort)?;

        let effect_gen = generation.ok_or(CpError::Authorisation {
            operation: ControlOp::TxAbort as u8,
        })?;
        self.run_effect(rv_id, CpCommand::tx_abort(cp_sid, cp_lane, effect_gen))
    }

    #[inline]
    fn descriptor_epoch_generation(op: ControlOp, expected_epoch: u16) -> Option<Generation> {
        match op {
            ControlOp::AbortAck
            | ControlOp::StateSnapshot
            | ControlOp::StateRestore
            | ControlOp::TxCommit
            | ControlOp::TxAbort => Some(Generation::new(expected_epoch)),
            _ => None,
        }
    }

    #[inline]
    fn descriptor_dispatch_generation(
        op: ControlOp,
        expected_epoch: u16,
        generation: Option<Generation>,
    ) -> Result<Option<Generation>, CpError> {
        let Some(descriptor_generation) = Self::descriptor_epoch_generation(op, expected_epoch)
        else {
            return Ok(generation);
        };
        if let Some(generation) = generation
            && generation != descriptor_generation
        {
            return Err(CpError::GenerationViolation {
                expected: descriptor_generation.raw(),
                actual: generation.raw(),
            });
        }
        Ok(Some(descriptor_generation))
    }

    #[inline]
    fn require_generation(actual: Generation, expected: Generation) -> Result<(), CpError> {
        if actual == expected {
            Ok(())
        } else {
            Err(CpError::GenerationViolation {
                expected: expected.raw(),
                actual: actual.raw(),
            })
        }
    }

    #[inline]
    fn require_local_lane_generation(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
        expected: Generation,
    ) -> Result<(), CpError> {
        Self::require_generation(self.local_lane_generation(rv_id, lane)?, expected)
    }

    #[inline]
    fn local_lane_generation(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
    ) -> Result<Generation, CpError> {
        self.get_local(&rv_id)
            .map(|rv| rv.lane_generation(lane))
            .ok_or(CpError::RendezvousMismatch {
                expected: rv_id.raw(),
                actual: 0,
            })
    }

    #[inline]
    fn local_snapshot_generation_for_commit(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
    ) -> Result<Generation, CpError> {
        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        rv.snapshot_generation(lane)
            .ok_or(CpError::TxCommit(TxCommitError::NoStateSnapshot))
    }

    #[inline]
    fn local_snapshot_generation_for_restore(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
    ) -> Result<Generation, CpError> {
        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        rv.snapshot_generation(lane)
            .ok_or(CpError::StateRestore(StateRestoreError::EpochNotFound))
    }

    #[inline]
    fn local_snapshot_generation_for_abort(
        &self,
        rv_id: RendezvousId,
        lane: Lane,
    ) -> Result<Generation, CpError> {
        let rv = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        rv.snapshot_generation(lane)
            .ok_or(CpError::TxAbort(TxAbortError::NoStateSnapshot))
    }

    pub(crate) fn preflight_topology_begin(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        self.with_control_mut(|core| {
            if core.topology_state.contains_sid(sid) {
                return Err(CpError::ReplayDetected {
                    operation: ControlOp::TopologyBegin as u8,
                    nonce: sid.raw(),
                });
            }
            core.ensure_distributed_topology_capacity(operands.src_rv, 1)
        })
    }

    pub(crate) fn preflight_topology_ack(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<(), CpError> {
        self.with_control_mut(|core| {
            core.topology_state
                .preflight_ack(sid, operands.src_rv, operands.ack(sid))
        })
    }

    pub(crate) fn abort_inflight_topology_entry(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
        sid: SessionId,
        src_rv: RendezvousId,
    ) -> Result<TopologyOperands, CpError> {
        let operands = core
            .topology_state
            .get(sid)
            .copied()
            .ok_or(CpError::Topology(TopologyError::InvalidSession))?;
        debug_assert_eq!(operands.src_rv, src_rv);

        {
            let rv = core
                .locals
                .get_mut(&operands.src_rv)
                .ok_or(CpError::RendezvousMismatch {
                    expected: operands.src_rv.raw(),
                    actual: 0,
                })?;
            rv.abort_topology_state(sid)
                .map_err(|err| CpError::Topology(err.into()))?;
        }

        if operands.dst_rv != operands.src_rv {
            let rv = core
                .locals
                .get_mut(&operands.dst_rv)
                .ok_or(CpError::RendezvousMismatch {
                    expected: operands.dst_rv.raw(),
                    actual: 0,
                })?;
            rv.abort_topology_state(sid)
                .map_err(|err| CpError::Topology(err.into()))?;
        }

        core.topology_state.abort(sid, src_rv)
    }

    pub(crate) fn verify_control_header(
        desc: ControlDesc,
        header: CapHeader,
        expected_scope_id: u16,
        expected_epoch: u16,
    ) -> Result<(), CpError> {
        let mismatch = CpError::Authorisation {
            operation: desc.op() as u8,
        };
        if header.tag() != desc.resource_tag()
            || header.op() != desc.op()
            || header.path() != desc.path()
            || header.shot() != desc.shot()
            || header.scope_kind() != desc.scope_kind()
            || header.flags() != desc.header_flags()
            || header.scope_id() != expected_scope_id
            || header.epoch() != expected_epoch
        {
            return Err(mismatch);
        }
        Ok(())
    }

    pub(crate) fn validate_descriptor_control_frame(
        &self,
        bytes: [u8; CAP_TOKEN_LEN],
        desc: ControlDesc,
        expected_scope_id: u16,
        expected_epoch: u16,
    ) -> Result<(), CpError> {
        let token = GenericCapToken::<()>::from_bytes(bytes);
        let header = token.control_header().map_err(|_| CpError::Authorisation {
            operation: desc.op() as u8,
        })?;
        Self::verify_control_header(desc, header, expected_scope_id, expected_epoch)?;

        match desc.op() {
            ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit => {
                TopologyDescriptor::decode_for(desc.op(), token.handle_bytes())?;
            }
            ControlOp::AbortBegin
            | ControlOp::AbortAck
            | ControlOp::StateSnapshot
            | ControlOp::TxCommit
            | ControlOp::TxAbort
            | ControlOp::StateRestore => {
                decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: desc.op() as u8,
                    }
                })?;
            }
            ControlOp::Fence
            | ControlOp::CapDelegate
            | ControlOp::RouteDecision
            | ControlOp::LoopContinue
            | ControlOp::LoopBreak => {}
        }

        Ok(())
    }

    pub(crate) fn validate_send_bound_descriptor_control_frame(
        &self,
        rv_id: RendezvousId,
        bytes: [u8; CAP_TOKEN_LEN],
        desc: ControlDesc,
        expected_sid: SessionId,
        expected_lane: Lane,
        expected_role: u8,
        expected_scope_id: u16,
        expected_epoch: u16,
    ) -> Result<(), CpError> {
        let token = GenericCapToken::<()>::from_bytes(bytes);
        let header = token.control_header().map_err(|_| CpError::Authorisation {
            operation: desc.op() as u8,
        })?;
        Self::verify_control_header(desc, header, expected_scope_id, expected_epoch)?;
        if header.sid() != expected_sid
            || header.lane() != expected_lane
            || header.role() != expected_role
        {
            return Err(CpError::Authorisation {
                operation: desc.op() as u8,
            });
        }

        let cp_sid = header.sid();
        let cp_lane = header.lane();
        match desc.op() {
            ControlOp::TopologyBegin => {
                let descriptor =
                    TopologyDescriptor::decode_for(ControlOp::TopologyBegin, token.handle_bytes())?;
                let _ = self.validate_topology_begin_operands(
                    rv_id,
                    cp_lane,
                    descriptor.operands(),
                    None,
                )?;
            }
            ControlOp::TopologyAck => {
                let descriptor =
                    TopologyDescriptor::decode_for(ControlOp::TopologyAck, token.handle_bytes())?;
                let _ = self.validate_topology_ack_operands(
                    rv_id,
                    cp_lane,
                    descriptor.operands(),
                    None,
                )?;
            }
            ControlOp::TopologyCommit => {
                let descriptor = TopologyDescriptor::decode_for(
                    ControlOp::TopologyCommit,
                    token.handle_bytes(),
                )?;
                let _ = self.validate_topology_commit_operands(
                    rv_id,
                    cp_lane,
                    descriptor.operands(),
                    None,
                )?;
            }
            ControlOp::AbortBegin => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::AbortBegin as u8,
                    }
                })?;
                Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::AbortBegin)?;
            }
            ControlOp::AbortAck => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::AbortAck as u8,
                    }
                })?;
                Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::AbortAck)?;
                self.require_local_lane_generation(
                    rv_id,
                    cp_lane,
                    Generation::new(expected_epoch),
                )?;
            }
            ControlOp::StateSnapshot => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::StateSnapshot as u8,
                    }
                })?;
                Self::validate_session_lane_handle(
                    cp_sid,
                    cp_lane,
                    handle,
                    ControlOp::StateSnapshot,
                )?;
                self.require_local_lane_generation(
                    rv_id,
                    cp_lane,
                    Generation::new(expected_epoch),
                )?;
            }
            ControlOp::TxCommit => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TxCommit as u8,
                    }
                })?;
                Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::TxCommit)?;
                Self::require_generation(
                    self.local_snapshot_generation_for_commit(rv_id, cp_lane)?,
                    Generation::new(expected_epoch),
                )?;
            }
            ControlOp::TxAbort => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TxAbort as u8,
                    }
                })?;
                Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::TxAbort)?;
                Self::require_generation(
                    self.local_snapshot_generation_for_abort(rv_id, cp_lane)?,
                    Generation::new(expected_epoch),
                )?;
            }
            ControlOp::StateRestore => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::StateRestore as u8,
                    }
                })?;
                Self::validate_session_lane_handle(
                    cp_sid,
                    cp_lane,
                    handle,
                    ControlOp::StateRestore,
                )?;
                Self::require_generation(
                    self.local_snapshot_generation_for_restore(rv_id, cp_lane)?,
                    Generation::new(expected_epoch),
                )?;
            }
            ControlOp::Fence
            | ControlOp::RouteDecision
            | ControlOp::LoopContinue
            | ControlOp::LoopBreak => {}
            ControlOp::CapDelegate => {
                return Err(CpError::UnsupportedEffect(ControlOp::CapDelegate as u8));
            }
        }

        Ok(())
    }

    pub(crate) fn dispatch_descriptor_control_frame(
        &self,
        rv_id: RendezvousId,
        bytes: [u8; CAP_TOKEN_LEN],
        desc: ControlDesc,
        expected_scope_id: u16,
        expected_epoch: u16,
        generation: Option<Generation>,
    ) -> Result<(), CpError> {
        let _ = self.get_local(&rv_id).ok_or(CpError::RendezvousMismatch {
            expected: rv_id.raw(),
            actual: 0,
        })?;
        self.validate_descriptor_control_frame(bytes, desc, expected_scope_id, expected_epoch)?;
        let generation =
            Self::descriptor_dispatch_generation(desc.op(), expected_epoch, generation)?;
        let token = GenericCapToken::<()>::from_bytes(bytes);
        let header = token.control_header().map_err(|_| CpError::Authorisation {
            operation: desc.op() as u8,
        })?;

        let cp_sid = header.sid();
        let cp_lane = header.lane();
        match desc.op() {
            ControlOp::TopologyBegin => {
                let descriptor =
                    TopologyDescriptor::decode_for(ControlOp::TopologyBegin, token.handle_bytes())?;
                let operands = self.validate_topology_begin_operands(
                    rv_id,
                    cp_lane,
                    descriptor.operands(),
                    generation,
                )?;
                self.run_effect(operands.src_rv, CpCommand::topology_begin(cp_sid, operands))?;
            }
            ControlOp::TopologyAck => {
                let descriptor =
                    TopologyDescriptor::decode_for(ControlOp::TopologyAck, token.handle_bytes())?;
                let operands = self.validate_topology_ack_operands(
                    rv_id,
                    cp_lane,
                    descriptor.operands(),
                    generation,
                )?;
                self.run_effect(operands.dst_rv, CpCommand::topology_ack(cp_sid, operands))?;
            }
            ControlOp::AbortBegin => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::AbortBegin as u8,
                    }
                })?;
                self.dispatch_abort_begin_with_handle(rv_id, cp_sid, cp_lane, handle, generation)?;
            }
            ControlOp::AbortAck => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::AbortAck as u8,
                    }
                })?;
                self.dispatch_abort_ack_with_handle(rv_id, cp_sid, cp_lane, handle, generation)?;
            }
            ControlOp::StateSnapshot => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::StateSnapshot as u8,
                    }
                })?;
                self.dispatch_state_snapshot_with_handle(
                    rv_id, cp_sid, cp_lane, handle, generation,
                )?;
            }
            ControlOp::TxCommit => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TxCommit as u8,
                    }
                })?;
                self.dispatch_tx_commit_with_handle(rv_id, cp_sid, cp_lane, handle, generation)?;
            }
            ControlOp::TxAbort => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::TxAbort as u8,
                    }
                })?;
                self.dispatch_tx_abort_with_handle(rv_id, cp_sid, cp_lane, handle, generation)?;
            }
            ControlOp::StateRestore => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: desc.op() as u8,
                    }
                })?;
                self.dispatch_state_restore_with_handle(
                    rv_id, cp_sid, cp_lane, handle, generation,
                )?;
            }
            ControlOp::Fence
            | ControlOp::RouteDecision
            | ControlOp::LoopContinue
            | ControlOp::LoopBreak => {}
            ControlOp::CapDelegate => {
                return Err(CpError::UnsupportedEffect(ControlOp::CapDelegate as u8));
            }
            ControlOp::TopologyCommit => {
                let descriptor = TopologyDescriptor::decode_for(
                    ControlOp::TopologyCommit,
                    token.handle_bytes(),
                )?;
                let operands = self.validate_topology_commit_operands(
                    rv_id,
                    cp_lane,
                    descriptor.operands(),
                    generation,
                )?;
                self.run_effect(
                    operands.src_rv,
                    CpCommand::topology_commit(cp_sid, operands),
                )?;
            }
        }
        Ok(())
    }
}
