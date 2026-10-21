//! Independent owned sends retain distinct pending bytes on the same actor.
use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::{
    future::{Future, poll_fn},
    pin::pin,
    task::{Context, Poll, Waker},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

type CarrierLoan = g::Msg<5, Endpoint<'static, 12>>;
type Body = g::Seq<
    g::Send<0, 1, CarrierLoan>,
    g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Send<0, 1, OtherLoan>,
            g::Seq<
                g::Par<g::Send<1, 0, Loan>, g::Send<12, 0, OtherLoan>>,
                g::Par<
                    g::Par<
                        g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>,
                        g::Seq<g::Send<11, 4, Read>, g::Send<4, 11, Value>>,
                    >,
                    g::Send<1, 0, CarrierLoan>,
                >,
            >,
        >,
    >,
>;
type Steps = g::Roll<Body>;

#[test]
fn independent_roll_and_route_progress_while_another_roll_waits_for_input() {
    type A = g::Msg<5, ()>;
    type B = g::Msg<6, ()>;
    type C = g::Msg<7, ()>;
    type D = g::Msg<8, ()>;
    type Start = g::Msg<9, ()>;
    type Left = g::Msg<10, ()>;
    type Right = g::Msg<11, ()>;
    type X = g::Msg<12, ()>;
    type Y = g::Msg<13, ()>;
    let graph = g::par(
        g::par(
            g::route(
                g::seq(g::send::<1, 0, A>(), g::send::<0, 1, B>()),
                g::seq(g::send::<1, 0, C>(), g::send::<0, 1, D>()),
            )
            .roll(),
            g::par(g::send::<0, 2, X>(), g::send::<0, 2, Y>()),
        ),
        g::seq(
            g::send::<0, 2, Start>(),
            g::route(g::send::<2, 0, Left>(), g::send::<2, 0, Right>()),
        )
        .roll(),
    );
    let p0 = crate::runtime::program::project::<0>(&graph);
    let p1 = crate::runtime::program::project::<1>(&graph);
    let p2 = crate::runtime::program::project::<2>(&graph);
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let mut slab = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, TestTransport::new())
                .unwrap();
            let sid = SessionId::new(9061);
            let mut a = rv.enter(sid, &p0).unwrap();
            let mut b = rv.enter(sid, &p1).unwrap();
            let mut c = rv.enter(sid, &p2).unwrap();
            let [mut waiting, mut x, mut y, mut request] = a.par().unwrap();
            let [mut x_peer, mut y_peer, mut reply] = c.par().unwrap();
            {
                let mut receive = pin!(waiting.recv::<A>());
                assert!(futures::poll!(receive.as_mut()).is_pending());
                tokio::time::timeout(core::time::Duration::from_millis(100), async {
                    tokio::join!(
                        async {
                            x.send::<X>(&()).await.unwrap();
                        },
                        async {
                            x_peer.recv::<X>().await.unwrap();
                        },
                        async {
                            y.send::<Y>(&()).await.unwrap();
                        },
                        async {
                            y_peer.recv::<Y>().await.unwrap();
                        },
                        async {
                            request.send::<Start>(&()).await.unwrap();
                            request.offer().await.unwrap().recv::<Left>().await.unwrap();
                        },
                        async {
                            reply.recv::<Start>().await.unwrap();
                            reply.send::<Left>(&()).await.unwrap();
                        },
                    );
                })
                .await
                .expect("independent operations must not wait for the other roll");
                b.send::<A>(&()).await.unwrap();
                receive.await.unwrap();
            }
            waiting.send::<B>(&()).await.unwrap();
            b.recv::<B>().await.unwrap();
        });
}

