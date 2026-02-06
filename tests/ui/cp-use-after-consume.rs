//! CP Mini Kernel: Using a handle after consumption must fail
//!
//! This test verifies that the typestate machine prevents reusing a handle
//! after it has been consumed by a state transition.

use hibana::control::{AtMostOnceCommit, IncreasingGen, LaneId, NoCrossLaneAliasing, NoopTap, Txn};

struct TestInv;
impl NoCrossLaneAliasing for TestInv {}
impl AtMostOnceCommit for TestInv {}

fn main() {
    let mut tap = NoopTap;
    let txn: Txn<TestInv, IncreasingGen, hibana::control::One> =
        unsafe { Txn::new(LaneId::new(1), hibana::control::Gen::ZERO) };

    let in_begin = txn.begin(&mut tap);

    // Consume in_begin
    let _in_acked = in_begin.ack(&mut tap);

    // This should fail: in_begin was moved/consumed
    let _illegal = in_begin.ack(&mut tap);
}
