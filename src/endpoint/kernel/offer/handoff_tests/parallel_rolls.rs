use super::*;
use crate::runtime::resolver::{DecisionArm, ResolverRef};

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_delegated_view_offers_its_continuation_after_parallel_rolls_finish() {
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type Left = g::Msg<1, ()>;
    type OtherLeft = g::Msg<2, ()>;
    type LeftDone = g::Msg<3, ()>;
    type Right = g::Msg<4, ()>;
    type OtherRight = g::Msg<5, ()>;
    type RightDone = g::Msg<6, ()>;
    type Next = g::Msg<7, ()>;
    type OtherNext = g::Msg<8, ()>;
    type Returned<'r> = g::Msg<9, Endpoint<'r, 3>>;
    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::seq(
                g::par(
                    g::seq(
                        g::route(g::send::<3, 0, Left>(), g::send::<3, 0, OtherLeft>())
                            .resolve::<0>()
                            .roll(),
                        g::send::<0, 3, LeftDone>(),
                    ),
                    g::seq(
                        g::route(g::send::<3, 2, Right>(), g::send::<3, 2, OtherRight>()).roll(),
                        g::send::<2, 3, RightDone>(),
                    ),
                ),
                g::route(g::send::<3, 1, Next>(), g::send::<3, 1, OtherNext>()).resolve::<1>(),
            ),
            g::send::<1, 0, Returned<'_>>(),
        ),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let network = TestTransport::new();
    let mut slab0 = [0; 65536];
    let mut slab1 = [0; 65536];
    let mut slab2 = [0; 65536];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s2 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
    rv1.set_resolver(
        &p3,
        ResolverRef::<0>::decision_state(&(), |_| Ok(DecisionArm::Left)),
    )
    .unwrap();
    rv1.set_resolver(
        &p3,
        ResolverRef::<1>::decision_state(&(), |_| Ok(DecisionArm::Left)),
    )
    .unwrap();
    let sid = SessionId::new(932);
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let db = rv0.enter(sid, &p3).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut service = rv2.enter(sid, &p2).unwrap();
    tokio::time::timeout(core::time::Duration::from_secs(1), async {
        tokio::try_join!(
            async {
                owner.send::<Loan<'_>>(db).await?;
                {
                    let [mut left] = owner.par()?;
                    for _ in 0..2 {
                        left.offer().await?.recv::<Left>().await?;
                    }
                    left.send::<LeftDone>(&()).await?;
                }
                let _db = owner.recv::<Returned<'_>>().await?;
                Ok::<_, crate::EndpointError>(())
            },
            async {
                let mut db = worker.recv::<Loan<'_>>().await?;
                {
                    let [mut left, mut right] = db.par()?;
                    tokio::try_join!(
                        async {
                            for _ in 0..2 {
                                left.offer().await?.send::<Left>(&()).await?;
                            }
                            left.recv::<LeftDone>().await
                        },
                        async {
                            for _ in 0..2 {
                                right.send::<Right>(&()).await?;
                            }
                            right.recv::<RightDone>().await
                        }
                    )?;
                }
                let next = db.offer().await?;
                assert_eq!(next.label(), 7);
                next.send::<Next>(&()).await?;
                worker.recv::<Next>().await?;
                worker.send::<Returned<'_>>(db).await
            },
            async {
                let [mut right] = service.par()?;
                for _ in 0..2 {
                    right.offer().await?.recv::<Right>().await?;
                }
                right.send::<RightDone>(&()).await
            }
        )
    })
    .await
    .expect("parallel roll continuation stalled")
    .unwrap();
    assert!(network.queue_is_empty());
}
