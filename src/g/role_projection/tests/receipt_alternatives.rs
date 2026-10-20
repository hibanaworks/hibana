//! An unknown return branch must not hide a borrow of its own receiver.
use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::{future::poll_fn, pin::pin, task::Poll};

#[test]
fn a_later_possible_self_receipt_cannot_be_hidden_by_an_independent_receipt() {
    use core::future::Future;

    type Tick = g::Msg<51, ()>;
    type ReturnElsewhere = g::Msg<52, Endpoint<'static, 10>>;
    type ReturnHere = g::Msg<53, Endpoint<'static, 10>>;
    type Steps = g::Seq<
        g::Roll<g::Seq<g::Send<0, 3, Tick>, g::Send<10, 4, Read>>>,
        g::Seq<
            g::Send<11, 1, Loan>,
            g::Seq<
                g::Route<g::Send<1, 11, ReturnElsewhere>, g::Send<1, 0, ReturnHere>>,
                g::Send<0, 2, OtherLoan>,
            >,
        >,
    >;
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let auth_program = crate::runtime::program::project::<11>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let ticker_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let reader_program = crate::runtime::program::project::<4>(&g::Program::<Steps>::new());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 96 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9051);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut ticker = rv.enter(sid, &ticker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            owner.send::<Tick>(&()).await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            ticker.recv::<Tick>().await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            auth.send::<Loan>(db).await.unwrap();
            let db = worker.recv::<Loan>().await.unwrap();
            worker.send::<ReturnHere>(db).await.unwrap();

            // Returning through OWNER is legal, but OWNER cannot receive it
            // while the restart future borrows that same Endpoint. Another
            // possible return through AUTH does not resolve this dependency.
            let mut restart = pin!(owner.send::<Tick>(&()));
            poll_fn(|cx| {
                let outcome = restart.as_mut().poll(cx);
                assert!(
                    matches!(outcome, Poll::Ready(Err(_))),
                    "an unknown alternative cannot authorize a self-receipt wait: {outcome:?}"
                );
                Poll::Ready(())
            })
            .await;
        });
}

#[test]
fn an_earlier_independent_return_can_restore_the_resource_before_a_later_self_receipt() {
    use core::future::Future;

    type Tick = g::Msg<54, ()>;
    type Steps = g::Seq<
        g::Roll<g::Seq<g::Send<0, 3, Tick>, g::Send<10, 4, Read>>>,
        g::Seq<
            g::Send<11, 1, Loan>,
            g::Seq<
                g::Send<1, 11, Loan>,
                g::Seq<g::Send<11, 1, Loan>, g::Seq<g::Send<1, 0, Loan>, g::Send<0, 2, OtherLoan>>>,
            >,
        >,
    >;
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let auth_program = crate::runtime::program::project::<11>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let ticker_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let reader_program = crate::runtime::program::project::<4>(&g::Program::<Steps>::new());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 96 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9052);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut ticker = rv.enter(sid, &ticker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            owner.send::<Tick>(&()).await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            ticker.recv::<Tick>().await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            auth.send::<Loan>(db).await.unwrap();
            let db = worker.recv::<Loan>().await.unwrap();

            let mut restart = pin!(owner.send::<Tick>(&()));
            poll_fn(|cx| {
                let outcome = restart.as_mut().poll(cx);
                assert!(
                    outcome.is_pending(),
                    "AUTH's earlier receipt can restore DB without borrowing OWNER: {outcome:?}"
                );
                Poll::Ready(())
            })
            .await;
            worker.send::<Loan>(db).await.unwrap();
            let (_, db) = tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        restart.await.unwrap();
                        ticker.recv::<Tick>().await.unwrap();
                        assert_eq!(reader.recv::<Read>().await.unwrap(), 9);
                    },
                    async {
                        let mut db = auth.recv::<Loan>().await.unwrap();
                        db.send::<Read>(&9).await.unwrap();
                        db
                    },
                )
            })
            .await
            .expect("the first independent return, not every later receipt, is needed");
            assert!(network.queue_is_empty());
            drop(db);
        });
}
