//! CP Mini Kernel: Wrong shot discipline must fail
//!
//! This test verifies that Many-shot transactions cannot be committed with
//! the One-shot API (which requires OneShot trait bound).

use hibana::control::{AtMostOnceCommit, IncreasingGen, LaneId, NoCrossLaneAliasing, NoopTap, Txn};

struct TestInv;
impl NoCrossLaneAliasing for TestInv {}
impl AtMostOnceCommit for TestInv {}

fn main() {
    let mut tap = NoopTap;

    // Create a Many-shot transaction
    let txn: Txn<TestInv, IncreasingGen, hibana::control::Many> =
        unsafe { Txn::new(LaneId::new(1), hibana::control::Gen::ZERO) };

    let in_begin = txn.begin(&mut tap);
    let in_acked = in_begin.ack(&mut tap);

    // This should fail: commit() requires OneShot, but we have Many
    let _closed = in_acked.commit(&mut tap);
}