#[test]
fn distinct_owned_sends_can_both_park_and_the_returned_views_can_run_in_parallel() {
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let auth_program = crate::runtime::program::project::<11>(&g::Program::<Steps>::new());
    let carrier_program = crate::runtime::program::project::<12>(&g::Program::<Steps>::new());
    let db_service_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let auth_service_program = crate::runtime::program::project::<4>(&g::Program::<Steps>::new());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut owner_slab = [0; 64 * 1024];
            let mut worker_slab = [0; 64 * 1024];
            let mut owner_storage = SessionKitStorage::uninit();
            let mut worker_storage = SessionKitStorage::uninit();
            let owner_rv = owner_storage
                .init()
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let worker_rv = worker_storage
                .init()
                .rendezvous(&mut worker_slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9054);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let mut db = owner_rv.enter(sid, &db_program).unwrap();
            let mut auth = owner_rv.enter(sid, &auth_program).unwrap();
            let mut carrier = owner_rv.enter(sid, &carrier_program).unwrap();
            let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
            let mut db_service = worker_rv.enter(sid, &db_service_program).unwrap();
            let mut auth_service = worker_rv.enter(sid, &auth_service_program).unwrap();
            let visits = async {
                for db_key in [17, 31] {
                    owner.send::<CarrierLoan>(carrier).await.unwrap();
                    owner.send::<Loan>(db).await.unwrap();
                    owner.send::<OtherLoan>(auth).await.unwrap();
                    let mut worker_carrier = worker.recv::<CarrierLoan>().await.unwrap();
                    let moved_db = worker.recv::<Loan>().await.unwrap();
                    let moved_auth = worker.recv::<OtherLoan>().await.unwrap();
                    assert!(network.queue_is_empty());

                    network.pause_send(1);
                    network.pause_send(12);
                    let (mut returned_db, mut returned_auth) = {
                        let mut db_return = pin!(worker.send::<Loan>(moved_db));
                        let mut auth_return = pin!(worker_carrier.send::<OtherLoan>(moved_auth));
                        let db_wakes = Arc::new(super::send_wait::WakeCount(AtomicUsize::new(0)));
                        let auth_wakes = Arc::new(super::send_wait::WakeCount(AtomicUsize::new(0)));
                        let db_waker = Waker::from(db_wakes.clone());
                        let auth_waker = Waker::from(auth_wakes.clone());
                        let mut db_cx = Context::from_waker(&db_waker);
                        let mut auth_cx = Context::from_waker(&auth_waker);
                        poll_fn(|_| {
                            assert!(db_return.as_mut().poll(&mut db_cx).is_pending());
                            assert!(auth_return.as_mut().poll(&mut auth_cx).is_pending());
                            // Neither private send can replace the other's staged frame.
                            assert!(db_return.as_mut().poll(&mut db_cx).is_pending());
                            assert!(auth_return.as_mut().poll(&mut auth_cx).is_pending());
                            Poll::Ready(())
                        })
                        .await;
                        assert!(network.queue_is_empty());
                        let db_before = db_wakes.0.load(Ordering::Relaxed);
                        let auth_before = auth_wakes.0.load(Ordering::Relaxed);
                        network.resume_send();
                        assert!(db_wakes.0.load(Ordering::Relaxed) > db_before);
                        assert!(auth_wakes.0.load(Ordering::Relaxed) > auth_before);
                        // Accept the second-staged frame first; the first stays private.
                        let (auth_sent, db_sent) =
                            tokio::join!(biased; auth_return.as_mut(), db_return.as_mut());
                        db_sent.unwrap();
                        auth_sent.unwrap();
                        let db = owner.recv::<Loan>().await.unwrap();
                        let auth = owner.recv::<OtherLoan>().await.unwrap();
                        // Both completed sender futures outlive the fresh received values.
                        (db, auth)
                    };

                    let (db_value, auth_value, sent, received, (), ()) = tokio::join!(
                        async {
                            returned_db.send::<Read>(&db_key).await.unwrap();
                            returned_db.recv::<Value>().await.unwrap()
                        },
                        async {
                            returned_auth.send::<Read>(&(db_key + 12)).await.unwrap();
                            returned_auth.recv::<Value>().await.unwrap()
                        },
                        worker.send::<CarrierLoan>(worker_carrier),
                        owner.recv::<CarrierLoan>(),
                        async {
                            let value = db_service.recv::<Read>().await.unwrap();
                            db_service.send::<Value>(&(value + 1)).await.unwrap();
                        },
                        async {
                            let value = auth_service.recv::<Read>().await.unwrap();
                            auth_service.send::<Value>(&(value + 1)).await.unwrap();
                        },
                    );
                    assert_eq!((db_value, auth_value), (db_key + 1, db_key + 13));
                    sent.unwrap();
                    carrier = received.unwrap();
                    db = returned_db;
                    auth = returned_auth;
                    assert!(network.queue_is_empty());
                }
            };
            tokio::time::timeout(core::time::Duration::from_secs(30), visits)
                .await
                .expect("independent return or resource work stopped progressing");
            assert!(network.queue_is_empty());
        });
}
