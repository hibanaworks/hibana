//! A dormant resource can be recovered by an independent logical view.
use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::{
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
};

#[test]
fn a_rolled_head_waits_for_another_view_to_receive_its_resource_back() {
    type Tick = g::Msg<39, ()>;
    type Steps = g::Seq<
        g::Roll<g::Seq<g::Send<0, 3, Tick>, g::Send<10, 4, Read>>>,
        g::Seq<g::Send<11, 1, Loan>, g::Seq<g::Send<1, 11, Loan>, g::Send<0, 2, OtherLoan>>>,
    >;
    type Independent = g::Msg<37, u32>;
    type ThirdLoan = g::Msg<5, Endpoint<'static, 12>>;
    type Global =
        g::Seq<g::Par<Steps, g::Roll<g::Send<12, 8, Independent>>>, g::Send<0, 2, ThirdLoan>>;
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Global, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 16>::IMAGE_REF,
    );
    let ticker_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Global, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Global, 16>::IMAGE_REF,
    );
    let independent_program = crate::global::role_program::role_program_from_image::<12>(
        &RoleProjection::<12, Global, 16>::IMAGE_REF,
    );
    let peer_program = crate::global::role_program::role_program_from_image::<8>(
        &RoleProjection::<8, Global, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9033);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut ticker = rv.enter(sid, &ticker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            let mut independent = rv.enter(sid, &independent_program).unwrap();
            let mut peer = rv.enter(sid, &peer_program).unwrap();
            owner.send::<Tick>(&()).await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            ticker.recv::<Tick>().await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            auth.send::<Loan>(db).await.unwrap();
            let db = worker.recv::<Loan>().await.unwrap();
            assert!(network.queue_is_empty());

            let mut restart = pin!(owner.send::<Tick>(&()));
            poll_fn(|cx| {
                let outcome = restart.as_mut().poll(cx);
                assert!(
                    outcome.is_pending(),
                    "AUTH can still receive DB back: {outcome:?}"
                );
                Poll::Ready(())
            })
            .await;
            assert!(network.queue_is_empty());
            for value in [2, 3] {
                independent.send::<Independent>(&value).await.unwrap();
                assert_eq!(peer.recv::<Independent>().await.unwrap(), value);
            }
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
            .expect("the independent return wakes the rolled head");
            assert!(network.queue_is_empty());
            drop(db);
        });
}

#[test]
fn a_rolled_head_cannot_wait_for_a_receipt_on_its_own_borrowed_view() {
    type Tick = g::Msg<39, ()>;
    type Steps = g::Seq<
        g::Roll<g::Seq<g::Send<0, 3, Tick>, g::Send<10, 4, Read>>>,
        g::Seq<g::Send<11, 1, Loan>, g::Seq<g::Send<1, 0, Loan>, g::Send<0, 2, OtherLoan>>>,
    >;
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let ticker_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9034);
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
            worker.send::<Loan>(db).await.unwrap();
            let result =
                tokio::time::timeout(core::time::Duration::from_secs(10), owner.send::<Tick>(&()))
                    .await;
            assert!(
                matches!(result, Ok(Err(_))),
                "the same view must receive first: {result:?}"
            );
        });
}

#[test]
fn an_operation_cannot_wait_for_an_earlier_transfer_of_its_own_endpoint() {
    type Body = g::Seq<g::Send<0, 11, Loan>, g::Seq<g::Send<10, 3, Read>, g::Send<11, 0, Loan>>>;
    type Steps = g::Seq<g::Roll<Body>, g::Send<0, 2, OtherLoan>>;
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9035);
            let owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let auth = rv.enter(sid, &auth_program).unwrap();
            let result =
                tokio::time::timeout(core::time::Duration::from_secs(10), db.send::<Read>(&7))
                    .await;
            assert!(
                matches!(result, Ok(Err(_))),
                "Read borrows the value required by the earlier Loan: {result:?}"
            );
            assert!(network.queue_is_empty());
            drop((owner, auth));
        });
}

