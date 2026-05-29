use super::{
    Lane, PreparedDistributedTopologyAck, PreparedDistributedTopologyBegin,
    ReservedDestinationTopologyCommitProof, ReservedSourceTopologyCommitProof,
    ReservedTopologyAckPublication, ReservedTopologyBeginPublication, ReservedTopologyCommitMeta,
    ReservedTopologyCommitPublication, TopologyAck,
};
use crate::control::cluster::core::PreparedDistributedTopologyCommit;
use crate::control::lease::core::RendezvousOwnerProof;
use crate::rendezvous::core::PreparedDestinationTopologyAck;

impl ReservedTopologyBeginPublication {
    #[inline]
    pub(super) fn new(
        ack: TopologyAck,
        owner: RendezvousOwnerProof,
        distributed: PreparedDistributedTopologyBegin,
    ) -> Self {
        Self {
            ack,
            owner,
            distributed,
        }
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) fn into_parts(
        self,
    ) -> (
        TopologyAck,
        RendezvousOwnerProof,
        PreparedDistributedTopologyBegin,
    ) {
        (self.ack, self.owner, self.distributed)
    }
}

impl ReservedTopologyAckPublication {
    #[inline]
    pub(super) fn new(
        destination: PreparedDestinationTopologyAck,
        owner: RendezvousOwnerProof,
        distributed: PreparedDistributedTopologyAck,
    ) -> Self {
        Self {
            destination,
            owner,
            distributed,
        }
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) fn into_parts(
        self,
    ) -> (
        PreparedDestinationTopologyAck,
        RendezvousOwnerProof,
        PreparedDistributedTopologyAck,
    ) {
        (self.destination, self.owner, self.distributed)
    }
}

impl ReservedTopologyCommitPublication {
    #[inline]
    pub(super) fn new(
        ack: TopologyAck,
        src_owner: RendezvousOwnerProof,
        dst_owner: RendezvousOwnerProof,
        source: ReservedSourceTopologyCommitProof,
        destination: ReservedDestinationTopologyCommitProof,
        distributed: PreparedDistributedTopologyCommit,
    ) -> Self {
        Self {
            meta: ReservedTopologyCommitMeta::from_ack(ack, src_owner, dst_owner),
            source,
            destination,
            distributed,
        }
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) fn into_proofs(
        self,
    ) -> (
        ReservedTopologyCommitMeta,
        ReservedSourceTopologyCommitProof,
        ReservedDestinationTopologyCommitProof,
        PreparedDistributedTopologyCommit,
    ) {
        (self.meta, self.source, self.destination, self.distributed)
    }
}

impl ReservedTopologyCommitMeta {
    #[inline]
    pub(super) const fn from_ack(
        ack: TopologyAck,
        src_owner: RendezvousOwnerProof,
        dst_owner: RendezvousOwnerProof,
    ) -> Self {
        Self {
            src_owner,
            dst_owner,
            src_lane: ack.src_lane,
            dst_lane: ack.new_lane,
        }
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) const fn src_owner(
        self,
    ) -> RendezvousOwnerProof {
        self.src_owner
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) const fn dst_owner(
        self,
    ) -> RendezvousOwnerProof {
        self.dst_owner
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) const fn src_lane(
        self,
    ) -> Lane {
        self.src_lane
    }

    #[inline]
    pub(in crate::control::cluster::core::descriptor_controls::prepared_send) const fn dst_lane(
        self,
    ) -> Lane {
        self.dst_lane
    }
}
