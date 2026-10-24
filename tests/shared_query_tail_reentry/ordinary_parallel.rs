use super::*;
use core::{
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Wake, Waker},
};

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

type Data = Msg<1, u32>;
type Stop = Msg<2, ()>;
type Permit = Msg<3, ()>;
type Ack = Msg<4, ()>;
type Close = Msg<5, ()>;
type Next = Msg<6, ()>;
type Reply = Msg<7, ()>;

fn parallel_input_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let input = g::par(
        g::send::<0, 1, Data>(),
        g::route(
            g::send::<2, 1, Stop>(),
            g::seq(g::send::<2, 1, Permit>(), g::send::<1, 2, Ack>()),
        ),
    );
    let work = g::route(
        g::send::<0, 1, Close>(),
        g::seq(g::send::<0, 1, Next>(), g::send::<1, 0, Reply>()),
    );
    project(&g::seq(input, work).roll())
}

#[test]
fn normal_parallel_input_is_not_route_evidence_on_roll_reentry() {
    // No OS I/O is used: keep Tokio's I/O driver disabled for strict Miri.
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let p0 = parallel_input_program::<0>();
            let p1 = parallel_input_program::<1>();
            let p2 = parallel_input_program::<2>();
            let mut bytes = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut bytes, common::TestTransport::new())
                .unwrap();
            let mut a = rv.enter(SessionId::new(37), &p0).unwrap();
            let mut b = rv.enter(SessionId::new(37), &p1).unwrap();
            let mut c = rv.enter(SessionId::new(37), &p2).unwrap();
            for round in 0..4u32 {
                {
                    let mut offer = pin!(b.offer());
                    for _ in 0..round % 2 {
                        poll_fn(|cx| {
                            assert!(matches!(offer.as_mut().poll(cx), Poll::Pending));
                            Poll::Ready(())
                        })
                        .await;
                    }
                    a.send::<Data>(&round).await.unwrap();
                    let wakes = Arc::new(WakeCount::default());
                    let waker = Waker::from(wakes.clone());
                    let mut observed_cx = Context::from_waker(&waker);
                    assert!(offer.as_mut().poll(&mut observed_cx).is_pending());
                    assert_eq!(
                        wakes.0.load(Ordering::Relaxed),
                        0,
                        "ordinary input alone cannot make a route preview runnable"
                    );
                    for _ in 0..4 {
                        poll_fn(|cx| {
                            match offer.as_mut().poll(cx) {
                                Poll::Pending => {}
                                Poll::Ready(Err(error)) => panic!("round {round}: {error:?}"),
                                Poll::Ready(Ok(branch)) => {
                                    panic!("round {round}: label {}", branch.label())
                                }
                            }
                            Poll::Ready(())
                        })
                        .await;
                    }
                    assert!(
                        tokio::time::timeout(std::time::Duration::from_millis(1), offer.as_mut())
                            .await
                            .is_err(),
                        "ordinary input is not a route preview"
                    );
                    tokio::try_join!(
                        async { offer.as_mut().await?.recv::<Permit>().await },
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                            c.send::<Permit>(&()).await
                        },
                    )
                    .unwrap();
                }
                b.send::<Ack>(&()).await.unwrap();
                c.recv::<Ack>().await.unwrap();
                assert_eq!(b.recv::<Data>().await.unwrap(), round);
                a.send::<Next>(&()).await.unwrap();
                b.recv::<Next>().await.unwrap();
                b.send::<Reply>(&()).await.unwrap();
                a.recv::<Reply>().await.unwrap();
            }
        });
}

