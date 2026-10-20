//! One close or abort at each quiescent cut of the four gated calls. All
//! remaining I/O releases are probed; a failed candidate ends only its replay.
use super::*;

struct FaultAttempt {
    before: Vec<u8>,
    after: Vec<u8>,
    abort: bool,
    result: Attempt,
}

#[test]
#[cfg_attr(
    miri,
    ignore = "native exhaustive fault schedules; close/abort replays run separately under Miri"
)]
fn bounded_close_and_abort_schedules_emit_lean_cases() {
    use core::fmt::Write;
    let attempts = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let mut prefixes = vec![Vec::<u8>::new()];
            let mut frontier = prefixes.clone();
            for _ in 0..8 {
                let mut next = Vec::new();
                for prefix in frontier {
                    for command in 0..8 {
                        let mut trace = prefix.clone();
                        trace.push(command);
                        if replay(&trace, None).await.accepted {
                            next.push(trace);
                        }
                    }
                }
                prefixes.extend(next.iter().cloned());
                frontier = next;
            }
            let mut attempts = Vec::new();
            for before in prefixes {
                for abort in [false, true] {
                    let root = replay(&before, Some((before.len(), abort))).await;
                    assert!(root.accepted);
                    attempts.push(FaultAttempt {
                        before: before.clone(),
                        after: Vec::new(),
                        abort,
                        result: root,
                    });
                    let mut frontier = vec![Vec::<u8>::new()];
                    for _ in before.len()..8 {
                        let mut next = Vec::new();
                        for after in frontier {
                            for command in 0..8 {
                                let mut candidate = after.clone();
                                candidate.push(command);
                                let mut trace = before.clone();
                                trace.extend_from_slice(&candidate);
                                let result = replay(&trace, Some((before.len(), abort))).await;
                                if result.accepted {
                                    next.push(candidate.clone());
                                }
                                attempts.push(FaultAttempt {
                                    before: before.clone(),
                                    after: candidate,
                                    abort,
                                    result,
                                });
                            }
                        }
                        frontier = next;
                    }
                }
            }
            attempts
        });
    let mut output = String::from("[");
    for (index, attempt) in attempts.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        writeln!(output,
            "{{\"before\":{:?},\"after\":{:?},\"abort\":{},\"accepted\":{},\"actors\":{:?},\"events\":[",
            attempt.before, attempt.after, attempt.abort, attempt.result.accepted, attempt.result.actors).unwrap();
        for (index, event) in attempt.result.events.iter().enumerate() {
            if index != 0 {
                output.push(',');
            }
            writeln!(output, "{{\"kind\":{},\"operation\":{},\"visit\":{},\"actor\":{},\"wire\":{:?},\"witness\":{}}}",
                event.kind, event.operation, event.visit, event.actor, event.wire, event.witness).unwrap();
        }
        output.push_str("]}");
    }
    output.push(']');
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("EndpointFaultPhaseCases.json"), output).unwrap();
    std::println!(
        "phased Endpoint fault schedules: attempts={}",
        attempts.len()
    );
}

#[test]
fn orderly_close_keeps_the_next_loan_queued_during_old_return_publication() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let attempt = replay(&[0, 4, 2, 3, 1, 6, 5, 7], Some((6, false))).await;
            assert!(attempt.accepted && attempt.finished);
            assert_eq!(attempt.actors, [1, 1, 1]);
            assert!(
                !attempt
                    .events
                    .iter()
                    .any(|event| event.kind == 6 || event.kind == 7)
            );
        });
}

#[test]
fn abort_isolates_the_next_loan_without_restoring_the_owner() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let attempt = replay(&[0, 4, 2, 3, 1, 6, 5, 7], Some((6, true))).await;
            assert!(attempt.accepted && !attempt.finished);
            assert_eq!(attempt.actors, [0, 2, 0]);
            assert!(
                attempt
                    .events
                    .iter()
                    .any(|event| event.kind == 1 && event.operation == 0 && event.visit == 1)
            );
            assert!(
                !attempt
                    .events
                    .iter()
                    .any(|event| event.kind == 2 && event.operation == 1 && event.visit == 1)
            );
            assert!(
                attempt
                    .events
                    .iter()
                    .any(|event| event.kind == 6 && event.operation == 1 && event.visit == 1)
            );
            assert!(
                !attempt
                    .events
                    .iter()
                    .any(|event| event.kind == 3 && event.operation == 5 && event.visit == 1)
            );
        });
}
