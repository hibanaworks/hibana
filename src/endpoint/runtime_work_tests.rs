//! Native test adapter for the no_std cursor counters. Coordination is executed
//! by actual Endpoints; formatting happens after the measured operations.
#[path = "../../examples/support/in_memory.rs"]
mod in_memory;
#[path = "../../benches/support/protocol_repetition.rs"]
mod protocol_repetition;
use in_memory::InMemoryTransport;

use super::proof_snapshot::{view_tag, work_counts};
use crate::{
    Endpoint,
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
        transport::EndpointBinding,
    },
};
use protocol_repetition::repeated;

type Request = Msg<1, u32>;
type Reply = Msg<2, u32>;
type Tail = Msg<3, ()>;
type Alternative = Msg<4, u32>;
type AlternativeReply = Msg<5, u32>;
type Loan<'r> = Msg<6, Endpoint<'r, 2>>;
type Tick = Msg<7, u32>;

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

fn report<const ACTORS: usize>(
    case: &str,
    parameter: u32,
    operations: u32,
    run: u32,
    before: [&[(&str, u64)]; ACTORS],
    after: [&[(&str, u64)]; ACTORS],
) {
    assert_eq!(before[0].len(), before[1].len());
    for (index, (metric, _)) in before[0].iter().enumerate() {
        let mut total = 0u64;
        for actor in 0..ACTORS {
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
            measure_routes().await;
            measure_delegation().await;
            measure_nested_roll().await;
        });
}

async fn measure_routes() {
    let global = g::route(
        g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()),
        g::seq(
            g::send::<0, 1, Alternative>(),
            g::send::<1, 0, AlternativeReply>(),
        ),
    )
    .resolve::<901>()
    .roll();
    let programs: (RoleProgram<0>, RoleProgram<1>) = (project(&global), project(&global));
    // Both resolver outcomes are measured, each in two fresh sessions.
    for right in [false, true] {
        for run in 0..2 {
            let network = InMemoryTransport::new([]);
            let mut slab = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, network.clone())
                .unwrap();
            rv.set_resolver(
                &programs.0,
                ResolverRef::<901>::decision_state(
                    &right,
                    |right| -> Result<DecisionArm, ResolverError> {
                        Ok(if *right {
                            DecisionArm::Right
                        } else {
                            DecisionArm::Left
                        })
                    },
                ),
            )
            .unwrap();
            let sid = SessionId::new(1720 + run);
            let mut a = rv.enter(sid, &programs.0).unwrap();
            let mut b = rv.enter(sid, &programs.1).unwrap();
            let tags = [view_tag(&a), view_tag(&b)];
            // SAFETY: both actors remain alive; no future is polled during the snapshot.
            let before = tags.map(|tag| unsafe { work_counts::<InMemoryTransport>(tag) });
            tokio::join!(
                async {
                    for value in 0..32 {
                        let branch = a.offer().await.unwrap();
                        match branch.label() {
                            1 => {
                                branch.send::<Request>(&value).await.unwrap();
                                assert_eq!(a.recv::<Reply>().await.unwrap(), value + 1);
                            }
                            4 => {
                                branch.send::<Alternative>(&value).await.unwrap();
                                assert_eq!(a.recv::<AlternativeReply>().await.unwrap(), value + 2);
                            }
                            label => panic!("unexpected selected route {label}"),
                        }
                    }
                },
                async {
                    for _ in 0..32 {
                        let branch = b.offer().await.unwrap();
                        match branch.label() {
                            1 => {
                                let value = branch.recv::<Request>().await.unwrap();
                                b.send::<Reply>(&(value + 1)).await.unwrap();
                            }
                            4 => {
                                let value = branch.recv::<Alternative>().await.unwrap();
                                b.send::<AlternativeReply>(&(value + 2)).await.unwrap();
                            }
                            label => panic!("unexpected received route {label}"),
                        }
                    }
                },
            );
            // SAFETY: both original actors are still retained by a and b.
            let after = tags.map(|tag| unsafe { work_counts::<InMemoryTransport>(tag) });
            report(
                "route",
                u32::from(right),
                128,
                run,
                [before[0].as_ref(), before[1].as_ref()],
                [after[0].as_ref(), after[1].as_ref()],
            );
            assert!(network.queue_is_empty());
        }
    }
}

