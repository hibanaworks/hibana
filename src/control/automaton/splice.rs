//! SpliceAutomaton — lease-first distributed splice.
//!
//! This module provides ControlAutomaton implementations for source-side splice operations.
//! It replaces the procedural DistributedSplice::begin/commit with typed automatons
//! that use RendezvousLease to access SpliceFacet.
//!
//! ## Design
//!
//! - **Source-only**: These automatons handle source RV operations only
//! - **Destination handling**: Destination RV ack is handled by existing Rendezvous::process_splice_intent
//! - **No LeaseGraph**: Single source RV doesn't need ownership tracking
//! - **Lease-first**: Uses ControlAutomaton pattern with RendezvousLease
//!
//! ## Lifecycle
//!
//! 1. **Begin** — Source RV calls `SpliceFacet::begin`, generates SpliceIntent
//! 2. **Wait Ack** — (External: destination processes intent via Rendezvous::process_splice_intent)
//! 3. **Commit** — Source RV calls `SpliceFacet::commit` with received SpliceAck

use core::marker::PhantomData;

use crate::{
    control::automaton::distributed::{SpliceAck, SpliceIntent},
    control::cluster::error::SpliceError,
    control::{
        cluster::core::{EffectRunner, SpliceOperands},
        lease::{
            bundle::LeaseBundleFacet,
            core::{ControlAutomaton, ControlStep, FullSpec, RendezvousLease, SpliceSpec},
            graph::{InlineLeaseChildStorage, InlineLeaseNodeStorage, LeaseSpec},
        },
        types::{Generation, Lane, RendezvousId, SessionId},
    },
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

#[derive(Debug, Default)]
pub(crate) struct SpliceGraphContext {
    pub(crate) last_intent: Option<SpliceIntent>,
}

impl SpliceGraphContext {
    pub(crate) fn new(last_intent: Option<SpliceIntent>) -> Self {
        Self { last_intent }
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        self.last_intent = None;
    }
}

/// Maximum node capacity for [`SpliceLeaseSpec`].
pub(crate) const SPLICE_LEASE_MAX_NODES: usize = 3;
/// Maximum child capacity for [`SpliceLeaseSpec`].
pub(crate) const SPLICE_LEASE_MAX_CHILDREN: usize = 2;

/// LeaseGraph specification for splice orchestration.
pub(crate) struct SpliceLeaseSpec<T, U, C, E>(PhantomData<(T, U, C, E)>);

impl<T, U, C, E> LeaseSpec for SpliceLeaseSpec<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type NodeId = RendezvousId;
    type Facet = LeaseBundleFacet<T, U, C, E>;
    type ChildStorage = InlineLeaseChildStorage<RendezvousId, SPLICE_LEASE_MAX_CHILDREN>;
    type NodeStorage<'graph>
        = InlineLeaseNodeStorage<'graph, Self, SPLICE_LEASE_MAX_NODES>
    where
        Self: 'graph;
    const MAX_NODES: usize = SPLICE_LEASE_MAX_NODES;
    const MAX_CHILDREN: usize = SPLICE_LEASE_MAX_CHILDREN;
}

/// Seed used for splice operand preparation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SplicePrepareSeed {
    pub(crate) sid: SessionId,
    pub(crate) src_lane: Lane,
    pub(crate) dst_rv: RendezvousId,
    pub(crate) dst_lane: Lane,
    pub(crate) fences: Option<(u32, u32)>,
}

/// Automaton that prepares splice operands through a lease graph.
pub(crate) struct SplicePrepareAutomaton;

impl<T, U, C, E> ControlAutomaton<T, U, C, E> for SplicePrepareAutomaton
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type Spec = FullSpec;
    type Seed = SplicePrepareSeed;
    type Output = SpliceOperands;
    type Error = crate::control::cluster::error::CpError;
    type GraphSpec = SpliceLeaseSpec<T, U, C, E>;

    fn run<'lease, 'lease_cfg>(
        _lease: &mut RendezvousLease<'lease, 'lease_cfg, T, U, C, E, Self::Spec>,
        _seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'lease_cfg: 'lease,
    {
        ControlStep::Abort(crate::control::cluster::error::CpError::Splice(
            SpliceError::InvalidState,
        ))
    }

    fn run_with_graph<'lease, 'lease_cfg, 'graph>(
        graph: &'graph mut crate::control::lease::graph::LeaseGraph<
            'graph,
            SpliceLeaseSpec<T, U, C, E>,
        >,
        root_lease: &mut RendezvousLease<'lease, 'lease_cfg, T, U, C, E, Self::Spec>,
        seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'lease_cfg: 'lease,
    {
        match root_lease.with_rendezvous(|rv| {
            EffectRunner::prepare_splice_operands(
                rv,
                seed.sid,
                seed.src_lane,
                seed.dst_rv,
                seed.dst_lane,
                seed.fences,
            )
        }) {
            Ok(operands) => {
                {
                    let mut handle = graph.root_handle_mut();
                    if let Some(splice) = handle.context().splice() {
                        splice.clear();
                    }
                }
                ControlStep::Complete(operands)
            }
            Err(err) => ControlStep::Abort(err),
        }
    }
}

