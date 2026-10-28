use super::*;
use crate::runtime::resolver::{DecisionArm, ResolverRef};

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn delegated_resolver_starts_before_the_holders_later_send() {
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type A = g::Msg<1, ()>;
    type B = g::Msg<2, ()>;
    type C = g::Msg<3, ()>;
    type D = g::Msg<4, ()>;
    type E = g::Msg<5, ()>;
    type F = g::Msg<6, ()>;
    type G = g::Msg<7, ()>;
    type H = g::Msg<8, ()>;
    type K = g::Msg<9, ()>;
    type L = g::Msg<10, ()>;
    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::route(
                g::seq(
                    g::send::<3, 2, A>(),
                    g::seq(
                        g::send::<3, 0, B>(),
                        g::route(
                            g::seq(
                                g::send::<3, 2, C>(),
                                g::seq(
                                    g::send::<3, 0, D>(),
                                    g::seq(g::send::<1, 2, K>(), g::send::<2, 3, L>()),
                                ),
                            ),
                            g::seq(g::send::<3, 2, E>(), g::send::<3, 0, F>()),
                        ),
                    ),
                ),
                g::seq(g::send::<3, 2, G>(), g::send::<3, 0, H>()),
            )
            .resolve::<0>(),
            g::send::<1, 0, Loan<'_>>(),
        ),
    );
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let sid = SessionId::new(923);
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
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let db = rv0.enter(sid, &p3).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut service = rv2.enter(sid, &p2).unwrap();
    tokio::time::timeout(core::time::Duration::from_secs(1), async {
        tokio::try_join!(
            async {
                owner.send::<Loan<'_>>(db).await?;
                owner.offer().await?.recv::<B>().await?;
                owner.offer().await?.recv::<D>().await?;
                let _db = owner.recv::<Loan<'_>>().await?;
                Ok::<_, crate::EndpointError>(())
            },
            async {
                let mut db = worker.recv::<Loan<'_>>().await?;
                let branch = db.offer().await?;
                assert_eq!(branch.label(), 1);
                drop(branch);
                db.offer().await?.send::<A>(&()).await?;
                db.send::<B>(&()).await?;
                db.send::<C>(&()).await?;
                db.send::<D>(&()).await?;
                worker.send::<K>(&()).await?;
                db.recv::<L>().await?;
                worker.send::<Loan<'_>>(db).await
            },
            async {
                service.offer().await?.recv::<A>().await?;
                service.offer().await?.recv::<C>().await?;
                service.recv::<K>().await?;
                service.send::<L>(&()).await
            }
        )
    })
    .await
    .expect("delegated resolver stalled")
    .unwrap();
    assert!(network.queue_is_empty());
}
