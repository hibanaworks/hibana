//! Cost of repeated request/reply operations with different roll body sizes.
#[path = "support/callgrind.rs"]
mod callgrind;
#[path = "../examples/support/in_memory.rs"]
mod in_memory;
#[path = "support/protocol_repetition.rs"]
mod protocol_repetition;
use protocol_repetition::repeated;

use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};
use in_memory::InMemoryTransport;
use std::{hint::black_box, time::Instant};

type Request = Msg<1, u32>;
type Reply = Msg<2, u32>;
const ROUNDS: u32 = 128;
const OPERATIONS: u32 = 4 * ROUNDS;

async fn measure(body_rounds: u32, programs: &(RoleProgram<0>, RoleProgram<1>), samples: usize) {
    let mut durations = Vec::with_capacity(samples);
    for sample in 0..samples + 3 {
        let mut slab = [0u8; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, InMemoryTransport::new([]))
            .unwrap();
        let sid = SessionId::new(1712);
        let mut a = rv.enter(sid, &programs.0).unwrap();
        let mut b = rv.enter(sid, &programs.1).unwrap();
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
        if sample >= 3 {
            durations.push(elapsed.as_nanos() as f64 / f64::from(OPERATIONS));
        }
    }
    durations.sort_by(f64::total_cmp);
    println!(
        "roll\t{body_rounds}\t{}\t{OPERATIONS}\t{samples}\t{:.1}\t{:.1}",
        ROUNDS / body_rounds,
        durations[0],
        durations[samples / 2],
    );
}

fn main() {
    let samples = std::env::var("HIBANA_PERF_SAMPLES")
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(30);
    assert!(samples > 0);
    let body = std::env::var("HIBANA_PERF_BODY").unwrap_or_else(|_| "all".into());
    assert!(matches!(body.as_str(), "all" | "1" | "8" | "32"));
    let body1 = repeated!(g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()); 1).roll();
    let body8 = repeated!(g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()); 8).roll();
    let body32 = repeated!(g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()); 32).roll();
    let programs = [
        (1, (project(&body1), project(&body1))),
        (8, (project(&body8), project(&body8))),
        (32, (project(&body32), project(&body32))),
    ];
    println!(
        "case\tbody_round_trips\tvisits\tendpoint_ops\tsamples\tbest_ns_per_op\tmedian_ns_per_op"
    );
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            for (size, programs) in &programs {
                if body == "all" || body.parse::<u32>().unwrap() == *size {
                    measure(*size, programs, samples).await;
                }
            }
        });
}
