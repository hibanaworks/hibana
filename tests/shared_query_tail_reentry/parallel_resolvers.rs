//! Parallel policy decisions remain runnable after another lane progresses.
use hibana::{
    g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverRef},
    },
};
type RequestA = g::Msg<1, ()>;
type MaterializeA = g::Msg<2, ()>;
type DoneA = g::Msg<3, ()>;
type RejectA = g::Msg<4, ()>;
type RequestB = g::Msg<11, ()>;
type MaterializeB = g::Msg<12, ()>;
type DoneB = g::Msg<13, ()>;
type RejectB = g::Msg<14, ()>;
#[tokio::test(flavor = "current_thread")]
async fn pending_local_resolvers_survive_other_parallel_requests() {
    let a = g::route(
        g::seq(
            g::send::<1, 0, RequestA>(),
            g::route(
                g::seq(g::send::<0, 0, MaterializeA>(), g::send::<0, 1, DoneA>()),
                g::send::<0, 1, RejectA>(),
            )
            .resolve::<1>(),
        ),
        g::seq(
            g::send::<1, 0, g::Msg<21, ()>>(),
            g::send::<0, 1, g::Msg<22, ()>>(),
        ),
    )
    .roll();
    let b = g::route(
        g::seq(
            g::send::<2, 0, RequestB>(),
            g::route(
                g::seq(g::send::<0, 0, MaterializeB>(), g::send::<0, 2, DoneB>()),
                g::send::<0, 2, RejectB>(),
            )
            .resolve::<2>(),
        ),
        g::seq(
            g::send::<2, 0, g::Msg<23, ()>>(),
            g::send::<0, 2, g::Msg<24, ()>>(),
        ),
    )
    .roll();
    let c = g::route(
        g::seq(
            g::send::<3, 0, g::Msg<31, ()>>(),
            g::route(
                g::seq(
                    g::send::<0, 0, g::Msg<32, ()>>(),
                    g::send::<0, 3, g::Msg<33, ()>>(),
                ),
                g::send::<0, 3, g::Msg<34, ()>>(),
            )
            .resolve::<3>(),
        ),
        g::seq(
            g::send::<3, 0, g::Msg<41, ()>>(),
            g::send::<0, 3, g::Msg<42, ()>>(),
        ),
    )
    .roll();
    let global = g::par(a, g::par(b, c));
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let mut slab = vec![0; 1024 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage.init();
    let carrier = crate::common::TestTransport::new();
    let rv = kit.rendezvous(&mut slab, carrier).unwrap();
    rv.set_resolver(
        &p0,
        ResolverRef::<1>::decision_state(&(), |_| Ok(DecisionArm::Left)),
    )
    .unwrap();
    rv.set_resolver(
        &p0,
        ResolverRef::<2>::decision_state(&(), |_| Ok(DecisionArm::Left)),
    )
    .unwrap();
    rv.set_resolver(
        &p0,
        ResolverRef::<3>::decision_state(&(), |_| Ok(DecisionArm::Left)),
    )
    .unwrap();
    let mut repo = rv.enter(SessionId::new(1), &p0).unwrap();
    let mut a = rv.enter(SessionId::new(1), &p1).unwrap();
    let mut b = rv.enter(SessionId::new(1), &p2).unwrap();
    let mut c = rv.enter(SessionId::new(1), &p3).unwrap();
    a.send::<RequestA>(&()).await.unwrap();
    b.send::<RequestB>(&()).await.unwrap();
    c.send::<g::Msg<31, ()>>(&()).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        tokio::join!(
            async {
                for _ in 0..6 {
                    let branch = repo.offer().await.unwrap();
                    match branch.label() {
                        1 => branch.recv::<RequestA>().await.unwrap(),
                        11 => branch.recv::<RequestB>().await.unwrap(),
                        2 => {
                            branch.recv::<MaterializeA>().await.unwrap();
                            repo.send::<DoneA>(&()).await.unwrap();
                        }
                        12 => {
                            branch.recv::<MaterializeB>().await.unwrap();
                            repo.send::<DoneB>(&()).await.unwrap();
                        }
                        31 => branch.recv::<g::Msg<31, ()>>().await.unwrap(),
                        32 => {
                            branch.recv::<g::Msg<32, ()>>().await.unwrap();
                            repo.send::<g::Msg<33, ()>>(&()).await.unwrap();
                        }
                        label => panic!("unexpected {label}"),
                    }
                }
            },
            async {
                a.recv::<DoneA>().await.unwrap();
                b.recv::<DoneB>().await.unwrap();
                c.recv::<g::Msg<33, ()>>().await.unwrap();
            }
        );
    })
    .await
    .expect("parallel resolver work must not wait for new ingress");
}
