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
    PreparedAbortAckEffect, PreparedAbortBeginEffect, PreparedStateRestoreEffect,
    PreparedStateSnapshotEffect, PreparedTxAbortEffect, PreparedTxCommitEffect,
    ReservedDestinationTopologyCommitProof, ReservedSourceTopologyCommitProof,
};

/// Compact send-side descriptor terminal carrier.
///
/// Reserved topology and descriptor-effect terminals carry state-owner proof.
/// Post-transport publication consumes prepared local rendezvous effects; it
/// does not re-enter fallible validation.
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

pub(super) enum DescriptorEffectTerminal {
    AbortBegin(PreparedDescriptorEffect<PreparedAbortBeginEffect>),
    AbortAck(PreparedDescriptorEffect<PreparedAbortAckEffect>),
    StateSnapshot(PreparedDescriptorEffect<PreparedStateSnapshotEffect>),
    StateRestore(PreparedDescriptorEffect<PreparedStateRestoreEffect>),
    TxCommit(PreparedDescriptorEffect<PreparedTxCommitEffect>),
    TxAbort(PreparedDescriptorEffect<PreparedTxAbortEffect>),
}

pub(super) struct PreparedDescriptorEffect<Proof> {
    owner: RendezvousOwnerProof,
    proof: Proof,
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
        proof: PreparedAbortBeginEffect,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::AbortBegin(PreparedDescriptorEffect::new(owner, proof)),
            )),
        }
    }

    #[inline]
    pub(super) const fn abort_ack(
        owner: RendezvousOwnerProof,
        proof: PreparedAbortAckEffect,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::AbortAck(PreparedDescriptorEffect::new(owner, proof)),
            )),
        }
    }

    #[inline]
    pub(super) const fn state_snapshot(
        owner: RendezvousOwnerProof,
        proof: PreparedStateSnapshotEffect,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::StateSnapshot(PreparedDescriptorEffect::new(
                    owner, proof,
                )),
            )),
        }
    }

    #[inline]
    pub(super) const fn state_restore(
        owner: RendezvousOwnerProof,
        proof: PreparedStateRestoreEffect,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::StateRestore(PreparedDescriptorEffect::new(owner, proof)),
            )),
        }
    }

    #[inline]
    pub(super) const fn tx_commit(
        owner: RendezvousOwnerProof,
        proof: PreparedTxCommitEffect,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::TxCommit(PreparedDescriptorEffect::new(owner, proof)),
            )),
        }
    }

    #[inline]
    pub(super) const fn tx_abort(
        owner: RendezvousOwnerProof,
        proof: PreparedTxAbortEffect,
    ) -> Self {
        Self {
            case: ManuallyDrop::new(DescriptorTerminalCase::DescriptorEffectTerminal(
                DescriptorEffectTerminal::TxAbort(PreparedDescriptorEffect::new(owner, proof)),
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
