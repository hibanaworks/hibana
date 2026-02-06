//! CP Mini Kernel: Skipping ack must fail
//!
//! This test verifies that the typestate machine enforces the correct sequence:
//! begin() -> ack() -> commit(). You cannot commit() directly from InBegin state.

use hibana::control::{AtMostOnceCommit, IncreasingGen, LaneId, NoCrossLaneAliasing, NoopTap, Txn};

struct TestInv;
impl NoCrossLaneAliasing for TestInv {}
impl AtMostOnceCommit for TestInv {}

fn main() {
    let mut tap = NoopTap;
    let txn: Txn<TestInv, IncreasingGen, hibana::control::One> =
        unsafe { Txn::new(LaneId::new(1), hibana::control::Gen::ZERO) };

    let in_begin = txn.begin(&mut tap);

    // This should fail: InBegin has no commit() method, only ack()
    let _closed = in_begin.commit(&mut tap);
}
