//! Distributed splice coordination using control::Txn.
//!
//! This module implements the distributed splice lifecycle:
//! 1. intent (SpliceBegin) - Source RV generates intent
//! 2. ack (SpliceAck) - Destination RV acknowledges
//! 3. commit (SpliceCommit) - Source RV commits the splice
//!
//! The lifecycle maps directly to control::Txn's typestate transitions.

use crate::control::automaton::txn::{Closed, InAcked, InBegin, Tap, Txn};
use crate::control::cluster::error::SpliceError;
use crate::control::types::{
    AtMostOnceCommit, Gen, IncreasingGen, LaneId, NoCrossLaneAliasing, One,
};

/// Invariant marker for distributed splice transactions.
pub struct DistributedSpliceInv;

impl NoCrossLaneAliasing for DistributedSpliceInv {}
impl AtMostOnceCommit for DistributedSpliceInv {}

/// Distributed splice intent message.
///
/// This message is sent from source RV to destination RV to initiate a splice.
/// This is the canonical type used by both control::automaton::distributed and ra.rs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpliceIntent {
    /// Source Rendezvous ID
    pub src_rv: crate::control::types::RendezvousId,

    /// Destination Rendezvous ID
    pub dst_rv: crate::control::types::RendezvousId,

    /// Session ID (for tracking)
    pub sid: u32,

    /// Old generation (before splice)
    pub old_gen: Gen,

    /// New generation (after splice)
    pub new_gen: Gen,

    /// Sequence number for TX fence (optional, 0 if not used)
    pub seq_tx: u32,

    /// Sequence number for RX fence (optional, 0 if not used)
    pub seq_rx: u32,

    /// Source lane ID
    pub src_lane: LaneId,

    /// Destination lane ID
    pub dst_lane: LaneId,
}

impl SpliceIntent {
    /// Create a new splice intent.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        src_rv: crate::control::types::RendezvousId,
        dst_rv: crate::control::types::RendezvousId,
        sid: u32,
        old_gen: Gen,
        new_gen: Gen,
        seq_tx: u32,
        seq_rx: u32,
        src_lane: LaneId,
        dst_lane: LaneId,
    ) -> Self {
        Self {
            src_rv,
            dst_rv,
            sid,
            old_gen,
            new_gen,
            seq_tx,
            seq_rx,
            src_lane,
            dst_lane,
        }
    }
}

/// Distributed splice acknowledgment message.
///
/// This message is sent from destination RV back to source RV after validation.
/// Compatible with ra.rs SpliceAck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpliceAck {
    /// Source Rendezvous ID
    pub src_rv: crate::control::types::RendezvousId,

    /// Destination Rendezvous ID
    pub dst_rv: crate::control::types::RendezvousId,

    /// Session ID
    pub sid: u32,

    /// New generation
    pub new_gen: Gen,

    /// New lane
    pub new_lane: LaneId,

    /// Sequence number for TX
    pub seq_tx: u32,

    /// Sequence number for RX
    pub seq_rx: u32,
}

impl SpliceAck {
    /// Create a new acknowledgment.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        src_rv: crate::control::types::RendezvousId,
        dst_rv: crate::control::types::RendezvousId,
        sid: u32,
        new_gen: Gen,
        new_lane: LaneId,
        seq_tx: u32,
        seq_rx: u32,
    ) -> Self {
        Self {
            src_rv,
            dst_rv,
            sid,
            new_gen,
            new_lane,
            seq_tx,
            seq_rx,
        }
    }

    /// Create acknowledgment from intent.
    pub fn from_intent(intent: &SpliceIntent) -> Self {
        Self {
            src_rv: intent.src_rv,
            dst_rv: intent.dst_rv,
            sid: intent.sid,
            new_gen: intent.new_gen,
            new_lane: intent.dst_lane,
            seq_tx: intent.seq_tx,
            seq_rx: intent.seq_rx,
        }
    }
}

/// Distributed splice coordinator.
///
/// This coordinates the distributed splice lifecycle using control::Txn.
pub struct DistributedSplice;

