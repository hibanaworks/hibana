mod descriptor_effects;
mod descriptor_terminal;
mod topology_commit_rollback;

pub(crate) use descriptor_terminal::{DescriptorPublicationAuthority, DescriptorTerminal};

use self::descriptor_terminal::{DescriptorTerminalCase, ReservedTopologyTerminal};
use super::ValidatedDescriptorControlEffect;
use crate::control::cluster::core::{
    CAP_TOKEN_LEN, ControlCore, ControlDesc, CpError, Lane, RendezvousId, ResourceScope,
    SessionCluster, SessionId, TopologyOperands,
};
use crate::control::lease::core::RendezvousOwnerProof;

type ClusterCore<'cfg, T, U, C, const MAX_RV: usize> =
    ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>;

fn ensure_local_topology_storage_in_core<'cfg, T, U, C, const MAX_RV: usize>(
    core: &mut ClusterCore<'cfg, T, U, C, MAX_RV>,
    owner: RendezvousOwnerProof,
    lane: Lane,
) -> Result<(), CpError>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    let rv = core.locals.get_mut_by_proof(owner);
    rv.ensure_topology_control_storage_for_lane_slots((lane.raw() as usize).saturating_add(1))
        .ok_or(CpError::resource_exhausted(ResourceScope::Generic))
}

