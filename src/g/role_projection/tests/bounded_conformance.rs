//! Enumerate atomic public-operation schedules and compare them with executable
//! Lean semantics. Scheduling data belongs only to this test, never to an app.
use super::*;
use crate::{
    g::ProgramShape,
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::{
        lean_choreo::LeanChoreo, lean_descriptor::descriptor_source, transport::TestTransport,
    },
};
use std::{format, string::String, vec, vec::Vec};

type Start = g::Msg<221, u32>;
type Ack = g::Msg<222, u32>;
type Finish = g::Msg<223, u32>;
type Body = g::Seq<g::Send<0, 1, Start>, g::Send<1, 0, Ack>>;
type Sequential = g::Seq<g::Roll<Body>, g::Send<1, 0, Finish>>;
type Parallel = g::Par<g::Roll<Body>, g::Send<1, 0, Finish>>;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Outcome {
    Accepted,
    Pending,
    Rejected(String),
}

impl Outcome {
    fn code(&self) -> u8 {
        match self {
            Self::Accepted => 0,
            Self::Pending => 1,
            Self::Rejected(_) => 2,
        }
    }
}

// Drive each operation under Tokio, including internal wakeups. The virtual
// watchdog classifies quiescence without making wall-clock speed a protocol
// condition. Cancelling the final candidate cannot poison another replay.
macro_rules! observe {
    ($operation:expr) => {{
        match tokio::time::timeout(core::time::Duration::from_millis(1), $operation).await {
            Ok(Ok(_)) => Outcome::Accepted,
            Ok(Err(error)) => Outcome::Rejected(format!("{error:?}")),
            Err(_) => Outcome::Pending,
        }
    }};
}

async fn replay<Steps: ProgramShape>(commands: &[u8]) -> Outcome {
    let a_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let b_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let network = TestTransport::new();
    let mut a_slab = [0; 16 * 1024];
    let mut b_slab = [0; 16 * 1024];
    let mut a_storage = SessionKitStorage::uninit();
    let mut b_storage = SessionKitStorage::uninit();
    let a_rv = a_storage
        .init()
        .rendezvous(&mut a_slab, network.clone())
        .unwrap();
    let b_rv = b_storage.init().rendezvous(&mut b_slab, network).unwrap();
    let sid = SessionId::new(9074);
    let mut a = a_rv.enter(sid, &a_program).unwrap();
    let mut b = b_rv.enter(sid, &b_program).unwrap();
    let mut outcome = Outcome::Accepted;
    for (index, command) in commands.iter().enumerate() {
        outcome = match command {
            0 => observe!(a.send::<Start>(&7)),
            1 => observe!(b.recv::<Start>()),
            2 => observe!(b.send::<Ack>(&7)),
            3 => observe!(a.recv::<Ack>()),
            4 => observe!(b.send::<Finish>(&7)),
            5 => observe!(a.recv::<Finish>()),
            _ => unreachable!(),
        };
        if index + 1 != commands.len() {
            assert_eq!(
                outcome,
                Outcome::Accepted,
                "successful prefix changed: {commands:?}"
            );
        }
    }
    outcome
}

async fn enumerate<Steps: ProgramShape + LeanChoreo>() -> (String, usize, usize) {
    use core::fmt::Write;
    let mut frontier = vec![Vec::<u8>::new()];
    let mut observations = String::new();
    let mut attempts = 0;
    let mut completed = 0;
    // Bounds are on tested occurrences, not extra runtime admission rules:
    // two body visits and one continuation, ten completed local operations.
    for _depth in 0..10 {
        let mut next = Vec::new();
        for prefix in frontier {
            for command in 0..6_u8 {
                let cap = if command < 4 { 2 } else { 1 };
                if prefix.iter().filter(|&&prior| prior == command).count() == cap {
                    continue;
                }
                let mut trace = prefix.clone();
                trace.push(command);
                let outcome = replay::<Steps>(&trace).await;
                let detail = format!("{outcome:?}");
                writeln!(
                    observations,
                    "{{ commands := {trace:?}, outcome := {}, runtime := {:?} }},",
                    outcome.code(),
                    detail,
                )
                .unwrap();
                attempts += 1;
                if outcome == Outcome::Accepted {
                    if trace.len() == 10 {
                        completed += 1;
                    }
                    next.push(trace);
                }
            }
        }
        frontier = next;
    }
    assert!(
        completed > 0,
        "the generator must reach complete bounded runs"
    );
    (observations, attempts, completed)
}

#[test]
fn bounded_public_schedules_emit_executable_lean_conformance_cases() {
    let (sequential, parallel) = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            (
                enumerate::<Sequential>().await,
                enumerate::<Parallel>().await,
            )
        });
    let generated = format!(
        "def main : IO Unit := checkCases [\n\
        {{ name := \"seq-roll\", choreo := {}, image := {}, attempts := [\n{}] }},\n\
        {{ name := \"par-roll\", choreo := {}, image := {}, attempts := [\n{}] }}]\n",
        Sequential::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(
            &RoleProjection::<0, Sequential, 16>::IMAGE_REF
        )),
        sequential.0,
        Parallel::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(
            &RoleProjection::<0, Parallel, 16>::IMAGE_REF
        )),
        parallel.0,
    );
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("EndpointConformanceCases.lean"), generated).unwrap();
    std::println!(
        "bounded Endpoint schedules: seq attempts={} complete={}, par attempts={} complete={}",
        sequential.1,
        sequential.2,
        parallel.1,
        parallel.2,
    );
}