#[test]
fn completed_other_lane_preserves_fresh_ordinary_input() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            type Last = Msg<8, ()>;
            type ClosedLast = Msg<9, ()>;
            type Independent = Msg<10, ()>;
            type OtherIndependent = Msg<11, ()>;
            type IndependentAck = Msg<12, ()>;
            let input = g::par(
                g::send::<0, 1, Data>(),
                g::route(
                    g::send::<2, 1, Stop>(),
                    g::seq(g::send::<2, 1, Permit>(), g::send::<1, 2, Ack>()),
                ),
            );
            let work = g::route(
                g::seq(g::send::<0, 1, Close>(), g::send::<1, 2, ClosedLast>()),
                g::seq(
                    g::send::<0, 1, Next>(),
                    g::par(g::send::<1, 0, Reply>(), g::send::<1, 2, Last>()),
                ),
            );
            let independent = g::seq(
                g::route(
                    g::send::<3, 1, Independent>(),
                    g::send::<3, 1, OtherIndependent>(),
                ),
                g::send::<1, 3, IndependentAck>(),
            )
            .roll();
            let global = g::par(g::seq(input, work).roll(), independent);
            let p0: RoleProgram<0> = project(&global);
            let p1: RoleProgram<1> = project(&global);
            let p2: RoleProgram<2> = project(&global);
            let p3: RoleProgram<3> = project(&global);
            let mut bytes = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut bytes, common::TestTransport::new())
                .unwrap();
            let mut a = rv.enter(SessionId::new(38), &p0).unwrap();
            let mut b = rv.enter(SessionId::new(38), &p1).unwrap();
            let mut c = rv.enter(SessionId::new(38), &p2).unwrap();
            let mut d = rv.enter(SessionId::new(38), &p3).unwrap();
            a.send::<Data>(&0).await.unwrap();
            for round in 0..4u32 {
                c.send::<Permit>(&()).await.unwrap();
                b.offer().await.unwrap().recv::<Permit>().await.unwrap();
                b.send::<Ack>(&()).await.unwrap();
                c.recv::<Ack>().await.unwrap();
                assert_eq!(b.recv::<Data>().await.unwrap(), round);
                a.send::<Next>(&()).await.unwrap();
                b.recv::<Next>().await.unwrap();
                b.send::<Reply>(&()).await.unwrap();
                a.recv::<Reply>().await.unwrap();
                // Refresh all route candidates while this visit is still incomplete.
                d.send::<Independent>(&()).await.unwrap();
                b.offer()
                    .await
                    .unwrap()
                    .recv::<Independent>()
                    .await
                    .unwrap();
                b.send::<IndependentAck>(&()).await.unwrap();
                d.recv::<IndependentAck>().await.unwrap();
                // The producer's local visit is complete while the receiver still has
                // one send on another lane. Its next ordinary input can arrive now.
                a.send::<Data>(&(round + 1)).await.unwrap();
                b.send::<Last>(&()).await.unwrap();
                c.recv::<Last>().await.unwrap();
                {
                    let mut offer = pin!(b.offer());
                    poll_fn(|cx| {
                        match offer.as_mut().poll(cx) {
                            Poll::Pending => {}
                            Poll::Ready(Err(error)) => {
                                panic!("round {round}: ordinary input returned {error:?}")
                            }
                            Poll::Ready(Ok(branch)) => {
                                panic!("round {round}: ordinary input previewed {}", branch.label())
                            }
                        }
                        Poll::Ready(())
                    })
                    .await;
                    assert!(
                        tokio::time::timeout(std::time::Duration::from_millis(1), offer.as_mut())
                            .await
                            .is_err()
                    );
                    tokio::try_join!(
                        async { offer.as_mut().await?.recv::<OtherIndependent>().await },
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                            d.send::<OtherIndependent>(&()).await
                        },
                    )
                    .unwrap();
                }
                b.send::<IndependentAck>(&()).await.unwrap();
                d.recv::<IndependentAck>().await.unwrap();
            }
            assert_eq!(b.recv::<Data>().await.unwrap(), 4);
        });
}

#[test]
fn ordinary_input_does_not_suspend_an_independent_local_resolver() {
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let global = g::par(
                g::send::<0, 1, Data>(),
                g::route(g::send::<1, 2, Permit>(), g::send::<1, 2, Stop>()).resolve::<21>(),
            )
            .roll();
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let p2 = project::<2>(&global);
            let mut bytes = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut bytes, common::TestTransport::new())
                .unwrap();
            rv.set_resolver(
                &p1,
                ResolverRef::<21>::decision_state(&(), |_| Ok(DecisionArm::Left)),
            )
            .unwrap();
            let mut a = rv.enter(SessionId::new(39), &p0).unwrap();
            let mut b = rv.enter(SessionId::new(39), &p1).unwrap();
            let mut c = rv.enter(SessionId::new(39), &p2).unwrap();
            for round in 0..4u32 {
                a.send::<Data>(&round).await.unwrap();
                tokio::time::timeout(std::time::Duration::from_millis(1), async {
                    b.offer().await?.send::<Permit>(&()).await
                })
                .await
                .expect("an independent resolver needs no new transport input")
                .unwrap();
                c.recv::<Permit>().await.unwrap();
                assert_eq!(b.recv::<Data>().await.unwrap(), round);
            }
        });
}
