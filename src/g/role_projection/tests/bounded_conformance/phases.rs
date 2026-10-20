//! Split acceptance/completion of the loan, independent par send, return and
//! next loan without changing localside. Other traffic runs normally.
use super::*;
use core::cell::RefCell;
use std::rc::Rc;

mod carrier;
mod faults;
use carrier::Network;

#[derive(Clone, Debug)]
struct Boundary {
    // 0=public call start, 1=physical acceptance, 2=physical delivery,
    // 3=successful public completion, 4=close, 5=abort, 6=transport error,
    // 7=public error, 8=test I/O release. Observations never grant progress.
    kind: u8,
    operation: usize,
    visit: usize,
    actor: u8,
    wire: Vec<u8>,
    witness: u64,
}

struct Attempt {
    commands: Vec<u8>,
    accepted: bool,
    finished: bool,
    events: Vec<Boundary>,
    // 0=parked, 1=Ok, 2=Endpoint error. Test teardown is not an observed fault.
    actors: [u8; 3],
}

const SID: SessionId = SessionId::new(9076);

async fn replay(commands: &[u8], injection: Option<(usize, bool)>) -> Attempt {
    let network = Network::new();
    let owner_io = network.for_actor(0);
    let worker_io = network.for_actor(1);
    let service_io = network.for_actor(2);
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let owner = tokio::task::spawn_local(async move {
                let program =
                    crate::runtime::program::project::<0>(&g::Program::<OwnedParallel>::new());
                let db_program =
                    crate::runtime::program::project::<10>(&g::Program::<OwnedParallel>::new());
                let mut slab = [0; 32 * 1024];
                let mut storage = SessionKitStorage::uninit();
                let rv = storage
                    .init()
                    .rendezvous(&mut slab, owner_io.clone())
                    .unwrap();
                let mut endpoint = rv.enter(SID, &program).unwrap();
                let db = rv.enter(SID, &db_program).unwrap();
                let [mut loans, mut ticker] = endpoint.par().unwrap();
                let (loans_result, tick_result) = tokio::join!(
                    async {
                        let mut db = db;
                        for visit in 0..2 {
                            owner_io.call(0, 0, visit);
                            let result = loans.send::<Loan>(db).await;
                            owner_io.call(if result.is_ok() { 3 } else { 7 }, 0, visit);
                            result?;
                            owner_io.call(0, 5, visit);
                            let result = loans.recv::<Return>().await;
                            owner_io.call(if result.is_ok() { 3 } else { 7 }, 5, visit);
                            db = result?;
                        }
                        Ok::<(), crate::endpoint::EndpointError>(())
                    },
                    async {
                        owner_io.call(0, 6, 0);
                        let result = ticker.send::<Tick>(&7).await;
                        owner_io.call(if result.is_ok() { 3 } else { 7 }, 6, 0);
                        result
                    },
                );
                loans_result?;
                tick_result?;
                Ok::<(), crate::endpoint::EndpointError>(())
            });
            let worker = tokio::task::spawn_local(async move {
                let program =
                    crate::runtime::program::project::<1>(&g::Program::<OwnedParallel>::new());
                let mut slab = [0; 32 * 1024];
                let mut storage = SessionKitStorage::uninit();
                let rv = storage
                    .init()
                    .rendezvous(&mut slab, worker_io.clone())
                    .unwrap();
                let mut endpoint = rv.enter(SID, &program).unwrap();
                for visit in 0..2 {
                    worker_io.call(0, 1, visit);
                    let result = endpoint.recv::<Loan>().await;
                    worker_io.call(if result.is_ok() { 3 } else { 7 }, 1, visit);
                    let mut db = result?;
                    worker_io.call(0, 2, visit);
                    let result = db.send::<Read>(&7).await;
                    worker_io.call(if result.is_ok() { 3 } else { 7 }, 2, visit);
                    result?;
                    worker_io.call(0, 4, visit);
                    let result = endpoint.send::<Return>(db).await;
                    worker_io.call(if result.is_ok() { 3 } else { 7 }, 4, visit);
                    result?;
                }
                Ok::<(), crate::endpoint::EndpointError>(())
            });
            let service = tokio::task::spawn_local(async move {
                let program =
                    crate::runtime::program::project::<2>(&g::Program::<OwnedParallel>::new());
                let mut slab = [0; 32 * 1024];
                let mut storage = SessionKitStorage::uninit();
                let rv = storage
                    .init()
                    .rendezvous(&mut slab, service_io.clone())
                    .unwrap();
                let mut endpoint = rv.enter(SID, &program).unwrap();
                let [mut reader, mut ticks] = endpoint.par().unwrap();
                let (reads_result, tick_result) = tokio::join!(
                    async {
                        for visit in 0..2 {
                            service_io.call(0, 3, visit);
                            let result = reader.recv::<Read>().await;
                            service_io.call(if result.is_ok() { 3 } else { 7 }, 3, visit);
                            assert_eq!(result?, 7);
                        }
                        Ok::<(), crate::endpoint::EndpointError>(())
                    },
                    async {
                        service_io.call(0, 7, 0);
                        let result = ticks.recv::<Tick>().await;
                        service_io.call(if result.is_ok() { 3 } else { 7 }, 7, 0);
                        assert_eq!(result?, 7);
                        Ok::<(), crate::endpoint::EndpointError>(())
                    },
                );
                reads_result?;
                tick_result?;
                Ok::<(), crate::endpoint::EndpointError>(())
            });
            // With paused Tokio time and no external I/O, this timer advances only
            // after runnable local work has parked. It never grants an Endpoint op.
            tokio::time::sleep(core::time::Duration::from_millis(1)).await;
            let mut accepted = true;
            for (index, &command) in commands.iter().enumerate() {
                if let Some((at, abort)) = injection
                    && at == index
                {
                    network.close_loan(abort);
                    tokio::time::sleep(core::time::Duration::from_millis(1)).await;
                }
                if !network.release(command) {
                    assert_eq!(
                        index + 1,
                        commands.len(),
                        "prefix stopped being reproducible"
                    );
                    accepted = false;
                    break;
                }
                if injection.is_some() {
                    network.call(8, usize::from(command), 0);
                }
                tokio::time::sleep(core::time::Duration::from_millis(1)).await;
            }
            if let Some((at, abort)) = injection
                && at == commands.len()
            {
                network.close_loan(abort);
                tokio::time::sleep(core::time::Duration::from_millis(1)).await;
            }
            let events = network.events.borrow().clone();
            let mut actors = [0; 3];
            for (index, task) in [owner, worker, service].into_iter().enumerate() {
                if task.is_finished() {
                    actors[index] = if task.await.unwrap().is_ok() { 1 } else { 2 };
                } else {
                    task.abort();
                    if let Err(error) = task.await {
                        if error.is_panic() {
                            std::panic::resume_unwind(error.into_panic());
                        }
                        assert!(error.is_cancelled(), "unexpected local task failure");
                    }
                }
            }
            let finished = actors == [1; 3];
            if finished {
                assert!(network.inner_queue_is_empty());
            }
            Attempt {
                commands: commands.to_vec(),
                accepted,
                finished,
                events,
                actors,
            }
        })
        .await
}

