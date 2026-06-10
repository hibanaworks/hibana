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

use crate::control::types::{
    AtMostOnceCommit, Generation, IncreasingGen, Lane, NoCrossLaneAliasing, One,
};
use core::marker::PhantomData;

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
    pub(crate) fn begin(self) -> InBegin<Inv, S> {
        InBegin { _p: PhantomData }
    }
}

/// Transaction in "begin" state (after `begin()`, before `ack()`).
pub(crate) struct InBegin<Inv, Shot> {
    _p: PhantomData<(Inv, Shot)>,
}

impl<Inv, S> InBegin<Inv, S> {
    /// Acknowledge the topology operation.
    pub(crate) fn ack(self) -> InAcked<Inv, S> {
        InAcked { _p: PhantomData }
    }
}

/// Transaction in "acked" state (after `ack()`, before `commit()` or `abort()`).
pub(crate) struct InAcked<Inv, Shot> {
    _p: PhantomData<(Inv, Shot)>,
}

impl<Inv: AtMostOnceCommit> InAcked<Inv, One> {
    /// Commit the transaction.
    pub(crate) fn commit(self) -> Closed<Inv> {
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

    #[test]
    fn test_txn_happy_path() {
        // Create a transaction
        let txn: Txn<TestInv, IncreasingGen, crate::control::types::One> =
            /* SAFETY: the topology owner has validated the lane/generation transition before minting this typestate transaction witness. */ unsafe { Txn::new(Lane::new(42), Generation::new(10)) };

        // Begin -> Ack -> Commit
        let in_begin = txn.begin();
        let in_acked = in_begin.ack();
        in_acked.commit();
    }
}
