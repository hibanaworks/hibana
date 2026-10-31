//! Native test adapter for the no_std cursor counters. Coordination is executed
//! by actual Endpoints; formatting happens after the measured operations.
#[path = "../../examples/support/in_memory.rs"]
mod in_memory;
#[path = "../../benches/support/protocol_repetition.rs"]
mod protocol_repetition;
use in_memory::InMemoryTransport;

use super::proof_snapshot::{view_tag, work_counts};
use crate::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
use protocol_repetition::repeated;

type Request = Msg<1, u32>;
type Reply = Msg<2, u32>;
type Tail = Msg<3, ()>;

#[test]
fn work_observation_preserves_live_endpoint_progress() {
    let global = g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>());
    let programs: (RoleProgram<0>, RoleProgram<1>) = (project(&global), project(&global));
    let network = InMemoryTransport::new([]);
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1709);
    let mut a = rv.enter(sid, &programs.0).unwrap();
    let mut b = rv.enter(sid, &programs.1).unwrap();
    let tag = view_tag(&a);
    // SAFETY: a retains this actor, attached with InMemoryTransport.
    let before = unsafe { work_counts::<InMemoryTransport>(tag) };
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            tokio::join!(
                async {
                    a.send::<Request>(&41).await.unwrap();
                    assert_eq!(a.recv::<Reply>().await.unwrap(), 42);
                },
                async {
                    let value = b.recv::<Request>().await.unwrap();
                    b.send::<Reply>(&(value + 1)).await.unwrap();
                },
            );
        });
    // SAFETY: a and its actor are still live and no future is being polled.
    let after = unsafe { work_counts::<InMemoryTransport>(tag) };
    // SAFETY: the same actor is retained for this second read.
    let again = unsafe { work_counts::<InMemoryTransport>(tag) };
    assert_eq!(after.as_ref(), again.as_ref());
    assert!(
        before
            .as_ref()
            .iter()
            .zip(after.as_ref())
            .any(|((_, first), (_, last))| first < last)
    );
    assert!(network.queue_is_empty());
}

fn report(
    case: &str,
    parameter: u32,
    operations: u32,
    run: u32,
    before: [&[(&str, u64)]; 2],
    after: [&[(&str, u64)]; 2],
) {
    assert_eq!(before[0].len(), before[1].len());
    for (index, (metric, _)) in before[0].iter().enumerate() {
        let mut total = 0u64;
        for actor in 0..2 {
            assert_eq!(before[actor].len(), after[actor].len());
            assert_eq!(before[actor][index].0, *metric);
            assert_eq!(after[actor][index].0, *metric);
            total = total
                .checked_add(
                    after[actor][index]
                        .1
                        .checked_sub(before[actor][index].1)
                        .expect("work counters decreased"),
                )
                .expect("work sum overflow");
        }
        std::println!("runtime-work\t{case}\t{parameter}\t{operations}\t{run}\t{metric}\t{total}");
    }
}