/// Begin automaton for distributed splice.
///
/// This automaton receives a SpliceIntent, validates it, and calls
/// SpliceFacet::begin on the source rendezvous. On success, it returns
/// the SpliceIntent to be sent to the destination.
///
/// ## Usage
///
/// ```ignore
/// let intent = SpliceIntent::new(src_rv, dst_rv, sid, old_gen, new_gen, ...);
/// let mut lease = core.lease::<SpliceSpec>(src_rv)?;
/// let result = SpliceBeginAutomaton::run(&mut lease, intent);
/// match result {
///     ControlStep::Complete(intent_msg) => {
///         // Send intent_msg to destination
///     }
///     ControlStep::Abort(err) => {
///         // Handle error
///     }
/// }
/// ```
pub(crate) struct SpliceBeginAutomaton;

impl<T, U, C, E> ControlAutomaton<T, U, C, E> for SpliceBeginAutomaton
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type Spec = SpliceSpec;
    type Seed = SpliceIntent;
    type Output = SpliceIntent;
    type Error = SpliceError;
    type GraphSpec = SpliceLeaseSpec<T, U, C, E>;

    fn run<'lease, 'cfg>(
        lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        intent: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease,
    {
        // Validate generation ordering
        if intent.new_gen.raw() <= intent.old_gen.raw() {
            return ControlStep::Abort(SpliceError::GenerationMismatch);
        }

        // Convert intent parameters to rendezvous types
        let sid = SessionId(intent.sid);
        let lane = Lane::new(intent.src_lane.raw());
        let fences = if intent.seq_tx != 0 || intent.seq_rx != 0 {
            Some((intent.seq_tx, intent.seq_rx))
        } else {
            None
        };
        let generation = Generation::new(intent.new_gen.raw());

        // Begin splice at source
        match lease.with_rendezvous(|rv| {
            let facet = rv.splice_facet();
            facet.begin(rv, sid, lane, fences, generation)
        }) {
            Ok(()) => ControlStep::Complete(intent),
            Err(ra_err) => ControlStep::Abort(ra_err.into()),
        }
    }

    fn run_with_graph<'lease, 'cfg, 'graph>(
        graph: &'graph mut crate::control::lease::graph::LeaseGraph<
            'graph,
            SpliceLeaseSpec<T, U, C, E>,
        >,
        root_lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        intent: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease,
    {
        let _ = graph;

        match <Self as ControlAutomaton<T, U, C, E>>::run(root_lease, intent) {
            ControlStep::Complete(intent) => {
                {
                    let mut handle = graph.root_handle_mut();
                    if let Some(splice) = handle.context().splice() {
                        splice.last_intent = Some(intent);
                    }
                }
                ControlStep::Complete(intent)
            }
            ControlStep::Abort(err) => {
                {
                    let mut handle = graph.root_handle_mut();
                    if let Some(splice) = handle.context().splice() {
                        splice.clear();
                    }
                }
                ControlStep::Abort(err)
            }
        }
    }
}

/// Commit automaton for distributed splice.
///
/// This automaton receives a SpliceAck (from destination), validates it,
/// and calls SpliceFacet::commit on the source rendezvous.
///
/// ## Usage
///
/// ```ignore
/// // After receiving SpliceAck from destination
/// let mut lease = core.lease::<SpliceSpec>(src_rv)?;
/// let result = SpliceCommitAutomaton::run(&mut lease, ack);
/// match result {
///     ControlStep::Complete(ack) => {
///         // Splice committed successfully
///     }
///     ControlStep::Abort(err) => {
///         // Handle error, may need rollback
///     }
/// }
/// ```
pub(crate) struct SpliceCommitAutomaton;

