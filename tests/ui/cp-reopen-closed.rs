//! CP Mini Kernel: Reopening a closed transaction must fail
//!
//! This test verifies that once a transaction reaches the Closed state,
//! it cannot be reopened or reused.

use hibana::control::{AtMostOnceCommit, IncreasingGen, LaneId, NoCrossLaneAliasing, NoopTap, Txn};

struct TestInv;
impl NoCrossLaneAliasing for TestInv {}
impl AtMostOnceCommit for TestInv {}

fn main() {
    let mut tap = NoopTap;
    let txn: Txn<TestInv, IncreasingGen, hibana::control::One> =
        unsafe { Txn::new(LaneId::new(1), hibana::control::Gen::ZERO) };

    let in_begin = txn.begin(&mut tap);
    let in_acked = in_begin.ack(&mut tap);
    let closed = in_acked.commit(&mut tap);

    // This should fail: Closed has no begin() method to reopen
    let _illegal = closed.begin(&mut tap);
}
