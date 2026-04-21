//! Control-plane transaction typestate machine.
//!
//! This module implements a typestate-based transaction protocol that enforces:
//! - No cross-lane aliasing (via `NoCrossLaneAliasing`)
//! - At-most-once commit (via `AtMostOnceCommit`)
//! - Strictly increasing generation (via `IncreasingGen`)
//! - Shot discipline (single-use `One` vs reusable `Many`)
//!
//! The typestate machine ensures that operations are performed in the correct order
//! and that invariants are maintained at compile time.

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

/// No-op tap for testing.
pub(crate) struct NoopTap;

impl Tap for NoopTap {
    fn emit(&mut self, _op: ControlOp) {}
}

/// Transaction handle with typestate-based invariants.
///
/// The type parameters encode the invariants that must hold:
/// - `Inv`: Invariant marker (e.g., `NoCrossLaneAliasing + AtMostOnceCommit`)
/// - `GenOrd`: Generation ordering marker (e.g., `IncreasingGen`)
/// - `Shot`: Shot discipline (e.g., `One` or `Many`)
pub(crate) struct Txn<Inv, GenOrd, Shot> {
    lane: Lane,
    #[cfg(test)]
    generation: Generation,
    _p: PhantomData<(Inv, GenOrd, Shot)>,
}

impl<Inv, GenOrd, Shot> Txn<Inv, GenOrd, Shot> {
    /// Create a new transaction handle.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the invariants encoded in the type parameters
    /// are actually satisfied. This is typically enforced by the rendezvous layer.
    pub(crate) unsafe fn new(lane: Lane, _generation: Generation) -> Self {
        Self {
            lane,
            #[cfg(test)]
            generation: _generation,
            _p: PhantomData,
        }
    }
}

impl<Inv: AtMostOnceCommit + NoCrossLaneAliasing, S> Txn<Inv, IncreasingGen, S> {
    /// Begin a splice operation.
    ///
    /// Emits `ControlOp::TopologyBegin` and transitions to `InBegin` state.
    pub(crate) fn begin(self, tap: &mut impl Tap) -> InBegin<Inv, S> {
        tap.emit(ControlOp::TopologyBegin);
        InBegin {
            lane: self.lane,
            #[cfg(test)]
            generation: self.generation,
            _p: PhantomData,
        }
    }
}

/// Transaction in "begin" state (after `begin()`, before `ack()`).
pub(crate) struct InBegin<Inv, Shot> {
    lane: Lane,
    #[cfg(test)]
    generation: Generation,
    _p: PhantomData<(Inv, Shot)>,
}

impl<Inv, S> InBegin<Inv, S> {
    /// Acknowledge the splice operation.
    ///
    /// Emits `ControlOp::TopologyAck` and transitions to `InAcked` state.
    pub(crate) fn ack(self, tap: &mut impl Tap) -> InAcked<Inv, S> {
        tap.emit(ControlOp::TopologyAck);
        InAcked {
            lane: self.lane,
            #[cfg(test)]
            generation: self.generation,
            _p: PhantomData,
        }
    }
}

/// Transaction in "acked" state (after `ack()`, before `commit()` or `abort()`).
pub(crate) struct InAcked<Inv, Shot> {
    lane: Lane,
    #[cfg(test)]
    generation: Generation,
    _p: PhantomData<(Inv, Shot)>,
}

impl<Inv: AtMostOnceCommit> InAcked<Inv, One> {
    /// Lane identifier associated with this transaction.
    pub(crate) fn lane(&self) -> Lane {
        self.lane
    }

    /// Commit the transaction.
    ///
    /// Emits `ControlOp::TopologyCommit` and transitions to `Closed` state.
    /// The generation number is bumped.
    pub(crate) fn commit(self, tap: &mut impl Tap) -> Closed<Inv> {
        tap.emit(ControlOp::TopologyCommit);
        Closed {
            #[cfg(test)]
            lane: self.lane,
            #[cfg(test)]
            generation: self.generation.bump(),
            _p: PhantomData,
        }
    }

    #[cfg(test)]
    /// Abort the transaction.
    ///
    /// Emits `ControlOp::TxAbort` and transitions to `Closed` state.
    /// The generation number is NOT bumped.
    pub(crate) fn abort(self, tap: &mut impl Tap) -> Closed<Inv> {
        tap.emit(ControlOp::TxAbort);
        Closed {
            #[cfg(test)]
            lane: self.lane,
            #[cfg(test)]
            generation: self.generation,
            _p: PhantomData,
        }
    }
}

/// Transaction in "closed" state (terminal state).
pub(crate) struct Closed<Inv> {
    #[cfg(test)]
    lane: Lane,
    #[cfg(test)]
    generation: Generation,
    _p: PhantomData<Inv>,
}

impl<Inv> Closed<Inv> {
    #[cfg(test)]
    /// Get the final lane identifier.
    pub(crate) fn lane(&self) -> Lane {
        self.lane
    }

    #[cfg(test)]
    /// Get the final generation number.
    pub(crate) fn generation(&self) -> Generation {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Concrete invariant type for testing
    struct TestInv;
    impl NoCrossLaneAliasing for TestInv {}
    impl AtMostOnceCommit for TestInv {}

    #[test]
    fn test_txn_happy_path() {
        let mut tap = NoopTap;

        // Create a transaction
        let txn: Txn<TestInv, IncreasingGen, crate::control::types::One> =
            unsafe { Txn::new(Lane::new(42), Generation::new(10)) };

        // Begin -> Ack -> Commit
        let in_begin = txn.begin(&mut tap);
        let in_acked = in_begin.ack(&mut tap);
        let closed = in_acked.commit(&mut tap);

        // Check final state
        assert_eq!(closed.lane(), Lane::new(42));
        assert_eq!(closed.generation(), Generation::new(11)); // Generation bumped
    }

    #[test]
    fn test_txn_abort_path() {
        let mut tap = NoopTap;

        let txn: Txn<TestInv, IncreasingGen, crate::control::types::One> =
            unsafe { Txn::new(Lane::new(42), Generation::new(10)) };

        // Begin -> Ack -> Abort
        let in_begin = txn.begin(&mut tap);
        let in_acked = in_begin.ack(&mut tap);
        let closed = in_acked.abort(&mut tap);

        // Check final state
        assert_eq!(closed.lane(), Lane::new(42));
        assert_eq!(closed.generation(), Generation::new(10)); // Generation NOT bumped
    }
}
