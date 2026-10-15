mod common;
use core::cell::Cell;
use hibana::{
    g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverError, ResolverRef},
    },
};

fn decide(input: &Cell<bool>) -> Result<DecisionArm, ResolverError> {
    Ok(if input.get() {
        DecisionArm::Left
    } else {
        DecisionArm::Right
    })
}

#[test]
fn inner_reentry_does_not_restart_completed_containing_resolver() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            type AcceptA = g::Msg<1, ()>;
            type ReviseA = g::Msg<2, ()>;
            type ResubmitA = g::Msg<3, ()>;
            type AcceptB = g::Msg<4, ()>;
            type ReviseB = g::Msg<5, ()>;
            type ResubmitB = g::Msg<6, ()>;
            let inner = g::route(
                g::send::<0, 1, AcceptB>(),
                g::seq(g::send::<0, 1, ReviseB>(), g::send::<1, 0, ResubmitB>()),
            )
            .resolve::<802>()
            .roll();
            let global = g::route(
                g::seq(g::send::<0, 1, AcceptA>(), inner),
                g::seq(g::send::<0, 1, ReviseA>(), g::send::<1, 0, ResubmitA>()),
            )
            .resolve::<801>()
            .roll();
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let a = Cell::new(false);
            let b = Cell::new(false);
            let mut bytes = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut bytes, common::TestTransport::new())
                .unwrap();
            rv.set_resolver(&p0, ResolverRef::<801>::decision_state(&a, decide))
                .unwrap();
            rv.set_resolver(&p0, ResolverRef::<802>::decision_state(&b, decide))
                .unwrap();
            let mut controller = rv.enter(SessionId::new(51), &p0).unwrap();
            let mut observer = rv.enter(SessionId::new(51), &p1).unwrap();
            for _ in 0..3 {
                a.set(false);
                controller.send::<ReviseA>(&()).await.unwrap();
                observer
                    .offer()
                    .await
                    .unwrap()
                    .recv::<ReviseA>()
                    .await
                    .unwrap();
                observer.send::<ResubmitA>(&()).await.unwrap();
                controller.recv::<ResubmitA>().await.unwrap();
                a.set(true);
                controller.send::<AcceptA>(&()).await.unwrap();
                observer
                    .offer()
                    .await
                    .unwrap()
                    .recv::<AcceptA>()
                    .await
                    .unwrap();
                b.set(false);
                for _ in 0..2 {
                    controller.send::<ReviseB>(&()).await.unwrap();
                    observer
                        .offer()
                        .await
                        .unwrap()
                        .recv::<ReviseB>()
                        .await
                        .unwrap();
                    observer.send::<ResubmitB>(&()).await.unwrap();
                    controller.recv::<ResubmitB>().await.unwrap();
                }
                // A child's new visit must keep its containing choice even if the
                // parent's resolver input changes after that choice committed.
                a.set(false);
                b.set(true);
                controller.send::<AcceptB>(&()).await.unwrap();
                observer
                    .offer()
                    .await
                    .unwrap()
                    .recv::<AcceptB>()
                    .await
                    .unwrap();
            }
        });
}