fn require_local_topology_storage_in_core<'cfg, T, U, C, const MAX_RV: usize>(
    core: &mut ClusterCore<'cfg, T, U, C, MAX_RV>,
    owner: RendezvousOwnerProof,
    lane: Lane,
) -> Result<(), CpError>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    let rv = core.locals.get_mut_by_proof(owner);
    if rv.has_topology_control_storage_for_lane(lane) {
        Ok(())
    } else {
        Err(CpError::resource_exhausted(ResourceScope::Generic))
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize>
    crate::endpoint::kernel::EndpointRevocationDescriptorRollback
    for ClusterCore<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    #[inline(never)]
    fn rollback_endpoint_revocation_descriptor(&mut self, ticket: DescriptorTerminal) {
        SessionCluster::<T, U, C, MAX_RV>::rollback_descriptor_terminal_in_core(self, ticket);
    }
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    #[inline]
    pub(crate) fn descriptor_publication_authority(
        &'cfg self,
    ) -> DescriptorPublicationAuthority<'cfg> {
        DescriptorPublicationAuthority::new(self)
    }

    #[inline(never)]
    pub(crate) fn prepare_send_bound_descriptor_terminal(
        &self,
        rv_id: RendezvousId,
        bytes: [u8; CAP_TOKEN_LEN],
        desc: ControlDesc,
        expected_sid: SessionId,
        expected_lane: Lane,
        expected_role: u8,
        expected_scope_id: u16,
        expected_epoch: u16,
    ) -> Result<DescriptorTerminal, CpError> {
        let frame = self.validate_bound_descriptor_control_frame(
            rv_id,
            bytes,
            desc,
            expected_sid,
            expected_lane,
            expected_role,
            expected_scope_id,
            expected_epoch,
        )?;
        match frame.effect {
            ValidatedDescriptorControlEffect::TopologyBegin(operands) => {
                self.prepare_topology_begin_descriptor_commit(frame.sid, operands)
            }
            ValidatedDescriptorControlEffect::TopologyAck(operands) => {
                self.prepare_topology_ack_descriptor_commit(frame.sid, operands)
            }
            ValidatedDescriptorControlEffect::TopologyCommit(operands) => {
                self.prepare_topology_commit_descriptor_commit(frame.sid, operands)
            }
            ValidatedDescriptorControlEffect::AbortBegin => {
                self.prepare_abort_begin_descriptor_terminal(rv_id, frame.sid, frame.lane)
            }
            ValidatedDescriptorControlEffect::AbortAck(generation) => {
                self.prepare_abort_ack_descriptor_terminal(rv_id, frame.sid, frame.lane, generation)
            }
            ValidatedDescriptorControlEffect::StateSnapshot(generation) => self
                .prepare_state_snapshot_descriptor_terminal(
                    rv_id, frame.sid, frame.lane, generation,
                ),
            ValidatedDescriptorControlEffect::StateRestore(generation) => self
                .prepare_state_restore_descriptor_terminal(
                    rv_id, frame.sid, frame.lane, generation,
                ),
            ValidatedDescriptorControlEffect::TxCommit(generation) => {
                self.prepare_tx_commit_descriptor_terminal(rv_id, frame.sid, frame.lane, generation)
            }
            ValidatedDescriptorControlEffect::TxAbort(generation) => {
                self.prepare_tx_abort_descriptor_terminal(rv_id, frame.sid, frame.lane, generation)
            }
            ValidatedDescriptorControlEffect::None => Ok(DescriptorTerminal::none()),
        }
    }

    #[inline(never)]
    fn prepare_topology_begin_descriptor_commit(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<DescriptorTerminal, CpError> {
        self.with_control_mut(|core| {
            let owner = core.locals.owner_proof(operands.src_rv).map_err(|_| {
                CpError::RendezvousMismatch {
                    expected: operands.src_rv.raw(),
                    actual: 0,
                }
            })?;
            core.topology_state.preflight_begin(sid, operands)?;
            {
                let rv = core.locals.get_mut_by_proof(owner);
                rv.preflight_topology_begin_from_intent(operands.intent(sid))
                    .map_err(|err| CpError::Topology(err.into()))?;
            }
            let distributed_begin_capacity =
                core.reserve_distributed_topology_begin_capacity(sid, operands, owner)?;
            if let Err(err) = ensure_local_topology_storage_in_core(core, owner, operands.src_lane)
            {
                core.rollback_distributed_topology_begin_capacity(distributed_begin_capacity);
                return Err(err);
            }
            {
                let rv = core.locals.get_mut_by_proof(owner);
                if let Err(err) = rv.prepare_topology_begin_from_intent(operands.intent(sid)) {
                    core.rollback_distributed_topology_begin_capacity(distributed_begin_capacity);
                    return Err(CpError::Topology(err.into()));
                }
            };
            let (ack, distributed) =
                core.publish_distributed_topology_begin(distributed_begin_capacity, sid, operands);
            Ok(DescriptorTerminal::topology_begin(ack, owner, distributed))
        })
    }

    #[inline(never)]
    fn prepare_topology_ack_descriptor_commit(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<DescriptorTerminal, CpError> {
        self.with_control_mut(|core| {
            let owner = core.locals.owner_proof(operands.dst_rv).map_err(|_| {
                CpError::RendezvousMismatch {
                    expected: operands.dst_rv.raw(),
                    actual: 0,
                }
            })?;
            let expected_ack = operands.ack(sid);
            core.topology_state
                .preflight_ack(sid, operands.src_rv, expected_ack)?;
            {
                let rv = core.locals.get_mut_by_proof(owner);
                rv.preflight_destination_topology_ack(&operands.intent(sid))
                    .map_err(|err| CpError::Topology(err.into()))?;
            }
            ensure_local_topology_storage_in_core(core, owner, operands.dst_lane)?;
            let destination = {
                let rv = core.locals.get_mut_by_proof(owner);
                rv.prepare_destination_topology_ack(&operands.intent(sid))
                    .map_err(|err| CpError::Topology(err.into()))?
            };
            assert_eq!(destination.ack(), expected_ack);
            let distributed =
                core.topology_state
                    .reserve_preflighted_ack(sid, operands.src_rv, expected_ack);
            Ok(DescriptorTerminal::topology_ack(
                destination,
                owner,
                distributed,
            ))
        })
    }

    #[inline(never)]
    fn prepare_topology_commit_descriptor_commit(
        &self,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> Result<DescriptorTerminal, CpError> {
        self.with_control_mut(|core| {
            let src_owner = core.locals.owner_proof(operands.src_rv).map_err(|_| {
                CpError::RendezvousMismatch {
                    expected: operands.src_rv.raw(),
                    actual: 0,
                }
            })?;
            let dst_owner = core.locals.owner_proof(operands.dst_rv).map_err(|_| {
                CpError::RendezvousMismatch {
                    expected: operands.dst_rv.raw(),
                    actual: 0,
                }
            })?;
            core.topology_state
                .preflight_commit(sid, operands.src_rv, Some(operands.ack(sid)))?;
            let source_lane = {
                let rv = core.locals.get_mut_by_proof(src_owner);
                rv.validate_topology_commit_operands(sid, operands)
                    .map_err(|err| CpError::Topology(err.into()))?
            };
            {
                let rv = core.locals.get_mut_by_proof(dst_owner);
                rv.preflight_destination_topology_commit(sid, operands.dst_lane)
                    .map_err(|err| CpError::Topology(err.into()))?;
            };
            require_local_topology_storage_in_core(core, src_owner, operands.src_lane)?;
            require_local_topology_storage_in_core(core, dst_owner, operands.dst_lane)?;
            let source_proof = {
                let rv = core.locals.get_mut_by_proof(src_owner);
                rv.reserve_source_topology_commit(sid, source_lane)
                    .map_err(|err| CpError::Topology(err.into()))?
            };
            let destination_proof = match {
                let rv = core.locals.get_mut_by_proof(dst_owner);
                rv.reserve_destination_topology_commit(sid, operands.dst_lane)
                    .map_err(|err| CpError::Topology(err.into()))
            } {
                Ok(proof) => proof,
                Err(err) => {
                    core.locals
                        .get_mut_by_proof(src_owner)
                        .rollback_source_topology_commit_reservation(
                            sid,
                            source_lane,
                            source_proof,
                        );
                    return Err(err);
                }
            };
            let distributed_proof = match core.topology_state.reserve_commit(
                sid,
                operands.src_rv,
                Some(operands.ack(sid)),
            ) {
                Ok(proof) => proof,
                Err(err) => {
                    core.locals
                        .get_mut_by_proof(dst_owner)
                        .rollback_destination_topology_commit_reservation(
                            sid,
                            operands.dst_lane,
                            destination_proof,
                        );
                    core.locals
                        .get_mut_by_proof(src_owner)
                        .rollback_source_topology_commit_reservation(
                            sid,
                            source_lane,
                            source_proof,
                        );
                    return Err(err);
                }
            };
            let ack = operands.ack(sid);
            debug_assert_eq!(ack.src_lane, source_lane);
            Ok(DescriptorTerminal::commit_topology(
                ack,
                src_owner,
                dst_owner,
                source_proof,
                destination_proof,
                distributed_proof,
            ))
        })
    }

    #[inline(never)]
    pub(crate) fn rollback_descriptor_terminal(&self, ticket: DescriptorTerminal) {
        self.with_control_mut(|core| Self::rollback_descriptor_terminal_in_core(core, ticket));
    }

    #[inline(never)]
    pub(super) fn rollback_descriptor_terminal_in_core(
        core: &mut ClusterCore<'cfg, T, U, C, MAX_RV>,
        ticket: DescriptorTerminal,
    ) {
        match ticket.into_case() {
            DescriptorTerminalCase::ReservedTopology(ticket) => match ticket {
                ReservedTopologyTerminal::Begin(ticket) => {
                    let (ack, owner, distributed) = ticket.into_parts();
                    let sid = SessionId::new(ack.sid);
                    let rv = core.locals.get_mut_by_proof(owner);
                    rv.rollback_prepared_topology_begin(sid);
                    core.topology_state.rollback_prepared_begin(distributed);
                }
                ReservedTopologyTerminal::Ack(ticket) => {
                    let (destination, owner, distributed) = ticket.into_parts();
                    let rv = core.locals.get_mut_by_proof(owner);
                    rv.rollback_prepared_destination_topology_ack(destination);
                    core.topology_state.rollback_prepared_ack(distributed);
                }
                ReservedTopologyTerminal::Commit(ticket) => {
                    Self::rollback_prepared_topology_commit_reservations(core, ticket);
                }
            },
            DescriptorTerminalCase::DescriptorEffectTerminal(ticket) => {
                Self::rollback_descriptor_effect_terminal_in_core(core, ticket);
            }
            DescriptorTerminalCase::None => {}
        }
    }

    #[inline(never)]
    pub(crate) fn publish_descriptor_terminal(&self, ticket: DescriptorTerminal) {
        match ticket.into_case() {
            DescriptorTerminalCase::None => {}
            DescriptorTerminalCase::ReservedTopology(ticket) => {
                self.publish_reserved_topology_terminal(ticket);
            }
            DescriptorTerminalCase::DescriptorEffectTerminal(ticket) => {
                self.publish_descriptor_effect_terminal(ticket);
            }
        }
    }

    #[inline(never)]
    fn publish_reserved_topology_terminal(&self, ticket: ReservedTopologyTerminal) {
        let revocation = self.with_control_mut(|core| match ticket {
            ReservedTopologyTerminal::Begin(ticket) => {
                let (ack, owner, distributed) = ticket.into_parts();
                let sid = SessionId::new(ack.sid);
                let rv_ptr = core::ptr::from_mut(core.locals.get_mut_by_proof(owner));
                core.topology_state.publish_prepared_begin(distributed);
                unsafe {
                    // SAFETY: the owner proof was minted with the distributed
                    // reservation; the slot assertion above gives the pinned
                    // rendezvous owner before terminal proof consumption.
                    (&mut *rv_ptr).publish_prepared_topology_begin(sid, ack.src_lane, ack.new_gen);
                }
                None
            }
            ReservedTopologyTerminal::Ack(ticket) => {
                let (destination, owner, distributed) = ticket.into_parts();
                let rv_ptr = core::ptr::from_mut(core.locals.get_mut_by_proof(owner));
                core.topology_state.publish_prepared_ack(distributed);
                unsafe {
                    // SAFETY: the owner proof was minted with the distributed
                    // reservation; the slot assertion above gives the pinned
                    // rendezvous owner before terminal proof consumption.
                    (&mut *rv_ptr).publish_prepared_destination_topology_ack(destination);
                }
                None
            }
            ReservedTopologyTerminal::Commit(ticket) => {
                let (meta, source, destination, distributed) = ticket.into_proofs();
                let sid = distributed.sid();
                let (src, dst) = core
                    .locals
                    .get_pair_mut_by_proof(meta.src_owner(), meta.dst_owner());
                let src_ptr = core::ptr::from_mut(src);
                let dst_ptr = core::ptr::from_mut(dst);
                core.topology_state.assert_prepared_commit(&distributed);
                let generation = source.target();
                assert_eq!(generation, destination.target());
                unsafe {
                    // SAFETY: both pointers were captured from distinct pinned
                    // rendezvous owners before any topology commit proof is
                    // terminally consumed. These assertion-only checks close
                    // every release invariant before the distributed publish
                    // boundary.
                    (&*dst_ptr).assert_prepared_destination_topology_commit(
                        &destination,
                        sid,
                        meta.dst_lane(),
                        generation,
                    );
                    (&*src_ptr).assert_prepared_source_topology_commit(
                        &source,
                        sid,
                        meta.src_lane(),
                        generation,
                    );
                }
                core.topology_state.publish_prepared_commit(distributed);
                // Distributed commit proof is consumed; the remaining local proof
                // publication path must not re-enter owner lookup or return.
                unsafe {
                    // SAFETY: both pointers were captured from distinct pinned
                    // rendezvous owners after local proof checks and before the
                    // distributed commit proof was terminally consumed.
                    // Rendezvous entries are not removed while the control-core
                    // mutation closure is active, so local proof publication has
                    // no post-consume owner lookup or early-return path.
                    (&mut *dst_ptr)
                        .publish_prepared_destination_topology_commit(destination, meta.dst_lane());
                    (&mut *src_ptr).publish_prepared_source_topology_commit(
                        source,
                        sid,
                        meta.src_lane(),
                    );
                }
                Some((meta.src_owner(), sid))
            }
        });
        if let Some((source_owner, sid)) = revocation {
            self.finish_topology_commit_revocation(source_owner, sid);
        }
    }

    fn finish_topology_commit_revocation(
        &self,
        source_owner: RendezvousOwnerProof,
        sid: SessionId,
    ) {
        loop {
            let Some(endpoint) = self.drain_one_topology_commit_revocation(source_owner, sid)
            else {
                break;
            };
            endpoint.finish();
        }
        self.retire_topology_commit_session_lanes(source_owner, sid);
    }

    fn drain_one_topology_commit_revocation(
        &self,
        source_owner: RendezvousOwnerProof,
        sid: SessionId,
    ) -> Option<crate::rendezvous::core::RevokedPublicEndpoint<'cfg>> {
        self.with_control_mut(|core| {
            let revocation = {
                let src = core.locals.get_mut_by_proof(source_owner);
                src.prepare_one_public_endpoint_revocation(sid)
            }?;
            let revocation = revocation.rollback_descriptor_with(core);
            let endpoint = {
                let src = core.locals.get_mut_by_proof(source_owner);
                src.commit_prepared_public_endpoint_revocation(revocation)
            };
            Some(endpoint)
        })
    }

    fn retire_topology_commit_session_lanes(
        &self,
        source_owner: RendezvousOwnerProof,
        sid: SessionId,
    ) {
        self.with_control_mut(|core| {
            let src = core.locals.get_mut_by_proof(source_owner);
            src.retire_session_lanes_for_topology(sid);
        });
    }
}
