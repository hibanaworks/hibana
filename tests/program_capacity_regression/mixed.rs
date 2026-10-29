#[path = "transport.rs"]
mod transport;

use hibana::{
    Endpoint, EndpointError, g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverRef},
    },
};
use std::{cell::Cell, time::Duration};

type Loan = g::Msg<240, Endpoint<'static, 2>>;
type A = g::Msg<1, u32>;
type B = g::Msg<2, u32>;
type C = g::Msg<3, u32>;
type D = g::Msg<4, u32>;
type Left = g::Msg<5, u32>;
type Right = g::Msg<6, u32>;
type OwnerDone = g::Msg<7, ()>;
type ServiceDone = g::Msg<8, ()>;
type Head = g::Msg<9, ()>;

macro_rules! burst {
    ($from:literal, $to:literal, $message:ty) => {{
        let p = g::seq(
            g::send::<$from, $to, $message>(),
            g::send::<$to, $from, $message>(),
        );
        let p = g::seq(p, p);
        let p = g::seq(p, p);
        let p = g::seq(p, p);
        let p = g::seq(p, p);
        let p = g::seq(p, p);
        let p = g::seq(p, p);
        g::seq(p, p)
    }};
}

#[tokio::test(flavor = "current_thread")]
async fn delegated_parallel_choices_and_nested_repeats_above_1500_events() {
    // 1,536 events in both choices, two outer headers, two transfers, two final messages.
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::seq(
                g::send::<1, 3, Head>(),
                g::seq(
                    g::send::<1, 0, Head>(),
                    g::par(
                        g::route(
                            g::par(burst!(2, 3, A), burst!(2, 3, B)),
                            g::par(burst!(2, 3, C), burst!(2, 3, D)),
                        )
                        .resolve::<10>()
                        .roll(),
                        g::route(burst!(1, 0, Left), burst!(1, 0, Right))
                            .resolve::<11>()
                            .roll(),
                    ),
                ),
            )
            .roll(),
            g::seq(
                g::send::<1, 0, Loan>(),
                g::seq(g::send::<0, 1, OwnerDone>(), g::send::<0, 3, ServiceDone>()),
            ),
        ),
    );
    let (p0, p1, p2, p3) = (
        project::<0>(&global),
        project::<1>(&global),
        project::<2>(&global),
        project::<3>(&global),
    );
    for choice in [DecisionArm::Left, DecisionArm::Right] {
        let network = transport::Network::new();
        let (resource_choices, worker_choices) =
            ((choice, Cell::new(0usize)), (choice, Cell::new(0usize)));
        let (mut slab0, mut slab1, mut slab3) = (
            vec![0; 128 * 1024],
            vec![0; 128 * 1024],
            vec![0; 128 * 1024],
        );
        let (mut storage0, mut storage1, mut storage3) = (
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
        );
        let r0 = storage0.init().rendezvous(&mut slab0, &network).unwrap();
        let r1 = storage1.init().rendezvous(&mut slab1, &network).unwrap();
        let r3 = storage3.init().rendezvous(&mut slab3, &network).unwrap();
        r1.set_resolver(
            &p2,
            ResolverRef::<10>::decision_state(&resource_choices, |(choice, calls)| {
                calls.set(calls.get() + 1);
                Ok(*choice)
            }),
        )
        .unwrap();
        r1.set_resolver(
            &p1,
            ResolverRef::<11>::decision_state(&worker_choices, |(choice, calls)| {
                calls.set(calls.get() + 1);
                Ok(*choice)
            }),
        )
        .unwrap();
        let sid = SessionId::new(1542);
        let mut owner = r0.enter(sid, &p0).unwrap();
        let db = r0.enter(sid, &p2).unwrap();
        let mut worker = r1.enter(sid, &p1).unwrap();
        let mut service = r3.enter(sid, &p3).unwrap();
        tokio::time::timeout(Duration::from_secs(30), async {
            tokio::try_join!(
                async {
                    owner.send::<Loan>(db).await?;
                    for _ in 0..2 {
                        owner.recv::<Head>().await?;
                        for _ in 0..2 {
                            let label = owner.offer().await?.label();
                            match label {
                                5 => {
                                    for i in 0..128u32 {
                                        let value = owner.recv::<Left>().await?;
                                        assert_eq!(value, i);
                                        owner.send::<Left>(&(value + 1)).await?;
                                    }
                                }
                                6 => {
                                    for i in 0..128u32 {
                                        let value = owner.recv::<Right>().await?;
                                        assert_eq!(value, i);
                                        owner.send::<Right>(&(value + 1)).await?;
                                    }
                                }
                                _ => panic!("unexpected worker choice"),
                            }
                        }
                    }
                    let db = owner.recv::<Loan>().await?;
                    owner.send::<OwnerDone>(&()).await?;
                    owner.send::<ServiceDone>(&()).await?;
                    drop(db);
                    Ok::<_, EndpointError>(())
                },
                async {
                    let mut db = worker.recv::<Loan>().await?;
                    for _ in 0..2 {
                        worker.send::<Head>(&()).await?;
                        worker.send::<Head>(&()).await?;
                        tokio::try_join!(
                            async {
                                for _ in 0..2 {
                                    let label = db.offer().await?.label();
                                    match label {
                                        1 | 2 => {
                                            let [mut a, mut b] = db.par()?;
                                            tokio::try_join!(
                                                async {
                                                    for i in 0..128u32 {
                                                        a.send::<A>(&i).await?;
                                                        assert_eq!(a.recv::<A>().await?, i + 1);
                                                    }
                                                    Ok::<_, EndpointError>(())
                                                },
                                                async {
                                                    for i in 0..128u32 {
                                                        b.send::<B>(&i).await?;
                                                        assert_eq!(b.recv::<B>().await?, i + 1);
                                                    }
                                                    Ok::<_, EndpointError>(())
                                                },
                                            )?;
                                        }
                                        3 | 4 => {
                                            let [mut a, mut b] = db.par()?;
                                            tokio::try_join!(
                                                async {
                                                    for i in 0..128u32 {
                                                        a.send::<C>(&i).await?;
                                                        assert_eq!(a.recv::<C>().await?, i + 1);
                                                    }
                                                    Ok::<_, EndpointError>(())
                                                },
                                                async {
                                                    for i in 0..128u32 {
                                                        b.send::<D>(&i).await?;
                                                        assert_eq!(b.recv::<D>().await?, i + 1);
                                                    }
                                                    Ok::<_, EndpointError>(())
                                                },
                                            )?;
                                        }
                                        _ => panic!("unexpected delegated choice"),
                                    }
                                }
                                Ok::<_, EndpointError>(())
                            },
                            async {
                                for _ in 0..2 {
                                    let label = worker.offer().await?.label();
                                    match label {
                                        5 => {
                                            for i in 0..128u32 {
                                                worker.send::<Left>(&i).await?;
                                                assert_eq!(worker.recv::<Left>().await?, i + 1);
                                            }
                                        }
                                        6 => {
                                            for i in 0..128u32 {
                                                worker.send::<Right>(&i).await?;
                                                assert_eq!(worker.recv::<Right>().await?, i + 1);
                                            }
                                        }
                                        _ => panic!("unexpected worker selection"),
                                    }
                                }
                                Ok::<_, EndpointError>(())
                            },
                        )?;
                    }
                    worker.send::<Loan>(db).await?;
                    worker.recv::<OwnerDone>().await?;
                    Ok::<_, EndpointError>(())
                },
                async {
                    for _ in 0..2 {
                        service.recv::<Head>().await?;
                        for _ in 0..2 {
                            let label = service.offer().await?.label();
                            match label {
                                1 | 2 => {
                                    let [mut a, mut b] = service.par()?;
                                    tokio::try_join!(
                                        async {
                                            for i in 0..128u32 {
                                                let value = a.recv::<A>().await?;
                                                assert_eq!(value, i);
                                                a.send::<A>(&(value + 1)).await?;
                                            }
                                            Ok::<_, EndpointError>(())
                                        },
                                        async {
                                            for i in 0..128u32 {
                                                let value = b.recv::<B>().await?;
                                                assert_eq!(value, i);
                                                b.send::<B>(&(value + 1)).await?;
                                            }
                                            Ok::<_, EndpointError>(())
                                        },
                                    )?;
                                }
                                3 | 4 => {
                                    let [mut a, mut b] = service.par()?;
                                    tokio::try_join!(
                                        async {
                                            for i in 0..128u32 {
                                                let value = a.recv::<C>().await?;
                                                assert_eq!(value, i);
                                                a.send::<C>(&(value + 1)).await?;
                                            }
                                            Ok::<_, EndpointError>(())
                                        },
                                        async {
                                            for i in 0..128u32 {
                                                let value = b.recv::<D>().await?;
                                                assert_eq!(value, i);
                                                b.send::<D>(&(value + 1)).await?;
                                            }
                                            Ok::<_, EndpointError>(())
                                        },
                                    )?;
                                }
                                _ => panic!("unexpected service branch"),
                            }
                        }
                    }
                    service.recv::<ServiceDone>().await?;
                    Ok::<_, EndpointError>(())
                },
            )
        })
        .await
        .expect("mixed capacity protocol stalled")
        .unwrap();
        assert!(network.is_empty());
        assert_eq!((network.sent.get(), network.received.get()), (3080, 3080));
        assert!(
            network.parallel_receivers.get() >= 2,
            "the delegated lanes must both reach Pending"
        );
        assert_eq!((resource_choices.1.get(), worker_choices.1.get()), (4, 4));
    }
}
