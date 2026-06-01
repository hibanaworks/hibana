//! Control-plane transaction typestate protocol.
//!
//! This module provides by-value typestate transitions for control owners that
//! have already validated:
//! - no cross-lane aliasing for the staged operation
//! - at-most-once terminal ownership
//! - the expected generation transition
//! - single-use shot discipline for the transaction witness
//!
//! The typestate protocol keeps the Begin -> Ack -> terminal order in the type
//! API; the owner that creates `Txn` remains responsible for the unsafe facts
//! named by the marker traits.

use crate::control::cap::mint::ControlOp;
use crate::control::types::{
    AtMostOnceCommit, Generation, IncreasingGen, Lane, NoCrossLaneAliasing, One,
};
use core::marker::PhantomData;

/// Trait for emitting atomic control operations.
///
/// This is typically implemented by the tap/observe infrastructure.
pub(crate) trait Tap {
    /// Emit a control-plane operation.
    fn emit(&mut self, op: ControlOp);
}

/// No-op tap for typestate transitions whose observable event is emitted by the caller.
pub(crate) struct NoopTap;

impl Tap for NoopTap {
    fn emit(&mut self, _op: ControlOp) {}
}

/// Transaction handle with typestate-based invariants.
///
/// The type parameters encode the invariants that must hold:
/// - `Inv`: Invariant marker (e.g., `NoCrossLaneAliasing + AtMostOnceCommit`)
/// - `GenOrd`: Generation ordering marker (e.g., `IncreasingGen`)
/// - `Shot`: Shot discipline marker
pub(crate) struct Txn<Inv, GenOrd, Shot> {
    _p: PhantomData<(Inv, GenOrd, Shot)>,
}

impl<Inv, GenOrd, Shot> Txn<Inv, GenOrd, Shot> {
    /// Create a new transaction handle.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the facts encoded in the type parameters
    /// are actually satisfied. Rendezvous owners validate those facts before
    /// minting transaction witnesses.
    pub(crate) unsafe fn new(_lane: Lane, _generation: Generation) -> Self {
        Self { _p: PhantomData }
    }
}

impl<Inv: AtMostOnceCommit + NoCrossLaneAliasing, S> Txn<Inv, IncreasingGen, S> {
    /// Begin a topology operation.
    ///
    /// Emits `ControlOp::TopologyBegin` and transitions to `InBegin` state.
    pub(crate) fn begin(self, tap: &mut impl Tap) -> InBegin<Inv, S> {
        tap.emit(ControlOp::TopologyBegin);
        InBegin { _p: PhantomData }
    }
}

/// Transaction in "begin" state (after `begin()`, before `ack()`).
pub(crate) struct InBegin<Inv, Shot> {
    _p: PhantomData<(Inv, Shot)>,
}

impl<Inv, S> InBegin<Inv, S> {
    /// Acknowledge the topology operation.
    ///
    /// Emits `ControlOp::TopologyAck` and transitions to `InAcked` state.
    pub(crate) fn ack(self, tap: &mut impl Tap) -> InAcked<Inv, S> {
        tap.emit(ControlOp::TopologyAck);
        InAcked { _p: PhantomData }
    }
}

/// Transaction in "acked" state (after `ack()`, before `commit()` or `abort()`).
pub(crate) struct InAcked<Inv, Shot> {
    _p: PhantomData<(Inv, Shot)>,
}

impl<Inv: AtMostOnceCommit> InAcked<Inv, One> {
    /// Commit the transaction.
    ///
    /// Emits `ControlOp::TopologyCommit` and transitions to `Closed` state.
    /// The generation number is bumped.
    pub(crate) fn commit(self, tap: &mut impl Tap) -> Closed<Inv> {
        tap.emit(ControlOp::TopologyCommit);
        Closed { _p: PhantomData }
    }
}

/// Transaction in "closed" state (terminal state).
pub(crate) struct Closed<Inv> {
    _p: PhantomData<Inv>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Concrete invariant type for transaction validation.
    struct TestInv;
    impl NoCrossLaneAliasing for TestInv {}
    impl AtMostOnceCommit for TestInv {}

    struct RecordingTap {
        ops: [Option<ControlOp>; 3],
        len: usize,
    }

    impl RecordingTap {
        fn new() -> Self {
            Self {
                ops: [None, None, None],
                len: 0,
            }
        }

        fn as_slice(&self) -> &[Option<ControlOp>] {
            &self.ops[..self.len]
        }
    }

    impl Tap for RecordingTap {
        fn emit(&mut self, op: ControlOp) {
            self.ops[self.len] = Some(op);
            self.len += 1;
        }
    }

    #[test]
    fn test_txn_happy_path() {
        let mut tap = RecordingTap::new();

        // Create a transaction
        let txn: Txn<TestInv, IncreasingGen, crate::control::types::One> =
            /* SAFETY: the topology owner has validated the lane/generation transition before minting this typestate transaction witness. */ unsafe { Txn::new(Lane::new(42), Generation::new(10)) };

        // Begin -> Ack -> Commit
        let in_begin = txn.begin(&mut tap);
        let in_acked = in_begin.ack(&mut tap);
        in_acked.commit(&mut tap);

        assert_eq!(
            tap.as_slice(),
            &[
                Some(ControlOp::TopologyBegin),
                Some(ControlOp::TopologyAck),
                Some(ControlOp::TopologyCommit),
            ]
        );
    }
}