async fn measure(case: &str, parameter: u32, programs: &(RoleProgram<0>, RoleProgram<1>)) {
    let rounds = if case == "roll" { 128u32 } else { 32 };
    for run in 0..2 {
        let network = InMemoryTransport::new([]);
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, network.clone())
            .unwrap();
        let sid = SessionId::new(1710 + run);
        let mut a = rv.enter(sid, &programs.0).unwrap();
        let mut b = rv.enter(sid, &programs.1).unwrap();
        let tags = [view_tag(&a), view_tag(&b)];
        if case == "par" {
            let [mut a_send, mut a_recv] = a.par().unwrap();
            let [mut b_recv, mut b_send] = b.par().unwrap();
            // SAFETY: both InMemoryTransport actors remain pinned by these live borrows.
            let before = tags.map(|tag| unsafe { work_counts::<InMemoryTransport>(tag) });
            tokio::join!(
                async {
                    for n in 0..rounds {
                        a_send.send::<Request>(&n).await.unwrap();
                    }
                },
                async {
                    for n in 0..rounds {
                        assert_eq!(a_recv.recv::<Reply>().await.unwrap(), n + 1);
                    }
                },
                async {
                    for n in 0..rounds {
                        assert_eq!(b_recv.recv::<Request>().await.unwrap(), n);
                    }
                },
                async {
                    for n in 0..rounds {
                        b_send.send::<Reply>(&(n + 1)).await.unwrap();
                    }
                },
            );
            // SAFETY: the same actors remain alive; the synchronous observation
            // takes the existing kernel lease without retaining it across a poll.
            let after = tags.map(|tag| unsafe { work_counts::<InMemoryTransport>(tag) });
            report(
                case,
                parameter,
                rounds * 4,
                run,
                [before[0].as_ref(), before[1].as_ref()],
                [after[0].as_ref(), after[1].as_ref()],
            );
        } else {
            // SAFETY: both InMemoryTransport actors remain pinned by a and b.
            let before = tags.map(|tag| unsafe { work_counts::<InMemoryTransport>(tag) });
            tokio::join!(
                async {
                    for n in 0..rounds {
                        a.send::<Request>(&n).await.unwrap();
                        assert_eq!(a.recv::<Reply>().await.unwrap(), n + 1);
                    }
                },
                async {
                    for n in 0..rounds {
                        assert_eq!(b.recv::<Request>().await.unwrap(), n);
                        b.send::<Reply>(&(n + 1)).await.unwrap();
                    }
                },
            );
            // SAFETY: a and b still retain the same actors.
            let after = tags.map(|tag| unsafe { work_counts::<InMemoryTransport>(tag) });
            report(
                case,
                parameter,
                rounds * 4,
                run,
                [before[0].as_ref(), before[1].as_ref()],
                [after[0].as_ref(), after[1].as_ref()],
            );
        }
        if case != "roll" {
            tokio::join!(
                async {
                    for _ in 0..parameter {
                        a.send::<Tail>(&()).await.unwrap();
                    }
                },
                async {
                    for _ in 0..parameter {
                        b.recv::<Tail>().await.unwrap();
                    }
                },
            );
        }
        assert!(network.queue_is_empty());
    }
}

#[test]
fn runtime_work_profile() {
    std::println!();
    let seq8 = g::seq(
        repeated!(g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()); 32),
        repeated!(g::send::<0, 1, Tail>(); 8),
    );
    let seq256 = g::seq(
        repeated!(g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()); 32),
        repeated!(g::send::<0, 1, Tail>(); 256),
    );
    let par8 = g::seq(
        g::par(
            repeated!(g::send::<0, 1, Request>(); 32),
            repeated!(g::send::<1, 0, Reply>(); 32),
        ),
        repeated!(g::send::<0, 1, Tail>(); 8),
    );
    let par256 = g::seq(
        g::par(
            repeated!(g::send::<0, 1, Request>(); 32),
            repeated!(g::send::<1, 0, Reply>(); 32),
        ),
        repeated!(g::send::<0, 1, Tail>(); 256),
    );
    let roll1 = repeated!(g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()); 1).roll();
    let roll8 = repeated!(g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()); 8).roll();
    let roll32 = repeated!(g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()); 32).roll();
    let cases = [
        ("seq", 8, (project(&seq8), project(&seq8))),
        ("seq", 256, (project(&seq256), project(&seq256))),
        ("par", 8, (project(&par8), project(&par8))),
        ("par", 256, (project(&par256), project(&par256))),
        ("roll", 1, (project(&roll1), project(&roll1))),
        ("roll", 8, (project(&roll8), project(&roll8))),
        ("roll", 32, (project(&roll32), project(&roll32))),
    ];
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for (name, parameter, programs) in cases {
                measure(name, parameter, &programs).await;
            }
        });
}
