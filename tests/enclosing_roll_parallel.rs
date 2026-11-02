//! A completed enclosing repeat leaves its parallel body available for borrowing.
mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use hibana::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn parallel_borrows_do_not_require_restarting_the_enclosing_prefix()
-> core::result::Result<(), hibana::EndpointError> {
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
    async {
        (async {
            assert!(a.par::<2>().is_err(), "the unfinished prefix is required");
            for _ in 0..2 {
                a.send::<Prefix>(&()).await.unwrap();
                b.recv::<Prefix>().await.unwrap();
                for _ in 0..2 {
                    let [mut left, mut right] = a.par().unwrap();
                    let (sent_left, sent_right, received_left, received_right) = async {
                        async { Ok::<_, hibana::EndpointError>((left.send::<Left>(&())).await) };

                        async { Ok::<_, hibana::EndpointError>((right.send::<Right>(&())).await) };

                        async { Ok::<_, hibana::EndpointError>((b.recv::<Left>()).await) };

                        async { Ok::<_, hibana::EndpointError>((c.recv::<Right>()).await) };
                    }
                    .await
                    .expect("all child results are data");
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
        .await;
    }
    .await;
    assert!(network.queue_is_empty());

    Ok(())
}