#[test]
#[cfg_attr(
    miri,
    ignore = "native exhaustive I/O schedules; the reverse-completion replay has a separate Miri test"
)]
fn bounded_acceptance_and_completion_schedules_emit_lean_cases() {
    use core::fmt::Write;
    let attempts = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let mut frontier = vec![Vec::<u8>::new()];
            let mut attempts = Vec::new();
            for _ in 0..8 {
                let mut next = Vec::new();
                for prefix in frontier {
                    for command in 0..8 {
                        let mut trace = prefix.clone();
                        trace.push(command);
                        let attempt = replay(&trace, None).await;
                        if attempt.accepted {
                            next.push(trace);
                        }
                        attempts.push(attempt);
                    }
                }
                frontier = next;
            }
            attempts
        });
    let complete = attempts.iter().filter(|attempt| attempt.finished).count();
    let mut output = String::from("[");
    for (index, attempt) in attempts.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        writeln!(
            output,
            "{{\"commands\":{:?},\"accepted\":{},\"finished\":{},\"events\":[",
            attempt.commands, attempt.accepted, attempt.finished
        )
        .unwrap();
        for (index, event) in attempt.events.iter().enumerate() {
            if index != 0 {
                output.push(',');
            }
            writeln!(output, "{{\"kind\":{},\"operation\":{},\"visit\":{},\"actor\":{},\"wire\":{:?},\"witness\":{}}}",
                event.kind, event.operation, event.visit, event.actor, event.wire, event.witness).unwrap();
        }
        writeln!(output, "]}}").unwrap();
    }
    output.push(']');
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("EndpointPhaseCases.json"), output).unwrap();
    std::println!(
        "phased Endpoint schedules: attempts={} complete={complete}",
        attempts.len()
    );
}

#[test]
fn one_actors_reverse_completions_preserve_the_real_returned_endpoint() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let attempt = replay(&[0, 4, 2, 3, 1, 6, 5, 7], None).await;
            assert!(attempt.accepted && attempt.finished);
            let position = |kind, operation, visit| {
                attempt
                    .events
                    .iter()
                    .position(|event| {
                        event.kind == kind && event.operation == operation && event.visit == visit
                    })
                    .unwrap()
            };
            let accepted = position(1, 0, 0);
            let published = position(3, 0, 0);
            assert!(
                accepted < position(3, 1, 0),
                "worker receives before old sender publishes"
            );
            assert!(
                position(1, 4, 0) < published,
                "return is accepted before old loan sender publishes"
            );
            assert!(
                position(3, 6, 0) < published,
                "the same actor's independent send completes first"
            );
            assert!(
                position(1, 0, 1) < position(3, 4, 0),
                "the next loan is accepted before the old return sender publishes"
            );
        });
}
