use super::super::{ReservedDestinationTopologyCommitProof, ReservedSourceTopologyCommitProof};
use super::{
    Clock, ControlOp, Generation, LabelUniverse, Lane, Rendezvous, SessionId, TopologyError,
    Transport,
};

impl<'rv, 'cfg, T, U, C, E> Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    pub(crate) fn reserve_source_topology_commit(
        &self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<ReservedSourceTopologyCommitProof, TopologyError> {
        self.topology.reserve_source_commit(lane, sid)
    }

    pub(crate) fn rollback_source_topology_commit_reservation(
        &self,
        sid: SessionId,
        lane: Lane,
        ticket: ReservedSourceTopologyCommitProof,
    ) {
        self.topology
            .rollback_source_commit_reserved(lane, sid, ticket)
    }

    pub(crate) fn reserve_destination_topology_commit(
        &self,
        sid: SessionId,
        lane: Lane,
    ) -> Result<ReservedDestinationTopologyCommitProof, TopologyError> {
        self.topology.reserve_destination_commit(lane, sid)
    }

    pub(crate) fn rollback_destination_topology_commit_reservation(
        &self,
        sid: SessionId,
        lane: Lane,
        ticket: ReservedDestinationTopologyCommitProof,
    ) {
        self.topology
            .rollback_destination_commit_reserved(lane, sid, ticket)
    }

    pub(crate) fn assert_prepared_destination_topology_commit(
        &self,
        ticket: &ReservedDestinationTopologyCommitProof,
        sid: SessionId,
        lane: Lane,
        target: Generation,
    ) {
        assert_eq!(self.r#gen.last(lane), ticket.previous_generation());
        assert_eq!(ticket.target(), target);
        self.topology
            .assert_destination_commit_reserved(lane, sid, ticket);
    }

    pub(crate) fn assert_prepared_source_topology_commit(
        &self,
        ticket: &ReservedSourceTopologyCommitProof,
        sid: SessionId,
        lane: Lane,
        target: Generation,
    ) {
        assert!(self.validate_topology_generation(lane, target).is_ok());
        assert_eq!(ticket.target(), target);
        self.topology
            .assert_source_commit_reserved(lane, sid, ticket);
    }

    pub(crate) fn publish_prepared_destination_topology_commit(
        &mut self,
        ticket: ReservedDestinationTopologyCommitProof,
        lane: Lane,
    ) {
        let target = ticket.target();
        self.r#gen.publish_prepared(lane, target);
        self.topology
            .finalize_prepared_destination_commit_unchecked(ticket);
    }

    pub(crate) fn publish_prepared_source_topology_commit(
        &mut self,
        ticket: ReservedSourceTopologyCommitProof,
        sid: SessionId,
        lane: Lane,
    ) {
        let target = ticket.target();
        self.topology
            .clear_prepared_source_commit_unchecked(&ticket);
        self.r#gen.publish_prepared(lane, target);
        ticket.commit();
        let packed = ((lane.as_wire() as u32) & 0xFF) | ((target.0 as u32) << 16);
        self.emit_effect(ControlOp::TopologyCommit, sid, lane, packed);
        self.revoke_public_endpoints_for_session(sid);
        self.retire_session_lanes(sid);
    }
}