#[test]
fn rolled_service_requests_resume_after_review_rework_while_another_flow_is_idle() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            type Open = g::Msg<20, u32>;
            type Opened = g::Msg<21, ()>;
            type Commit = g::Msg<22, ()>;
            type Materialize = g::Msg<23, ()>;
            type Completed = g::Msg<24, ()>;
            type Rejected = g::Msg<25, ()>;
            type ReviewRequested = g::Msg<26, ()>;
            type Ready = g::Msg<27, ()>;
            type Rework = g::Msg<28, ()>;
            type Recorded = g::Msg<29, ()>;
            type Revise = g::Msg<30, ()>;
            type Approve = g::Msg<31, ()>;
            type Done = g::Msg<32, ()>;
            type Begin = g::Msg<33, ()>;
            type Other = g::Msg<34, ()>;
            type OtherEnd = g::Msg<35, ()>;
            let requests = g::seq(
                g::send::<0, 0, Begin>(),
                g::route(
                    g::seq(g::send::<0, 1, Open>(), g::send::<1, 0, Opened>()),
                    g::seq(
                        g::send::<0, 1, Commit>(),
                        g::route(
                            g::seq(g::send::<1, 1, Materialize>(), g::send::<1, 0, Completed>()),
                            g::send::<1, 0, Rejected>(),
                        )
                        .resolve::<803>(),
                    ),
                ),
            )
            .roll();
            let work = g::seq(
                g::par(requests, g::send::<2, 0, ReviewRequested>()),
                g::seq(
                    g::send::<0, 2, Ready>(),
                    g::route(
                        g::seq(
                            g::send::<2, 1, Approve>(),
                            g::seq(g::send::<1, 2, Recorded>(), g::send::<2, 0, Done>()),
                        ),
                        g::seq(
                            g::send::<2, 1, Rework>(),
                            g::seq(g::send::<1, 2, Recorded>(), g::send::<2, 0, Revise>()),
                        ),
                    )
                    .resolve::<804>(),
                ),
            )
            .roll();
            let global = g::par(
                work,
                g::route(g::send::<3, 1, Other>(), g::send::<3, 1, OtherEnd>()).roll(),
            );
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let p2 = project::<2>(&global);
            let p3 = project::<3>(&global);
            let valid = Cell::new(true);
            let accepted = Cell::new(false);
            let mut bytes = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut bytes, common::TestTransport::new())
                .unwrap();
            rv.set_resolver(&p1, ResolverRef::<803>::decision_state(&valid, decide))
                .unwrap();
            rv.set_resolver(&p2, ResolverRef::<804>::decision_state(&accepted, decide))
                .unwrap();
            let mut worker = rv.enter(SessionId::new(52), &p0).unwrap();
            let mut repo = rv.enter(SessionId::new(52), &p1).unwrap();
            let mut reviewer = rv.enter(SessionId::new(52), &p2).unwrap();
            let mut other = rv.enter(SessionId::new(52), &p3).unwrap();
            other.send::<Other>(&()).await.unwrap();
            repo.offer().await.unwrap().recv::<Other>().await.unwrap();
            for value in 0..3 {
                worker.send::<Begin>(&()).await.unwrap();
                worker.send::<Open>(&value).await.unwrap();
                let branch = tokio::time::timeout(core::time::Duration::from_secs(1), repo.offer())
                    .await
                    .unwrap_or_else(|_| panic!("ready request hidden after rework, round {value}"))
                    .unwrap();
                assert_eq!(branch.recv::<Open>().await.unwrap(), value);
                repo.send::<Opened>(&()).await.unwrap();
                worker.recv::<Opened>().await.unwrap();
                worker.send::<Begin>(&()).await.unwrap();
                worker.send::<Commit>(&()).await.unwrap();
                repo.offer().await.unwrap().recv::<Commit>().await.unwrap();
                repo.offer()
                    .await
                    .unwrap()
                    .recv::<Materialize>()
                    .await
                    .unwrap();
                repo.send::<Completed>(&()).await.unwrap();
                worker.recv::<Completed>().await.unwrap();
                reviewer.send::<ReviewRequested>(&()).await.unwrap();
                worker.recv::<ReviewRequested>().await.unwrap();
                worker.send::<Ready>(&()).await.unwrap();
                reviewer.recv::<Ready>().await.unwrap();
                reviewer.send::<Rework>(&()).await.unwrap();
                repo.offer().await.unwrap().recv::<Rework>().await.unwrap();
                repo.send::<Recorded>(&()).await.unwrap();
                reviewer.recv::<Recorded>().await.unwrap();
                reviewer.send::<Revise>(&()).await.unwrap();
                worker.recv::<Revise>().await.unwrap();
            }
        });
}
