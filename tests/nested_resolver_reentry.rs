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
