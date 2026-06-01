mod prepared_send;

pub(crate) use prepared_send::{DescriptorPublicationAuthority, DescriptorTerminal};

use super::{
    CAP_TOKEN_LEN, CapHeader, ControlCore, ControlDesc, ControlOp, CpError, DistributedPhaseKind,
    Generation, GenericCapToken, Lane, RendezvousId, SessionCluster, SessionId, SessionLaneHandle,
    StateRestoreError, TopologyDescriptor, TopologyOperands, TxAbortError, TxCommitError,
    decode_session_lane_handle, validate_topology_rendezvous_pair,
};
use crate::rendezvous::TopologySessionState;

#[derive(Clone, Copy)]
pub(crate) struct ValidatedDescriptorControlFrame {
    pub(crate) sid: SessionId,
    pub(crate) lane: Lane,
    pub(crate) effect: ValidatedDescriptorControlEffect,
}

#[derive(Clone, Copy)]
pub(crate) enum ValidatedDescriptorControlEffect {
    None,
    TopologyBegin(TopologyOperands),
    TopologyAck(TopologyOperands),
    TopologyCommit(TopologyOperands),
    AbortBegin,
    AbortAck(Generation),
    StateSnapshot(Generation),
    StateRestore(Generation),
    TxCommit(Generation),
    TxAbort(Generation),
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(crate) fn validate_topology_begin_operands(
        &self,
        rv_id: RendezvousId,
        cp_lane: Lane,
        operands: TopologyOperands,
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

        if rv_id != operands.src_rv {
            return Err(CpError::RendezvousMismatch {
                expected: operands.src_rv.raw(),
                actual: rv_id.raw(),
            });
        }
        Ok(operands)
    }

    pub(crate) fn validate_topology_ack_operands(
        &self,
        rv_id: RendezvousId,
        cp_lane: Lane,
        operands: TopologyOperands,
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

    pub(crate) fn abort_inflight_topology_entry(
        core: &mut ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>,
        sid: SessionId,
        src_rv: RendezvousId,
    ) -> Result<TopologyOperands, CpError> {
        let (operands, phase) = core.topology_state.preflight_abort(sid, src_rv)?;
        let src_owner =
            core.locals
                .owner_proof(operands.src_rv)
                .map_err(|_| CpError::RendezvousMismatch {
                    expected: operands.src_rv.raw(),
                    actual: 0,
                })?;
        let dst_owner = if operands.dst_rv != operands.src_rv {
            Some(core.locals.owner_proof(operands.dst_rv).map_err(|_| {
                CpError::RendezvousMismatch {
                    expected: operands.dst_rv.raw(),
                    actual: 0,
                }
            })?)
        } else {
            None
        };

        assert_eq!(
            core.locals
                .get_mut_by_proof(src_owner)
                .topology_session_state(sid),
            Some(TopologySessionState::SourcePending {
                lane: operands.src_lane
            }),
            "distributed topology abort missing source local pending state"
        );
        let destination_pending = matches!(
            phase,
            DistributedPhaseKind::AckReserved | DistributedPhaseKind::Acked
        );
        match (destination_pending, dst_owner) {
            (true, Some(dst_owner)) => {
                assert_eq!(
                    core.locals
                        .get_mut_by_proof(dst_owner)
                        .topology_session_state(sid),
                    Some(TopologySessionState::DestinationPending {
                        lane: operands.dst_lane
                    }),
                    "distributed topology abort missing destination local pending state"
                );
            }
            (true, None) => {
                panic!("distributed topology abort missing destination owner proof");
            }
            (false, Some(dst_owner)) => {
                assert_eq!(
                    core.locals
                        .get_mut_by_proof(dst_owner)
                        .topology_session_state(sid),
                    None,
                    "distributed topology begin abort found unexpected destination local pending state"
                );
            }
            (false, None) => {}
        }
        assert!(
            core.locals
                .get_mut_by_proof(src_owner)
                .abort_topology_state(sid),
            "distributed topology abort source local pending state disappeared after preflight"
        );
        if let (true, Some(dst_owner)) = (destination_pending, dst_owner) {
            assert!(
                core.locals
                    .get_mut_by_proof(dst_owner)
                    .abort_topology_state(sid),
                "distributed topology abort destination local pending state disappeared after preflight"
            );
        }
        let aborted =
            core.topology_state
                .commit_preflighted_abort(sid, operands.src_rv, operands, phase);
        Ok(aborted)
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

    pub(crate) fn validate_bound_descriptor_control_frame(
        &self,
        rv_id: RendezvousId,
        bytes: [u8; CAP_TOKEN_LEN],
        desc: ControlDesc,
        expected_sid: SessionId,
        expected_lane: Lane,
        expected_role: u8,
        expected_scope_id: u16,
        expected_epoch: u16,
    ) -> Result<ValidatedDescriptorControlFrame, CpError> {
        let token = GenericCapToken::<()>::from_raw_bytes(bytes);
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
        let generation = Generation::new(expected_epoch);
        let effect = match desc.op() {
            ControlOp::TopologyBegin => {
                let descriptor =
                    TopologyDescriptor::decode_for(ControlOp::TopologyBegin, token.handle_bytes())?;
                let operands =
                    self.validate_topology_begin_operands(rv_id, cp_lane, descriptor.operands())?;
                ValidatedDescriptorControlEffect::TopologyBegin(operands)
            }
            ControlOp::TopologyAck => {
                let descriptor =
                    TopologyDescriptor::decode_for(ControlOp::TopologyAck, token.handle_bytes())?;
                let operands =
                    self.validate_topology_ack_operands(rv_id, cp_lane, descriptor.operands())?;
                ValidatedDescriptorControlEffect::TopologyAck(operands)
            }
            ControlOp::TopologyCommit => {
                let descriptor = TopologyDescriptor::decode_for(
                    ControlOp::TopologyCommit,
                    token.handle_bytes(),
                )?;
                let operands =
                    self.validate_topology_commit_operands(rv_id, cp_lane, descriptor.operands())?;
                ValidatedDescriptorControlEffect::TopologyCommit(operands)
            }
            ControlOp::AbortBegin => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::AbortBegin as u8,
                    }
                })?;
                Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::AbortBegin)?;
                ValidatedDescriptorControlEffect::AbortBegin
            }
            ControlOp::AbortAck => {
                let handle = decode_session_lane_handle(token.handle_bytes()).map_err(|_| {
                    CpError::Authorisation {
                        operation: ControlOp::AbortAck as u8,
                    }
                })?;
                Self::validate_session_lane_handle(cp_sid, cp_lane, handle, ControlOp::AbortAck)?;
                self.require_local_lane_generation(rv_id, cp_lane, generation)?;
                ValidatedDescriptorControlEffect::AbortAck(generation)
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
                self.require_local_lane_generation(rv_id, cp_lane, generation)?;
                ValidatedDescriptorControlEffect::StateSnapshot(generation)
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
                    generation,
                )?;
                ValidatedDescriptorControlEffect::TxCommit(generation)
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
                    generation,
                )?;
                ValidatedDescriptorControlEffect::TxAbort(generation)
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
                    generation,
                )?;
                ValidatedDescriptorControlEffect::StateRestore(generation)
            }
            ControlOp::Fence
            | ControlOp::RouteDecision
            | ControlOp::LoopContinue
            | ControlOp::LoopBreak => ValidatedDescriptorControlEffect::None,
        };

        Ok(ValidatedDescriptorControlFrame {
            sid: cp_sid,
            lane: cp_lane,
            effect,
        })
    }
}
