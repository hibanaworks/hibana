use super::descriptor_terminal::{ReservedTopologyCommitMeta, ReservedTopologyCommitPublication};
use crate::control::cluster::core::{
    ControlCore, PreparedDistributedTopologyCommit, SessionCluster,
};
use crate::rendezvous::core::{
    ReservedDestinationTopologyCommitProof, ReservedSourceTopologyCommitProof,
};

type ClusterCore<'cfg, T, U, C, const MAX_RV: usize> =
    ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>;

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    pub(super) fn rollback_prepared_topology_commit_reservations(
        core: &mut ClusterCore<'cfg, T, U, C, MAX_RV>,
        ticket: ReservedTopologyCommitPublication,
    ) {
        let (meta, source, destination, distributed) = ticket.into_proofs();
        Self::rollback_prepared_topology_commit_parts(core, meta, source, destination, distributed);
    }

    fn rollback_prepared_topology_commit_parts(
        core: &mut ClusterCore<'cfg, T, U, C, MAX_RV>,
        meta: ReservedTopologyCommitMeta,
        source: ReservedSourceTopologyCommitProof,
        destination: ReservedDestinationTopologyCommitProof,
        distributed: PreparedDistributedTopologyCommit,
    ) {
        let sid = distributed.sid();
        Self::rollback_prepared_topology_commit_local_parts(core, sid, meta, source, destination);
        core.topology_state.rollback_commit_reserved(distributed);
    }

    fn rollback_prepared_topology_commit_local_parts(
        core: &mut ClusterCore<'cfg, T, U, C, MAX_RV>,
        sid: crate::control::cluster::core::SessionId,
        meta: ReservedTopologyCommitMeta,
        source: ReservedSourceTopologyCommitProof,
        destination: ReservedDestinationTopologyCommitProof,
    ) {
        {
            let rv = core.locals.get_mut_by_proof(meta.dst_owner());
            rv.rollback_destination_topology_commit_reservation(sid, meta.dst_lane(), destination);
        }
        {
            let rv = core.locals.get_mut_by_proof(meta.src_owner());
            rv.rollback_source_topology_commit_reservation(sid, meta.src_lane(), source);
        }
    }
}
