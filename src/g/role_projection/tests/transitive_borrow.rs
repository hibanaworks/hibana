//! A negative localside call must not wait for its own moved view through two joins.
use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

type ThirdLoan = g::Msg<5, Endpoint<'static, 12>>;
type Steps = g::Seq<
    g::Par<g::Send<11, 3, Read>, g::Send<10, 4, Read>>,
    g::Seq<
        g::Par<g::Send<11, 5, Value>, g::Send<12, 6, Value>>,
        g::Seq<g::Send<0, 1, Loan>, g::Seq<g::Send<0, 2, OtherLoan>, g::Send<0, 7, ThirdLoan>>>,
    >,
>;

#[test]
fn an_owned_send_cannot_wait_for_a_transitive_predecessor_using_its_payload() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let auth_program = crate::runtime::program::project::<11>(&g::Program::<Steps>::new());
    let third_program = crate::runtime::program::project::<12>(&g::Program::<Steps>::new());
    let reader_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 96 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .unwrap();
        let sid = SessionId::new(9043);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &db_program).unwrap();
        let mut auth = rv.enter(sid, &auth_program).unwrap();
        let _third = rv.enter(sid, &third_program).unwrap();
        let mut reader = rv.enter(sid, &reader_program).unwrap();
        auth.send::<Read>(&7).await.unwrap();
        assert_eq!(reader.recv::<Read>().await.unwrap(), 7);

        // DB's first Read has not happened. Both middle parallel operations
        // depend on that first join, and Loan depends on the middle join.
        // Moving DB into Loan makes that ancestor unavailable to this caller.
        let mut send = pin!(owner.send::<Loan>(db));
        let mut context = Context::from_waker(Waker::noop());
        let result = send.as_mut().poll(&mut context);
        assert!(
            matches!(result, Poll::Ready(Err(_))),
            "a transitive borrowed-view predecessor must reject, not wait: {result:?}"
        );
    });
}

#[test]
fn two_parallel_joins_release_the_owned_transfers_after_both_fragments_finish() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let other_program = crate::runtime::program::project::<2>(&g::Program::<Steps>::new());
    let third_worker_program = crate::runtime::program::project::<7>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let auth_program = crate::runtime::program::project::<11>(&g::Program::<Steps>::new());
    let third_program = crate::runtime::program::project::<12>(&g::Program::<Steps>::new());
    let first_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let second_program = crate::runtime::program::project::<4>(&g::Program::<Steps>::new());
    let middle_left_program = crate::runtime::program::project::<5>(&g::Program::<Steps>::new());
    let middle_right_program = crate::runtime::program::project::<6>(&g::Program::<Steps>::new());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 160 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, TestTransport::new())
            .unwrap();
        let sid = SessionId::new(9044);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &db_program).unwrap();
        let mut auth = rv.enter(sid, &auth_program).unwrap();
        let mut third = rv.enter(sid, &third_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut other = rv.enter(sid, &other_program).unwrap();
        let mut third_worker = rv.enter(sid, &third_worker_program).unwrap();
        let mut first = rv.enter(sid, &first_program).unwrap();
        let mut second = rv.enter(sid, &second_program).unwrap();
        let mut middle_left = rv.enter(sid, &middle_left_program).unwrap();
        let mut middle_right = rv.enter(sid, &middle_right_program).unwrap();
        let ((), db, auth, third, (), (), (), ()) = tokio::join!(
            async {
                let mut db = db;
                let (left, right) = tokio::join!(auth.send::<Read>(&7), db.send::<Read>(&11),);
                left.unwrap();
                right.unwrap();
                let (left, right) =
                    tokio::join!(auth.send::<Value>(&13), third.send::<Value>(&17),);
                left.unwrap();
                right.unwrap();
                owner.send::<Loan>(db).await.unwrap();
                owner.send::<OtherLoan>(auth).await.unwrap();
                owner.send::<ThirdLoan>(third).await.unwrap();
            },
            async { worker.recv::<Loan>().await.unwrap() },
            async { other.recv::<OtherLoan>().await.unwrap() },
            async { third_worker.recv::<ThirdLoan>().await.unwrap() },
            async {
                assert_eq!(first.recv::<Read>().await.unwrap(), 7);
            },
            async {
                assert_eq!(second.recv::<Read>().await.unwrap(), 11);
            },
            async {
                assert_eq!(middle_left.recv::<Value>().await.unwrap(), 13);
            },
            async {
                assert_eq!(middle_right.recv::<Value>().await.unwrap(), 17);
            },
        );
        drop((db, auth, third));
    });
}