impl DistributedSplice {
    /// Begin a distributed splice intent.
    ///
    /// Returns a transaction in InBegin state and the SpliceIntent message
    /// to send to the destination RV.
    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        src_rv: crate::control::types::RendezvousId,
        dst_rv: crate::control::types::RendezvousId,
        sid: u32,
        old_gen: Gen,
        new_gen: Gen,
        seq_tx: u32,
        seq_rx: u32,
        src_lane: LaneId,
        dst_lane: LaneId,
        tap: &mut impl Tap,
    ) -> (InBegin<DistributedSpliceInv, One>, SpliceIntent) {
        // Create transaction
        let txn: Txn<DistributedSpliceInv, IncreasingGen, One> =
            unsafe { Txn::new(src_lane, old_gen) };

        // Begin the splice (emits SpliceBegin effect)
        let in_begin = txn.begin(tap);

        // Create intent message
        let intent = SpliceIntent::new(
            src_rv, dst_rv, sid, old_gen, new_gen, seq_tx, seq_rx, src_lane, dst_lane,
        );

        (in_begin, intent)
    }

    /// Process a splice intent at the destination RV.
    ///
    /// Validates the intent and generates an acknowledgment.
    pub fn process_intent(
        intent: &SpliceIntent,
        _tap: &mut impl Tap,
    ) -> Result<SpliceAck, SpliceError> {
        // Validate generation ordering
        if intent.new_gen.raw() <= intent.old_gen.raw() {
            return Err(SpliceError::GenerationMismatch);
        }

        Ok(SpliceAck::from_intent(intent))
    }

    /// Acknowledge a splice intent.
    ///
    /// Transitions the transaction from InBegin to InAcked state.
    pub fn acknowledge(
        in_begin: InBegin<DistributedSpliceInv, One>,
        tap: &mut impl Tap,
    ) -> InAcked<DistributedSpliceInv, One> {
        // Transition to acked state (emits SpliceAck effect)
        in_begin.ack(tap)
    }

    /// Commit the splice.
    ///
    /// Transitions the transaction to Closed state and bumps generation.
    pub fn commit(
        in_acked: InAcked<DistributedSpliceInv, One>,
        tap: &mut impl Tap,
    ) -> Closed<DistributedSpliceInv> {
        // Commit (emits SpliceCommit effect and bumps generation)
        in_acked.commit(tap)
    }

    /// Abort the splice.
    ///
    /// Transitions the transaction to Closed state without bumping generation.
    pub fn abort(
        in_acked: InAcked<DistributedSpliceInv, One>,
        tap: &mut impl Tap,
    ) -> Closed<DistributedSpliceInv> {
        // Abort (emits Abort effect, no generation bump)
        in_acked.abort(tap)
    }
}

#[cfg(test)]
mod tests {
    use super::super::txn::NoopTap;
    use super::*;
    use crate::control::types::RendezvousId;

    #[test]
    fn test_distributed_splice_happy_path() {
        let mut tap = NoopTap;

        let (in_begin, intent) = DistributedSplice::begin(
            RendezvousId::new(1), // src_rv
            RendezvousId::new(2), // dst_rv
            42,                   // sid
            Gen::new(10),         // old_gen
            Gen::new(11),         // new_gen
            0,                    // seq_tx
            0,                    // seq_rx
            LaneId::new(1),       // src_lane
            LaneId::new(2),       // dst_lane
            &mut tap,
        );

        assert_eq!(intent.sid, 42);
        assert_eq!(intent.src_lane, LaneId::new(1));
        assert_eq!(intent.dst_lane, LaneId::new(2));
        assert_eq!(intent.old_gen, Gen::new(10));
        assert_eq!(intent.new_gen, Gen::new(11));

        let ack = DistributedSplice::process_intent(&intent, &mut tap).unwrap();
        assert_eq!(ack.new_gen, Gen::new(11));

        let in_acked = DistributedSplice::acknowledge(in_begin, &mut tap);

        let closed = DistributedSplice::commit(in_acked, &mut tap);

        // Verify generation was bumped
        assert_eq!(closed.generation(), Gen::new(11));
    }

    #[test]
    fn test_distributed_splice_failure() {
        let mut tap = NoopTap;

        // Begin with invalid generation (new_gen <= old_gen)
        let (_in_begin, intent) = DistributedSplice::begin(
            RendezvousId::new(1),
            RendezvousId::new(2),
            42,           // sid
            Gen::new(10), // old_gen
            Gen::new(10), // new_gen (same as old_gen - invalid!)
            0,            // seq_tx
            0,            // seq_rx
            LaneId::new(1),
            LaneId::new(2),
            &mut tap,
        );

        // Process should fail due to invalid generation
        let result = DistributedSplice::process_intent(&intent, &mut tap);
        assert!(result.is_err());
    }

    #[test]
    fn test_distributed_splice_abort() {
        let mut tap = NoopTap;

        // Begin
        let (in_begin, _intent) = DistributedSplice::begin(
            RendezvousId::new(1),
            RendezvousId::new(2),
            42,           // sid
            Gen::new(10), // old_gen
            Gen::new(11), // new_gen
            0,            // seq_tx
            0,            // seq_rx
            LaneId::new(1),
            LaneId::new(2),
            &mut tap,
        );

        // Acknowledge
        let in_acked = DistributedSplice::acknowledge(in_begin, &mut tap);

        // Abort instead of commit
        let closed = DistributedSplice::abort(in_acked, &mut tap);

        // Verify generation was NOT bumped (stays at old_gen)
        assert_eq!(closed.generation(), Gen::new(10));
    }
}
