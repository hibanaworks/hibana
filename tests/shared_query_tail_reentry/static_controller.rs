//! A committed send choice still authorizes its following receive choice.
use hibana::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};
type Left = g::Msg<81, ()>;
type Right = g::Msg<82, ()>;
type Accepted = g::Msg<83, ()>;
type Stopped = g::Msg<84, ()>;
type Rejected = g::Msg<85, ()>;

#[test]
fn completed_send_choice_keeps_its_receive_continuation() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let global = g::seq(
                g::route(g::send::<0, 1, Left>(), g::send::<0, 1, Right>()),
                g::route(
                    g::send::<1, 0, Accepted>(),
                    g::route(g::send::<1, 0, Stopped>(), g::send::<1, 0, Rejected>()),
                ),
            )
            .roll();
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let mut slab = vec![0; 65536];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, crate::common::TestTransport::new())
                .unwrap();
            let mut source = rv.enter(SessionId::new(1), &p0).unwrap();
            let mut receiver = rv.enter(SessionId::new(1), &p1).unwrap();
            // The Miri process has an external watchdog. Interpreter speed
            // must not become a deadline in this in-memory protocol test.
            tokio::join!(
                async {
                    source.send::<Left>(&()).await.unwrap();
                    source
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Stopped>()
                        .await
                        .unwrap();
                    source.send::<Right>(&()).await.unwrap();
                    source
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Accepted>()
                        .await
                        .unwrap();
                },
                async {
                    receiver
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Left>()
                        .await
                        .unwrap();
                    receiver.send::<Stopped>(&()).await.unwrap();
                    receiver
                        .offer()
                        .await
                        .unwrap()
                        .recv::<Right>()
                        .await
                        .unwrap();
                    receiver.send::<Accepted>(&()).await.unwrap();
                }
            );
        });
}
