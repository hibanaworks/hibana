//! A completed enclosing repeat leaves its parallel body available for borrowing.
mod common;

use hibana::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};

#[test]
fn parallel_borrows_do_not_require_restarting_the_enclosing_prefix() {
    type Prefix = g::Msg<231, ()>;
    type Left = g::Msg<232, ()>;
    type Right = g::Msg<233, ()>;
    let global = g::seq(
        g::send::<0, 1, Prefix>(),
        g::par(g::send::<0, 1, Left>(), g::send::<0, 2, Right>()).roll(),
    )
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let network = common::TestTransport::new();
    let [mut slab0, mut slab1, mut slab2] = [[0; 16 * 1024]; 3];
    let mut storage0 = SessionKitStorage::uninit();
    let mut storage1 = SessionKitStorage::uninit();
    let mut storage2 = SessionKitStorage::uninit();
    let rv0 = storage0
        .init()
        .rendezvous(&mut slab0, network.clone())
        .unwrap();
    let rv1 = storage1
        .init()
        .rendezvous(&mut slab1, network.clone())
        .unwrap();
    let rv2 = storage2
        .init()
        .rendezvous(&mut slab2, network.clone())
        .unwrap();
    let sid = SessionId::new(9284);
    let mut a = rv0.enter(sid, &p0).unwrap();
    let mut b = rv1.enter(sid, &p1).unwrap();
    let mut c = rv2.enter(sid, &p2).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                assert!(a.par::<2>().is_err(), "the unfinished prefix is required");
                for _ in 0..2 {
                    a.send::<Prefix>(&()).await.unwrap();
                    b.recv::<Prefix>().await.unwrap();
                    for _ in 0..2 {
                        let [mut left, mut right] = a.par().unwrap();
                        let (sent_left, sent_right, received_left, received_right) = tokio::join!(
                            left.send::<Left>(&()),
                            right.send::<Right>(&()),
                            b.recv::<Left>(),
                            c.recv::<Right>(),
                        );
                        sent_left.unwrap();
                        sent_right.unwrap();
                        received_left.unwrap();
                        received_right.unwrap();
                    }
                    // Borrow construction and destruction leave the completed
                    // visit unchanged; the following Prefix starts the outer one.
                    let [left, right] = a.par().unwrap();
                    drop((left, right));
                }
            })
            .await
            .expect("independent parallel operations stalled");
        });
    assert!(network.queue_is_empty());
}
