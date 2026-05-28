use core::mem::ManuallyDrop;

mod lane_effect;
mod publisher;
mod topology;
pub(crate) use publisher::DescriptorTerminalPublisher;

use crate::control::cluster::core::{
    Generation, Lane, PreparedDistributedTopologyAck, PreparedDistributedTopologyBegin,
    PreparedDistributedTopologyCommit, SessionId, TopologyAck,
};
use crate::control::lease::core::RendezvousOwnerProof;
use crate::rendezvous::core::{
    ReservedDestinationTopologyCommitProof, ReservedSourceTopologyCommitProof,
};

/// Compact send-side descriptor terminal carrier.
///
/// Reserved topology terminals carry state-owner proof. Descriptor-effect
/// terminals are endpoint-scoped and terminate fail-closed through the owning
/// rendezvous lane; it is intentionally not presented as a reserved topology
/// publication proof.
pub(crate) struct DescriptorTerminal {
    case: ManuallyDrop<DescriptorTerminalCase>,
}

pub(super) enum DescriptorTerminalCase {
    None,
    ReservedTopology(ReservedTopologyTerminal),
    DescriptorEffectTerminal(DescriptorEffectTerminal),
}

pub(super) enum ReservedTopologyTerminal {
    Begin(ReservedTopologyBeginPublication),
    Ack(ReservedTopologyAckPublication),
    Commit(ReservedTopologyCommitPublication),
}

pub(super) struct ReservedTopologyBeginPublication {
    ack: TopologyAck,
    owner: RendezvousOwnerProof,
    distributed: PreparedDistributedTopologyBegin,
}

pub(super) struct ReservedTopologyAckPublication {
    ack: TopologyAck,
    owner: RendezvousOwnerProof,
    distributed: PreparedDistributedTopologyAck,
}

pub(super) struct ReservedTopologyCommitPublication {
    meta: ReservedTopologyCommitMeta,
    source: ReservedSourceTopologyCommitProof,
    destination: ReservedDestinationTopologyCommitProof,
    distributed: PreparedDistributedTopologyCommit,
}

#[derive(Clone, Copy)]
pub(super) struct ReservedTopologyCommitMeta {
    src_owner: RendezvousOwnerProof,
    dst_owner: RendezvousOwnerProof,
    sid: SessionId,
    generation: Generation,
    src_lane: Lane,
    dst_lane: Lane,
}

pub(super) struct DescriptorEffectTerminal {
    effect: DescriptorEffect,
    owner: RendezvousOwnerProof,
    sid: SessionId,
    lane: Lane,
    generation: Generation,
}

#[derive(Clone, Copy)]
pub(super) enum DescriptorEffect {
    AbortBegin,
    AbortAck,
    StateSnapshot,
    StateRestore,
    TxCommit,
    TxAbort,
}

impl DescriptorTerminal {
    #[inline]
    pub(crate) const fn none() -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::None),
        }
    }

    #[inline]
    pub(crate) fn is_none(&self) -> bool {
        matches!(&*self.case, DescriptorTerminalCase::None)
    }

    #[inline]
    pub(super) fn into_case(self) -> DescriptorTerminalCase {
        let mut this = ManuallyDrop::new(self);
        unsafe {
            // SAFETY: `this` will not run `Drop`; ownership of the inner affine
            // terminal case is transferred exactly once to the caller.
            ManuallyDrop::take(&mut this.case)
        }
    }

    #[inline]
    pub(super) fn topology_begin(
        ack: TopologyAck,
        owner: RendezvousOwnerProof,
        distributed: PreparedDistributedTopologyBegin,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::ReservedTopology(
                ReservedTopologyTerminal::Begin(ReservedTopologyBeginPublication::new(
                    ack,
                    owner,
                    distributed,
                )),
            )),
        }
    }

    #[inline]
    pub(super) fn topology_ack(
        ack: TopologyAck,
        owner: RendezvousOwnerProof,
        distributed: PreparedDistributedTopologyAck,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::ReservedTopology(
                ReservedTopologyTerminal::Ack(ReservedTopologyAckPublication::new(
                    ack,
                    owner,
                    distributed,
                )),
            )),
        }
    }

    #[inline]
    pub(super) fn commit_topology(
        ack: TopologyAck,
        src_owner: RendezvousOwnerProof,
        dst_owner: RendezvousOwnerProof,
        source: ReservedSourceTopologyCommitProof,
        destination: ReservedDestinationTopologyCommitProof,
        distributed: PreparedDistributedTopologyCommit,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::ReservedTopology(
                ReservedTopologyTerminal::Commit(ReservedTopologyCommitPublication::new(
                    ack,
                    src_owner,
                    dst_owner,
                    source,
                    destination,
                    distributed,
                )),
            )),
        }
    }

    #[inline]
    pub(super) const fn abort_begin(
        owner: RendezvousOwnerProof,
        sid: SessionId,
        lane: Lane,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::new(
                    DescriptorEffect::AbortBegin,
                    owner,
                    sid,
                    lane,
                    Generation::ZERO,
                ),
            )),
        }
    }

    #[inline]
    pub(super) const fn abort_ack(
        owner: RendezvousOwnerProof,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::new(
                    DescriptorEffect::AbortAck,
                    owner,
                    sid,
                    lane,
                    generation,
                ),
            )),
        }
    }

    #[inline]
    pub(super) const fn state_snapshot(
        owner: RendezvousOwnerProof,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::new(
                    DescriptorEffect::StateSnapshot,
                    owner,
                    sid,
                    lane,
                    generation,
                ),
            )),
        }
    }

    #[inline]
    pub(super) const fn state_restore(
        owner: RendezvousOwnerProof,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::new(
                    DescriptorEffect::StateRestore,
                    owner,
                    sid,
                    lane,
                    generation,
                ),
            )),
        }
    }

    #[inline]
    pub(super) const fn tx_commit(
        owner: RendezvousOwnerProof,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::new(
                    DescriptorEffect::TxCommit,
                    owner,
                    sid,
                    lane,
                    generation,
                ),
            )),
        }
    }

    #[inline]
    pub(super) const fn tx_abort(
        owner: RendezvousOwnerProof,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::new(
                    DescriptorEffect::TxAbort,
                    owner,
                    sid,
                    lane,
                    generation,
                ),
            )),
        }
    }
}

impl Drop for DescriptorTerminal {
    fn drop(&mut self) {
        assert!(self.is_none());
        unsafe {
            // SAFETY: dropping the empty case is the only implicit terminal path.
            // Non-empty terminal tickets must be consumed by publish/rollback.
            ManuallyDrop::drop(&mut self.case);
        }
    }
}
