//! Current-kernel measurement. Setup and tail draining are outside the timer.
//! One operation means one successful Endpoint send or receive, not one round trip.
#[path = "support/callgrind.rs"]
mod callgrind;
#[path = "../examples/support/in_memory.rs"]
mod in_memory;

use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
use in_memory::InMemoryTransport;
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

type Request = Msg<1, u32>;
type Reply = Msg<2, u32>;
type Tail = Msg<3, ()>;
const ROUNDS: u32 = 32;
const OPERATIONS: u32 = 4 * ROUNDS;

macro_rules! repeated {
    ($term:expr; 1) => { $term };
    ($term:expr; 2) => { g::seq(repeated!($term; 1), repeated!($term; 1)) };
    ($term:expr; 4) => { g::seq(repeated!($term; 2), repeated!($term; 2)) };
    ($term:expr; 8) => { g::seq(repeated!($term; 4), repeated!($term; 4)) };
    ($term:expr; 16) => { g::seq(repeated!($term; 8), repeated!($term; 8)) };
    ($term:expr; 32) => { g::seq(repeated!($term; 16), repeated!($term; 16)) };
}

macro_rules! tail {
    (1) => {
        g::send::<0, 1, Tail>()
    };
    (2) => {
        g::seq(tail!(1), tail!(1))
    };
    (4) => {
        g::seq(tail!(2), tail!(2))
    };
    (8) => {
        g::seq(tail!(4), tail!(4))
    };
    (16) => {
        g::seq(tail!(8), tail!(8))
    };
    (32) => {
        g::seq(tail!(16), tail!(16))
    };
    (64) => {
        g::seq(tail!(32), tail!(32))
    };
    (128) => {
        g::seq(tail!(64), tail!(64))
    };
    (256) => {
        g::seq(tail!(128), tail!(128))
    };
}

async fn sequential<const S: usize>(
    a_program: &RoleProgram<0>,
    b_program: &RoleProgram<1>,
) -> Duration {
    let mut slab = [0u8; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, InMemoryTransport::new([]))
        .unwrap();
    let sid = SessionId::new(1710);
    let mut a = rv.enter(sid, a_program).unwrap();
    let mut b = rv.enter(sid, b_program).unwrap();
    let start = Instant::now();
    callgrind::start();
    tokio::join!(
        async {
            for n in 0..ROUNDS {
                a.send::<Request>(&black_box(n)).await.unwrap();
                assert_eq!(a.recv::<Reply>().await.unwrap(), n + 1);
            }
        },
        async {
            for n in 0..ROUNDS {
                assert_eq!(b.recv::<Request>().await.unwrap(), n);
                b.send::<Reply>(&black_box(n + 1)).await.unwrap();
            }
        },
    );
    callgrind::stop();
    let elapsed = start.elapsed();
    tokio::join!(
        async {
            for _ in 0..S {
                a.send::<Tail>(&()).await.unwrap();
            }
        },
        async {
            for _ in 0..S {
                b.recv::<Tail>().await.unwrap();
            }
        },
    );
    elapsed
}

async fn parallel<const S: usize>(
    a_program: &RoleProgram<0>,
    b_program: &RoleProgram<1>,
) -> Duration {
    let mut slab = [0u8; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, InMemoryTransport::new([]))
        .unwrap();
    let sid = SessionId::new(1711);
    let mut a = rv.enter(sid, a_program).unwrap();
    let mut b = rv.enter(sid, b_program).unwrap();
    let elapsed;
    {
        let [mut a_send, mut a_recv] = a.par().unwrap();
        let [mut b_recv, mut b_send] = b.par().unwrap();
        let start = Instant::now();
        callgrind::start();
        tokio::join!(
            async {
                for n in 0..ROUNDS {
                    a_send.send::<Request>(&black_box(n)).await.unwrap();
                }
            },
            async {
                for n in 0..ROUNDS {
                    assert_eq!(a_recv.recv::<Reply>().await.unwrap(), n + 1);
                }
            },
            async {
                for n in 0..ROUNDS {
                    assert_eq!(b_recv.recv::<Request>().await.unwrap(), n);
                }
            },
            async {
                for n in 0..ROUNDS {
                    b_send.send::<Reply>(&black_box(n + 1)).await.unwrap();
                }
            },
        );
        callgrind::stop();
        elapsed = start.elapsed();
    }
    tokio::join!(
        async {
            for _ in 0..S {
                a.send::<Tail>(&()).await.unwrap();
            }
        },
        async {
            for _ in 0..S {
                b.recv::<Tail>().await.unwrap();
            }
        },
    );
    elapsed
}

async fn measure<const S: usize>(
    seq: &(RoleProgram<0>, RoleProgram<1>),
    par: &(RoleProgram<0>, RoleProgram<1>),
    samples: usize,
    filter: &str,
) {
    for (name, programs) in [("seq", seq), ("par", par)] {
        if filter != "all" && filter != name {
            continue;
        }
        let mut durations = Vec::with_capacity(samples);
        for sample in 0..samples + 3 {
            let duration = if name == "seq" {
                sequential::<S>(&programs.0, &programs.1).await
            } else {
                parallel::<S>(&programs.0, &programs.1).await
            };
            if sample >= 3 {
                durations.push(duration.as_nanos() as f64 / f64::from(OPERATIONS));
            }
        }
        durations.sort_by(f64::total_cmp);
        println!(
            "{name}\t{S}\t{OPERATIONS}\t{samples}\t{:.1}\t{:.1}",
            durations[0],
            durations[samples / 2]
        );
    }
}

fn main() {
    let samples = std::env::var("HIBANA_PERF_SAMPLES")
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(30);
    assert!(samples > 0);
    let filter = std::env::var("HIBANA_PERF_CASE").unwrap_or_else(|_| "all".into());
    let size = std::env::var("HIBANA_PERF_TAIL").unwrap_or_else(|_| "all".into());
    let seq8 = g::seq(
        repeated!(g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()); 32),
        tail!(8),
    );
    let seq256 = g::seq(
        repeated!(g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()); 32),
        tail!(256),
    );
    let par8 = g::seq(
        g::par(
            repeated!(g::send::<0, 1, Request>(); 32),
            repeated!(g::send::<1, 0, Reply>(); 32),
        ),
        tail!(8),
    );
    let par256 = g::seq(
        g::par(
            repeated!(g::send::<0, 1, Request>(); 32),
            repeated!(g::send::<1, 0, Reply>(); 32),
        ),
        tail!(256),
    );
    let seq8 = (project(&seq8), project(&seq8));
    let seq256 = (project(&seq256), project(&seq256));
    let par8 = (project(&par8), project(&par8));
    let par256 = (project(&par256), project(&par256));
    println!("case\ttail_events\tendpoint_ops\tsamples\tbest_ns_per_op\tmedian_ns_per_op");
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            if size == "all" || size == "8" {
                measure::<8>(&seq8, &par8, samples, &filter).await;
            }
            if size == "all" || size == "256" {
                measure::<256>(&seq256, &par256, samples, &filter).await;
            }
        });
}
