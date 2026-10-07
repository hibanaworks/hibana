mod common;
mod control {
    use hibana::g;
    pub type Plain = g::Msg<238, ()>;
    pub type PlainSink = g::Msg<240, ()>;
    pub type StartupSettled = g::Msg<242, ()>;
}
use hibana::{
    g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
type Data = g::Msg<6, u64>;
type Ack = g::Msg<7, u64>;
type End = g::Msg<9, ()>;
type Fin = g::Msg<8, u64>;
type Failed = g::Msg<217, u64>;
type Interrupted = g::Msg<219, u64>;
type Retired = g::Msg<10, ()>;
type Reclaim = g::Msg<193, u64>;
type NoReclaim = g::Msg<195, u64>;
type Stored = g::Msg<194, u64>;
type Done = g::Msg<196, ()>;
type Closed = g::Msg<197, ()>;
#[test]
fn offer_observes_delayed_receive_in_parallel_with_local_send_route() {
    let packets = g::seq(
        g::route(
            g::seq(
                g::send::<10, 11, Data>(),
                g::route(
                    g::send::<11, 10, Ack>(),
                    g::route(
                        g::send::<11, 10, Fin>(),
                        g::route(
                            g::send::<11, 10, Failed>(),
                            g::send::<11, 10, Interrupted>(),
                        ),
                    ),
                ),
            ),
            g::send::<10, 11, End>(),
        )
        .roll(),
        g::send::<11, 10, Retired>(),
    );
    let receipts = g::route(
        g::seq(
            g::route(g::send::<11, 28, Reclaim>(), g::send::<11, 28, NoReclaim>()),
            g::send::<28, 11, Stored>(),
        ),
        g::seq(g::send::<11, 28, Done>(), g::send::<28, 11, Closed>()),
    )
    .roll();
    let body = g::par(packets, receipts);
    let production = g::par(
        g::send::<8, 9, g::Msg<168, ()>>(),
        g::send::<9, 27, g::Msg<189, u64>>(),
    );
    let global = g::par(
        g::par(production, body),
        g::route(
            g::seq(
                g::send::<8, 19, control::Plain>(),
                g::seq(
                    g::send::<19, 11, control::PlainSink>(),
                    g::send::<19, 8, control::StartupSettled>(),
                ),
            ),
            g::seq(
                g::send::<8, 19, g::Msg<239, ()>>(),
                g::seq(
                    g::send::<19, 11, g::Msg<241, ()>>(),
                    g::send::<19, 8, control::StartupSettled>(),
                ),
            ),
        ),
    );
    let a: RoleProgram<8> = project(&global);
    let b: RoleProgram<10> = project(&global);
    let c: RoleProgram<11> = project(&global);
    let d: RoleProgram<19> = project(&global);
    let e: RoleProgram<28> = project(&global);
    let carrier = common::TestTransport::new();
    let mut slab = vec![0; 65536];
    let mut kit = SessionKitStorage::uninit();
    let sid = SessionId::new(1);
    let rendezvous = kit.init().rendezvous(&mut slab, carrier).unwrap();
    let mut a = rendezvous.enter(sid, &a).unwrap();
    let mut b = rendezvous.enter(sid, &b).unwrap();
    let mut c = rendezvous.enter(sid, &c).unwrap();
    let mut d = rendezvous.enter(sid, &d).unwrap();
    let mut e = rendezvous.enter(sid, &e).unwrap();
    let source = async {
        a.send::<control::Plain>(&())
            .await
            .map_err(|e| format!("{e:?}"))?;
        println!("source plain sent");
        a.recv::<control::StartupSettled>()
            .await
            .map_err(|e| format!("{e:?}"))?;
        println!("source startup settled");
        Ok::<_, String>(())
    };
    let sink = async {
        c.recv::<control::PlainSink>()
            .await
            .map_err(|e| format!("{e:?}"))?;
        println!("sink plain received");
        loop {
            let o = c.offer().await.map_err(|e| format!("{e:?}"))?;
            match o.label() {
                6 => {
                    let v = o.recv::<Data>().await.map_err(|e| format!("{e:?}"))?;
                    println!("sink data received");
                    c.send::<Ack>(&v).await.map_err(|e| format!("{e:?}"))?;
                }
                9 => {
                    o.recv::<End>().await.map_err(|e| format!("{e:?}"))?;
                    c.send::<Done>(&()).await.map_err(|e| format!("{e:?}"))?;
                    c.recv::<Closed>().await.map_err(|e| format!("{e:?}"))?;
                    c.send::<Retired>(&()).await.map_err(|e| format!("{e:?}"))?;
                    break;
                }
                _ => panic!("label"),
            }
        }
        Ok::<_, String>(())
    };
    let receive = async {
        for _ in 0..5 {
            // This test peer delays the physical arrival by one executor turn.
            // Unlike the offer path, the finite delay must schedule itself.
            std::future::poll_fn(|cx| {
                cx.waker().wake_by_ref();
                Poll::Ready(())
            })
            .await;
            futures::pending!();
        }
        b.send::<Data>(&0).await.map_err(|e| format!("{e:?}"))?;
        println!("receive data sent");
        b.recv::<Ack>().await.map_err(|e| format!("{e:?}"))?;
        b.send::<End>(&()).await.map_err(|e| format!("{e:?}"))?;
        b.recv::<Retired>().await.map_err(|e| format!("{e:?}"))?;
        Ok::<_, String>(())
    };
    let owner = async {
        let o = d.offer().await.map_err(|e| format!("{e:?}"))?;
        o.recv::<control::Plain>()
            .await
            .map_err(|e| format!("{e:?}"))?;
        println!("owner plain received");
        d.send::<control::PlainSink>(&())
            .await
            .map_err(|e| format!("{e:?}"))?;
        println!("owner plain sink sent");
        d.send::<control::StartupSettled>(&())
            .await
            .map_err(|e| format!("{e:?}"))?;
        println!("owner startup sent");
        Ok::<_, String>(())
    };
    let collector = async {
        e.recv::<Done>().await.map_err(|e| format!("{e:?}"))?;
        e.send::<Closed>(&()).await.map_err(|e| format!("{e:?}"))?;
        Ok::<_, String>(())
    };
    let mut all = pin!(async { futures::try_join!(source, sink, receive, owner, collector) });
    struct WakeCount(std::sync::atomic::AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: std::sync::Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &std::sync::Arc<Self>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let wakes = std::sync::Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    for _ in 0..128 {
        let before = wakes.0.load(std::sync::atomic::Ordering::SeqCst);
        if let Poll::Ready(result) = all.as_mut().poll(&mut cx) {
            assert!(result.is_ok(), "{result:?}");
            return;
        }
        assert!(
            wakes.0.load(std::sync::atomic::Ordering::SeqCst) > before,
            "the in-process peers are parked without a registered wake"
        );
    }
    panic!("parked on upstream TestTransport");
}
