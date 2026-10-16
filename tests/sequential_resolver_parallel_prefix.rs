use super::common;
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
fn next_resolver_preserves_a_parallel_rolled_prefix() {
    use core::{future::Future, pin::pin, task::Poll};
    use std::{future::poll_fn, time::Duration};
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            const C: u8 = 0;
            const D: u8 = 1;
            const A: u8 = 2;
            const B: u8 = 3;
            type Start = g::Msg<1, ()>;
            type AcceptA = g::Msg<2, ()>;
            type BeginB = g::Msg<3, ()>;
            type BeginDocB = g::Msg<4, u32>;
            type AcceptB = g::Msg<5, ()>;
            type BeginCombine = g::Msg<6, u32>;
            type ReviseA = g::Msg<7, ()>;
            type WaitB = g::Msg<8, ()>;
            type ReviseDocA = g::Msg<9, ()>;
            type ReviseB = g::Msg<10, ()>;
            type ReviseDocB = g::Msg<11, u32>;
            type ReadyB = g::Msg<12, ()>;
            type Combined = g::Msg<13, u32>;
            let first = g::route(
                g::seq(
                    g::send::<C, A, AcceptA>(),
                    g::par(g::send::<C, B, BeginB>(), g::send::<C, D, BeginDocB>()),
                ),
                g::seq(
                    g::par(g::send::<C, A, ReviseA>(), g::send::<C, B, WaitB>()),
                    g::send::<C, D, ReviseDocA>(),
                ),
            )
            .resolve::<801>()
            .roll();
            let second = g::route(
                g::seq(
                    g::send::<C, B, AcceptB>(),
                    g::seq(g::send::<C, D, BeginCombine>(), g::send::<C, D, Combined>()),
                ),
                g::seq(g::send::<C, B, ReviseB>(), g::send::<C, D, ReviseDocB>()),
            )
            .resolve::<802>()
            .roll();
            let global = g::seq(
                g::send::<C, D, Start>(),
                g::seq(g::par(first, g::send::<B, C, ReadyB>()), second),
            );
            let pc = project::<C>(&global);
            let pd = project::<D>(&global);
            let pb = project::<B>(&global);
            let choice = Cell::new(true);
            let mut slab = vec![0; 65536];
            let mut storage = SessionKitStorage::uninit();
            let wire = common::TestTransport::new();
            let rv = storage.init().rendezvous(&mut slab, wire.clone()).unwrap();
            rv.set_resolver(&pc, ResolverRef::<801>::decision_state(&choice, decide))
                .unwrap();
            rv.set_resolver(&pc, ResolverRef::<802>::decision_state(&choice, decide))
                .unwrap();
            let session = SessionId::new(91);
            let mut c = rv.enter(session, &pc).unwrap();
            let mut d = rv.enter(session, &pd).unwrap();
            let mut b = rv.enter(session, &pb).unwrap();
            c.send::<Start>(&()).await.unwrap();
            c.send::<AcceptA>(&()).await.unwrap();
            c.send::<BeginB>(&()).await.unwrap();
            c.send::<BeginDocB>(&17).await.unwrap();
            b.send::<ReadyB>(&()).await.unwrap();
            c.recv::<ReadyB>().await.unwrap();
            c.send::<AcceptB>(&()).await.unwrap();
            c.send::<BeginCombine>(&29).await.unwrap();
            c.send::<Combined>(&41).await.unwrap();
            d.recv::<Start>().await.unwrap();
            // Two ordered future receives reuse the same wire key. Neither
            // becomes an enabled match while the other lane's prefix is held.
            let deliver_prefix = wire.hold_next_frame(session, D, 1);
            {
                let mut offered = pin!(d.offer());
                for _ in 0..3 {
                    poll_fn(|cx| {
                        assert!(matches!(offered.as_mut().poll(cx), Poll::Pending));
                        Poll::Ready(())
                    })
                    .await;
                }
                let (branch, ()) = tokio::time::timeout(Duration::from_secs(1), async {
                    tokio::join!(offered, async {
                        tokio::task::yield_now().await;
                        deliver_prefix();
                    })
                })
                .await
                .expect("delayed prefix did not wake the pending offer");
                let branch = branch.unwrap();
                assert_eq!(branch.label(), 4);
                drop(branch);
            }
            let branch = d.offer().await.unwrap();
            assert_eq!(
                branch.label(),
                4,
                "later resolver skipped the rolled prefix"
            );
            assert_eq!(branch.recv::<BeginDocB>().await.unwrap(), 17);
            let branch = d.offer().await.unwrap();
            assert_eq!(branch.label(), 6);
            assert_eq!(branch.recv::<BeginCombine>().await.unwrap(), 29);
            assert_eq!(d.recv::<Combined>().await.unwrap(), 41);
        });
}