impl<T, U, C, E> ControlAutomaton<T, U, C, E> for SpliceCommitAutomaton
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type Spec = SpliceSpec;
    type Seed = SpliceAck;
    type Output = SpliceAck;
    type Error = SpliceError;
    type GraphSpec = SpliceLeaseSpec<T, U, C, E>;

    fn run<'lease, 'cfg>(
        lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        ack: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease,
    {
        // Convert ack parameters to rendezvous types
        let sid = SessionId(ack.sid);
        let new_lane = Lane::new(ack.new_lane.raw());

        // Commit splice at source
        // Note: In distributed splice, we commit on the source lane, then
        // the new_lane becomes active. The commit operation handles this internally.
        match lease.with_rendezvous(|rv| {
            let facet = rv.splice_facet();
            facet.commit(rv, sid, new_lane)
        }) {
            Ok(()) => ControlStep::Complete(ack),
            Err(ra_err) => ControlStep::Abort(ra_err.into()),
        }
    }

    fn run_with_graph<'lease, 'cfg, 'graph>(
        graph: &'graph mut crate::control::lease::graph::LeaseGraph<
            'graph,
            SpliceLeaseSpec<T, U, C, E>,
        >,
        root_lease: &mut RendezvousLease<'lease, 'cfg, T, U, C, E, Self::Spec>,
        ack: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'cfg: 'lease,
    {
        let root_id = graph.root_id();
        if ack.src_rv != root_id {
            return ControlStep::Abort(SpliceError::RendezvousIdMismatch);
        }

        let intent = {
            let mut handle = graph.root_handle_mut();
            let result = match handle.context().splice() {
                Some(splice) => splice.last_intent.ok_or(SpliceError::NoPending),
                None => Err(SpliceError::NoPending),
            };
            let intent = match result {
                Ok(intent) => intent,
                Err(err) => return ControlStep::Abort(err),
            };

            if ack.sid != intent.sid {
                return ControlStep::Abort(SpliceError::InvalidSession);
            }
            if ack.dst_rv != intent.dst_rv {
                return ControlStep::Abort(SpliceError::RendezvousIdMismatch);
            }
            if ack.new_gen != intent.new_gen {
                return ControlStep::Abort(SpliceError::GenerationMismatch);
            }
            if ack.new_lane != intent.dst_lane {
                return ControlStep::Abort(SpliceError::LaneMismatch);
            }
            if ack.seq_tx != intent.seq_tx || ack.seq_rx != intent.seq_rx {
                return ControlStep::Abort(SpliceError::SeqnoMismatch);
            }

            intent
        };

        let sid = SessionId(ack.sid);
        let lane = Lane::new(intent.src_lane.raw());

        match root_lease.with_rendezvous(|rv| {
            let facet = rv.splice_facet();
            facet
                .commit(rv, sid, lane)
                .map(|()| facet.release_lane(rv, lane))
        }) {
            Ok(()) => {
                {
                    let mut handle = graph.root_handle_mut();
                    if let Some(splice) = handle.context().splice() {
                        splice.clear();
                    }
                }
                ControlStep::Complete(ack)
            }
            Err(ra_err) => ControlStep::Abort(ra_err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::types::{Generation, Lane, RendezvousId};

    #[test]
    fn test_splice_intent_validation() {
        let intent = SpliceIntent::new(
            RendezvousId::new(1),
            RendezvousId::new(2),
            42,
            Generation::new(1),
            Generation::new(2),
            0,
            0,
            Lane::new(0),
            Lane::new(1),
        );

        // Validate generation ordering (should pass)
        assert!(intent.new_gen.raw() > intent.old_gen.raw());
    }

    #[test]
    fn test_splice_ack_from_intent() {
        let intent = SpliceIntent::new(
            RendezvousId::new(1),
            RendezvousId::new(2),
            42,
            Generation::new(1),
            Generation::new(2),
            100,
            200,
            Lane::new(0),
            Lane::new(1),
        );

        let ack = SpliceAck::from_intent(&intent);

        assert_eq!(ack.src_rv, intent.src_rv);
        assert_eq!(ack.dst_rv, intent.dst_rv);
        assert_eq!(ack.sid, intent.sid);
        assert_eq!(ack.new_gen, intent.new_gen);
        assert_eq!(ack.new_lane, intent.dst_lane);
        assert_eq!(ack.seq_tx, intent.seq_tx);
        assert_eq!(ack.seq_rx, intent.seq_rx);
    }

    // Note: Full integration tests require a complete rendezvous setup and live
    // in the integration test suite.
}