#[test]
fn a_remote_tick_stays_queued_until_another_view_receives_the_entry_resource() {
    type Tick = g::Msg<39, ()>;
    type Steps = g::Seq<
        g::Roll<g::Seq<g::Send<0, 1, Tick>, g::Send<10, 3, Read>>>,
        g::Seq<g::Send<11, 4, Loan>, g::Seq<g::Send<4, 11, Loan>, g::Send<1, 2, OtherLoan>>>,
    >;
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let controller_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let owner_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9036);
            let mut controller = rv.enter(sid, &controller_program).unwrap();
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            controller.send::<Tick>(&()).await.unwrap();
            owner.recv::<Tick>().await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            auth.send::<Loan>(db).await.unwrap();
            let db = worker.recv::<Loan>().await.unwrap();
            assert!(network.queue_is_empty());
            controller.send::<Tick>(&()).await.unwrap();
            let mut restart = pin!(owner.recv::<Tick>());
            poll_fn(|cx| {
                let outcome = restart.as_mut().poll(cx);
                assert!(
                    outcome.is_pending(),
                    "the resource can return through AUTH: {outcome:?}"
                );
                Poll::Ready(())
            })
            .await;
            assert!(
                !network.queue_is_empty(),
                "Tick must stay uncommitted in the transport"
            );
            worker.send::<Loan>(db).await.unwrap();
            let (_, db) = tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        restart.await.unwrap();
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
            .expect("return and the already accepted Tick can both complete");
            assert!(network.queue_is_empty());
            drop(db);
        });
}

#[test]
fn an_offered_tick_waits_for_another_view_to_receive_the_entry_resource() {
    type Tick = g::Msg<39, ()>;
    type Tock = g::Msg<40, ()>;
    type AlternateRead = g::Msg<41, u32>;
    type Steps = g::Seq<
        g::Roll<
            g::Route<
                g::Seq<g::Send<0, 1, Tick>, g::Send<10, 3, Read>>,
                g::Seq<g::Send<0, 1, Tock>, g::Send<10, 3, AlternateRead>>,
            >,
        >,
        g::Seq<g::Send<11, 4, Loan>, g::Seq<g::Send<4, 11, Loan>, g::Send<1, 2, OtherLoan>>>,
    >;
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let controller_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let owner_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 16>::IMAGE_REF,
    );
    let reader_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 16>::IMAGE_REF,
    );
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
            let sid = SessionId::new(9037);
            let mut controller = rv.enter(sid, &controller_program).unwrap();
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut db = rv.enter(sid, &db_program).unwrap();
            let mut auth = rv.enter(sid, &auth_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut reader = rv.enter(sid, &reader_program).unwrap();
            controller.send::<Tick>(&()).await.unwrap();
            owner.offer().await.unwrap().recv::<Tick>().await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            assert_eq!(reader.recv::<Read>().await.unwrap(), 7);
            auth.send::<Loan>(db).await.unwrap();
            let db = worker.recv::<Loan>().await.unwrap();
            controller.send::<Tick>(&()).await.unwrap();
            {
                let preview = owner.offer().await.unwrap();
                assert_eq!(preview.label(), 39);
                let mut paused = pin!(preview.recv::<Tick>());
                poll_fn(|cx| {
                    assert!(paused.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
                // Dropping the uncommitted wait must preserve the preview's
                // frame and leave the same branch available to offer again.
            }
            let branch = owner.offer().await.unwrap();
            assert_eq!(branch.label(), 39);
            let mut restart = pin!(branch.recv::<Tick>());
            poll_fn(|cx| {
                let outcome = restart.as_mut().poll(cx);
                assert!(
                    outcome.is_pending(),
                    "the offered branch must wait for AUTH's receipt: {outcome:?}"
                );
                Poll::Ready(())
            })
            .await;
            worker.send::<Loan>(db).await.unwrap();
            let (_, db) = tokio::time::timeout(core::time::Duration::from_secs(10), async {
                tokio::join!(
                    async {
                        restart.await.unwrap();
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
            .expect("a branch preview must not prevent another view's return");
            assert!(network.queue_is_empty());
            drop(db);
        });
}