async fn measure_delegation() {
    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::send::<1, 3, Tick>(),
            g::seq(
                g::par(
                    g::seq(g::send::<2, 3, Request>(), g::send::<3, 2, Reply>()),
                    g::send::<1, 3, Tick>(),
                ),
                g::send::<1, 0, Loan<'_>>(),
            ),
        ),
    )
    .roll();
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let resource_program = project::<2>(&global);
    let service_program = project::<3>(&global);
    for run in 0..2 {
        let sid = SessionId::new(1730 + run);
        // SAFETY: this fresh test resource has one issuer, shared by all three runtimes.
        let binding = unsafe { EndpointBinding::new() };
        let network = InMemoryTransport::new([((sid, 2), binding)]);
        let mut owner_slab = [0; 32 * 1024];
        let mut worker_slab = [0; 32 * 1024];
        let mut service_slab = [0; 32 * 1024];
        let mut owner_storage = SessionKitStorage::uninit();
        let mut worker_storage = SessionKitStorage::uninit();
        let mut service_storage = SessionKitStorage::uninit();
        let owner_rv = owner_storage
            .init()
            .rendezvous(&mut owner_slab, network.clone())
            .unwrap();
        let worker_rv = worker_storage
            .init()
            .rendezvous(&mut worker_slab, network.clone())
            .unwrap();
        let service_rv = service_storage
            .init()
            .rendezvous(&mut service_slab, network.clone())
            .unwrap();
        let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
        let mut resource = owner_rv.enter(sid, &resource_program).unwrap();
        let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
        let mut service = service_rv.enter(sid, &service_program).unwrap();
        let tags = [view_tag(&owner), view_tag(&worker), view_tag(&service)];
        // SAFETY: all three actors remain alive, including during resource transfers.
        let before = tags.map(|tag| unsafe { work_counts::<InMemoryTransport>(tag) });
        let (_resource, (), ()) = tokio::join!(
            async {
                for _ in 0..32 {
                    owner.send::<Loan<'_>>(resource).await.unwrap();
                    resource = owner.recv::<Loan<'_>>().await.unwrap();
                }
                resource
            },
            async {
                for value in 0..32 {
                    let mut resource = worker.recv::<Loan<'_>>().await.unwrap();
                    tokio::join!(
                        biased;
                        async {
                            resource.send::<Request>(&value).await.unwrap();
                            assert_eq!(resource.recv::<Reply>().await.unwrap(), value + 1);
                        },
                        async {
                            worker.send::<Tick>(&value).await.unwrap();
                            worker.send::<Tick>(&value).await.unwrap();
                        },
                    );
                    worker.send::<Loan<'_>>(resource).await.unwrap();
                }
            },
            async {
                for value in 0..32 {
                    assert_eq!(service.recv::<Tick>().await.unwrap(), value);
                    let [mut query, mut tick] = service.par().unwrap();
                    tokio::join!(
                        async {
                            assert_eq!(query.recv::<Request>().await.unwrap(), value);
                            query.send::<Reply>(&(value + 1)).await.unwrap();
                        },
                        async {
                            assert_eq!(tick.recv::<Tick>().await.unwrap(), value);
                        },
                    );
                }
            },
        );
        // SAFETY: owner, worker and service still retain their original actors.
        let after = tags.map(|tag| unsafe { work_counts::<InMemoryTransport>(tag) });
        report(
            "delegation",
            1,
            384,
            run,
            [before[0].as_ref(), before[1].as_ref(), before[2].as_ref()],
            [after[0].as_ref(), after[1].as_ref(), after[2].as_ref()],
        );
        assert!(network.queue_is_empty());
    }
}

async fn measure_nested_roll() {
    let global = g::seq(
        g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()).roll(),
        g::seq(
            g::send::<0, 1, Alternative>(),
            g::send::<1, 0, AlternativeReply>(),
        ),
    )
    .roll();
    let programs: (RoleProgram<0>, RoleProgram<1>) = (project(&global), project(&global));
    for run in 0..2 {
        let network = InMemoryTransport::new([]);
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, network.clone())
            .unwrap();
        let sid = SessionId::new(1740 + run);
        let mut a = rv.enter(sid, &programs.0).unwrap();
        let mut b = rv.enter(sid, &programs.1).unwrap();
        let tags = [view_tag(&a), view_tag(&b)];
        // SAFETY: both actors remain pinned by their live endpoints.
        let before = tags.map(|tag| unsafe { work_counts::<InMemoryTransport>(tag) });
        tokio::join!(
            async {
                for value in 0..32 {
                    for _ in 0..2 {
                        a.send::<Request>(&value).await.unwrap();
                        assert_eq!(a.recv::<Reply>().await.unwrap(), value + 1);
                    }
                    a.send::<Alternative>(&value).await.unwrap();
                    assert_eq!(a.recv::<AlternativeReply>().await.unwrap(), value + 2);
                }
            },
            async {
                for value in 0..32 {
                    for _ in 0..2 {
                        assert_eq!(b.recv::<Request>().await.unwrap(), value);
                        b.send::<Reply>(&(value + 1)).await.unwrap();
                    }
                    assert_eq!(b.recv::<Alternative>().await.unwrap(), value);
                    b.send::<AlternativeReply>(&(value + 2)).await.unwrap();
                }
            },
        );
        // SAFETY: the same a and b are still live after all measured operations.
        let after = tags.map(|tag| unsafe { work_counts::<InMemoryTransport>(tag) });
        report(
            "nested-roll",
            2,
            384,
            run,
            [before[0].as_ref(), before[1].as_ref()],
            [after[0].as_ref(), after[1].as_ref()],
        );
        assert!(network.queue_is_empty());
    }
}
