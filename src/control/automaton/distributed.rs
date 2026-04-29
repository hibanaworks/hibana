//! Distributed topology coordination using control::Txn.
//!
//! This module implements the distributed topology lifecycle:
//! 1. intent (TopologyBegin) - Source RV generates intent
//! 2. ack (TopologyAck) - Destination RV acknowledges
//! 3. commit (TopologyCommit) - Source RV commits the topology transition
//!
//! The lifecycle maps directly to control::Txn's typestate transitions.

use crate::control::automaton::txn::{Closed, InAcked, InBegin, Tap, Txn};
use crate::control::types::{
    AtMostOnceCommit, Generation, IncreasingGen, Lane, NoCrossLaneAliasing, One,
};

/// Invariant marker for distributed topology transactions.
pub(crate) struct DistributedTopologyInv;

impl NoCrossLaneAliasing for DistributedTopologyInv {}
impl AtMostOnceCommit for DistributedTopologyInv {}

/// Distributed topology intent message.
///
/// This message is sent from source RV to destination RV to initiate a topology transition.
/// This is the canonical topology intent used by the control automaton.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TopologyIntent {
    /// Source Rendezvous ID
    pub(crate) src_rv: crate::control::types::RendezvousId,

    /// Destination Rendezvous ID
    pub(crate) dst_rv: crate::control::types::RendezvousId,

    /// Session ID (for tracking)
    pub(crate) sid: u32,

    /// Old generation (before topology transition)
    pub(crate) old_gen: Generation,

    /// New generation (after topology transition)
    pub(crate) new_gen: Generation,

    /// Sequence number for TX fence (optional, 0 if not used)
    pub(crate) seq_tx: u32,

    /// Sequence number for RX fence (optional, 0 if not used)
    pub(crate) seq_rx: u32,

    /// Source lane ID
    pub(crate) src_lane: Lane,

    /// Destination lane ID
    pub(crate) dst_lane: Lane,
}

/// Distributed topology acknowledgment message.
///
/// This message is sent from destination RV back to source RV after validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TopologyAck {
    /// Source Rendezvous ID
    pub(crate) src_rv: crate::control::types::RendezvousId,

    /// Destination Rendezvous ID
    pub(crate) dst_rv: crate::control::types::RendezvousId,

    /// Session ID
    pub(crate) sid: u32,

    /// New generation
    pub(crate) new_gen: Generation,

    /// Source lane to commit on the origin rendezvous.
    pub(crate) src_lane: Lane,

    /// New lane
    pub(crate) new_lane: Lane,

    /// Sequence number for TX
    pub(crate) seq_tx: u32,

    /// Sequence number for RX
    pub(crate) seq_rx: u32,
}

impl TopologyAck {
    /// Create acknowledgment from intent.
    pub(crate) fn from_intent(intent: &TopologyIntent) -> Self {
        Self {
            src_rv: intent.src_rv,
            dst_rv: intent.dst_rv,
            sid: intent.sid,
            new_gen: intent.new_gen,
            src_lane: intent.src_lane,
            new_lane: intent.dst_lane,
            seq_tx: intent.seq_tx,
            seq_rx: intent.seq_rx,
        }
    }
}

/// Distributed topology coordinator.
///
/// This coordinates the distributed topology lifecycle using control::Txn.
pub(crate) struct DistributedTopology;

impl DistributedTopology {
    /// Begin a distributed topology intent.
    ///
    /// Returns a transaction in InBegin state and the TopologyIntent message
    /// to send to the destination RV.
    pub(crate) fn begin(
        intent: TopologyIntent,
        tap: &mut impl Tap,
    ) -> (InBegin<DistributedTopologyInv, One>, TopologyIntent) {
        let txn: Txn<DistributedTopologyInv, IncreasingGen, One> =
            unsafe { Txn::new(intent.src_lane, intent.old_gen) };

        let in_begin = txn.begin(tap);
        (in_begin, intent)
    }

    /// Acknowledge a topology intent.
    ///
    /// Transitions the transaction from InBegin to InAcked state.
    pub(crate) fn acknowledge(
        in_begin: InBegin<DistributedTopologyInv, One>,
        tap: &mut impl Tap,
    ) -> InAcked<DistributedTopologyInv, One> {
        // Transition to acked state (emits TopologyAck effect)
        in_begin.ack(tap)
    }

    /// Commit the topology transition.
    ///
    /// Transitions the transaction to Closed state and bumps generation.
    pub(crate) fn topology_commit(
        in_acked: InAcked<DistributedTopologyInv, One>,
        tap: &mut impl Tap,
    ) -> Closed<DistributedTopologyInv> {
        // Commit (emits TopologyCommit effect and bumps generation)
        in_acked.commit(tap)
    }
}

#[cfg(test)]
mod tests {
    use super::super::txn::NoopTap;
    use super::*;
    use crate::control::types::RendezvousId;

    #[test]
    fn distributed_topology_typestate_begin_ack_commit_path_closes() {
        let mut tap = NoopTap;

        let (in_begin, intent) = DistributedTopology::begin(
            TopologyIntent {
                src_rv: RendezvousId::new(1),
                dst_rv: RendezvousId::new(2),
                sid: 42,
                old_gen: Generation::new(10),
                new_gen: Generation::new(11),
                seq_tx: 0,
                seq_rx: 0,
                src_lane: Lane::new(1),
                dst_lane: Lane::new(2),
            },
            &mut tap,
        );

        assert_eq!(intent.sid, 42);
        assert_eq!(intent.src_lane, Lane::new(1));
        assert_eq!(intent.dst_lane, Lane::new(2));
        assert_eq!(intent.old_gen, Generation::new(10));
        assert_eq!(intent.new_gen, Generation::new(11));

        let in_acked = DistributedTopology::acknowledge(in_begin, &mut tap);

        DistributedTopology::topology_commit(in_acked, &mut tap);
    }
}
