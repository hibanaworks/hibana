//! The same inner route can be controlled remotely or through a co-resident view.
use super::*;
use crate::endpoint::proof_snapshot::{actor_snapshot, branch_snapshot};
use crate::runtime::{
    SessionKitStorage,
    ids::SessionId,
    resolver::{DecisionArm, ResolverRef},
};
use crate::test_support::transport::TestTransport;
use core::cell::Cell;
use std::vec::Vec;

type Notice = g::Msg<8, ()>;
type ServiceReady = g::Msg<12, ()>;
type OtherRead = g::Msg<6, u32>;
type AfterRead = g::Msg<9, u32>;
type AfterValue = g::Msg<7, u32>;
type Choice = g::Resolve<
    g::Route<
        g::Seq<
            g::Send<0, 3, Notice>,
            g::Seq<
                g::Send<3, 0, ServiceReady>,
                g::Seq<g::Send<0, 1, Loan>, g::Send<0, 1, OtherLoan>>,
            >,
        >,
        g::Seq<g::Send<0, 3, Loan>, g::Seq<g::Send<0, 3, OtherLoan>, g::Send<0, 1, Notice>>>,
    >,
    701,
>;
type Work = g::Roll<
    g::Resolve<
        g::Route<
            g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>,
            g::Seq<g::Send<10, 3, OtherRead>, g::Send<3, 10, Value>>,
        >,
        702,
    >,
>;
pub(super) type Steps = g::Seq<
    Choice,
    g::Seq<
        Work,
        g::Seq<g::Send<11, 0, Loan>, g::Seq<g::Send<10, 3, AfterRead>, g::Send<3, 10, AfterValue>>>,
    >,
>;

pub(super) type LocalSteps = g::Seq<
    g::Send<0, 1, Loan>,
    g::Resolve<g::Route<g::Send<10, 10, g::Msg<20, ()>>, g::Send<10, 10, g::Msg<21, ()>>>, 710>,
>;

#[test]
fn a_completed_right_arm_reopens_the_public_route_without_receiving() {
    use crate::runtime::program::project;

    let global = g::seq(
        g::route(
            g::seq(g::send::<10, 3, Read>(), g::send::<3, 10, Value>()),
            g::seq(g::send::<10, 3, OtherRead>(), g::send::<3, 10, Value>()),
        )
        .resolve::<702>()
        .roll(),
        g::seq(
            g::send::<10, 3, AfterRead>(),
            g::send::<3, 10, AfterValue>(),
        ),
    );
    let db_program = project::<10>(&global);
    let service_program = project::<3>(&global);
    let network = TestTransport::new();
    let mut db_slab = std::vec![0; 16 * 1024];
    let mut service_slab = std::vec![0; 16 * 1024];
    let number = Cell::new(41u32);
    let mut db_storage = SessionKitStorage::uninit();
    let mut service_storage = SessionKitStorage::uninit();
    let db_rv = db_storage
        .init()
        .rendezvous(&mut db_slab, network.clone())
        .unwrap();
    let service_rv = service_storage
        .init()
        .rendezvous(&mut service_slab, network)
        .unwrap();
    db_rv
        .set_resolver(
            &db_program,
            ResolverRef::<702>::decision_state(&number, |number| {
                Ok(if number.get() % 2 == 1 {
                    DecisionArm::Left
                } else {
                    DecisionArm::Right
                })
            }),
        )
        .unwrap();
    let sid = SessionId::new(849);
    let mut db = db_rv.enter(sid, &db_program).unwrap();
    let mut service = service_rv.enter(sid, &service_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(240), async {
                tokio::join!(
                    async {
                        for value in [41, 43, 42, 44, 45] {
                            number.set(value);
                            let branch = db.offer().await.unwrap();
                            match branch.label() {
                                3 => branch.send::<Read>(&value).await.unwrap(),
                                6 => branch.send::<OtherRead>(&value).await.unwrap(),
                                label => panic!("unexpected controller label {label}"),
                            }
                            assert_eq!(db.recv::<Value>().await.unwrap(), value + 1);
                        }
                        db.send::<AfterRead>(&47).await.unwrap();
                        assert_eq!(db.recv::<AfterValue>().await.unwrap(), 48);
                    },
                    async {
                        for expected in [41, 43, 42, 44, 45] {
                            let branch = service.offer().await.unwrap();
                            let value = match branch.label() {
                                3 => branch.recv::<Read>().await.unwrap(),
                                6 => branch.recv::<OtherRead>().await.unwrap(),
                                label => panic!("unexpected observer label {label}"),
                            };
                            assert_eq!(value, expected);
                            service.send::<Value>(&(value + 1)).await.unwrap();
                        }
                        assert_eq!(service.recv::<AfterRead>().await.unwrap(), 47);
                        service.send::<AfterValue>(&48).await.unwrap();
                    }
                );
            })
            .await
            .expect("a completed right arm must not wait for an incoming request");
        });
}

#[test]
fn a_conditional_controller_does_not_take_authority_from_its_co_resident_observer() {
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let a_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 32>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    for outer in [DecisionArm::Left, DecisionArm::Right] {
        runtime.block_on(async {
            let mut a_observations = Vec::new();
            let mut controller_observations = Vec::new();
            let mut observer_observations = Vec::new();
            let network = TestTransport::new();
            let mut owner_slab = std::vec![0; 64 * 1024];
            let mut a_slab = std::vec![0; 64 * 1024];
            let mut service_slab = std::vec![0; 64 * 1024];
            let a_number = Cell::new(41u32);
            // The observer must not resolve from stale application input before
            // the distinct DB view sets its next value and chooses its branch.
            let service_number = Cell::new(42u32);
            let mut owner_storage = SessionKitStorage::uninit();
            let mut a_storage = SessionKitStorage::uninit();
            let mut service_storage = SessionKitStorage::uninit();
            let owner_rv = owner_storage
                .init()
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let a_rv = a_storage
                .init()
                .rendezvous(&mut a_slab, network.clone())
                .unwrap();
            let service_rv = service_storage
                .init()
                .rendezvous(&mut service_slab, network)
                .unwrap();
            owner_rv
                .set_resolver(
                    &owner_program,
                    ResolverRef::<701>::decision_state(&outer, |arm| Ok(*arm)),
                )
                .unwrap();
            a_rv.set_resolver(
                &db_program,
                ResolverRef::<702>::decision_state(&a_number, |number| {
                    Ok(if number.get() % 2 == 1 {
                        DecisionArm::Left
                    } else {
                        DecisionArm::Right
                    })
                }),
            )
            .unwrap();
            service_rv
                .set_resolver(
                    &db_program,
                    ResolverRef::<702>::decision_state(&service_number, |number| {
                        Ok(if number.get() % 2 == 1 {
                            DecisionArm::Left
                        } else {
                            DecisionArm::Right
                        })
                    }),
                )
                .unwrap();
            let sid = SessionId::new(848);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let db = owner_rv.enter(sid, &db_program).unwrap();
            let auth = owner_rv.enter(sid, &auth_program).unwrap();
            let mut a = a_rv.enter(sid, &a_program).unwrap();
            let mut service = service_rv.enter(sid, &service_program).unwrap();
            tokio::time::timeout(core::time::Duration::from_secs(240), async {
            tokio::join!(
                async {
                    let branch = owner.offer().await.unwrap();
                    match branch.label() {
                        8 => {
                            branch.send::<Notice>(&()).await.unwrap();
                            owner.recv::<ServiceReady>().await.unwrap();
                            owner.send::<Loan>(db).await.unwrap();
                            owner.send::<OtherLoan>(auth).await.unwrap();
                        }
                        1 => {
                            branch.send::<Loan>(db).await.unwrap();
                            owner.send::<OtherLoan>(auth).await.unwrap();
                            owner.send::<Notice>(&()).await.unwrap();
                        }
                        label => panic!("unexpected owner branch {label}"),
                    }
                    let mut db = owner.recv::<Loan>().await.unwrap();
                    db.send::<AfterRead>(&47).await.unwrap();
                    assert_eq!(db.recv::<AfterValue>().await.unwrap(), 48);
                },
                async {
                    let branch = a.offer().await.unwrap();
                    match branch.label() {
                        1 => {
                            let mut db = branch.recv::<Loan>().await.unwrap();
                            let mut auth = a.recv::<OtherLoan>().await.unwrap();
                            for number in [41, 43, 42, 44, 45] {
                                a_number.set(number);
                                // SAFETY: a_rv was created with TestTransport.
                                let before = unsafe { actor_snapshot(&db) };
                                let branch = db.offer().await.unwrap();
                                // SAFETY: this branch retains that same actor.
                                let offered = unsafe { branch_snapshot(&branch) };
                                match branch.label() {
                                    3 => branch.send::<Read>(&number).await.unwrap(),
                                    6 => branch.send::<OtherRead>(&number).await.unwrap(),
                                    label => panic!("unexpected DB branch {label}"),
                                }
                                // SAFETY: the live DB view still belongs to a_rv.
                                let after = unsafe { actor_snapshot(&db) };
                                a_observations.push((before, offered, after));
                                assert_eq!(db.recv::<Value>().await.unwrap(), number + 1);
                            }
                            auth.send::<Loan>(db).await.unwrap();
                        }
                        8 => branch.recv::<Notice>().await.unwrap(),
                        label => panic!("unexpected holder choice {label}"),
                    }
                },
                async {
                    let branch = service.offer().await.unwrap();
                    match branch.label() {
                        1 => {
                            let mut db = branch.recv::<Loan>().await.unwrap();
                            let mut auth = service.recv::<OtherLoan>().await.unwrap();
                            // Poll the observer first. It must await the same
                            // actor's distinct controller view without deciding.
                            tokio::join!(
                                biased;
                                async {
                                    for expected in [41, 43, 42, 44, 45] {
                                        // SAFETY: service_rv uses TestTransport.
                                        let before = unsafe { actor_snapshot(&service) };
                                        let branch = service.offer().await.unwrap();
                                        // SAFETY: the branch retains service_rv.
                                        let offered = unsafe { branch_snapshot(&branch) };
                                        let number = match branch.label() {
                                            3 => branch.recv::<Read>().await.unwrap(),
                                            6 => branch.recv::<OtherRead>().await.unwrap(),
                                            label => panic!("unexpected service branch {label}"),
                                        };
                                        // SAFETY: service remains attached to service_rv.
                                        let after = unsafe { actor_snapshot(&service) };
                                        observer_observations.push((before, offered, after));
                                        assert_eq!(number, expected);
                                        service.send::<Value>(&(number + 1)).await.unwrap();
                                    }
                                    assert_eq!(service.recv::<AfterRead>().await.unwrap(), 47);
                                    service.send::<AfterValue>(&48).await.unwrap();
                                },
                                async {
                                    for number in [41, 43, 42, 44, 45] {
                                        service_number.set(number);
                                        // SAFETY: the received view is issued by service_rv.
                                        let before = unsafe { actor_snapshot(&db) };
                                        let branch = db.offer().await.unwrap();
                                        // SAFETY: the branch retains that TestTransport actor.
                                        let offered = unsafe { branch_snapshot(&branch) };
                                        match branch.label() {
                                            3 => branch.send::<Read>(&number).await.unwrap(),
                                            6 => branch.send::<OtherRead>(&number).await.unwrap(),
                                            label => panic!("unexpected local DB branch {label}"),
                                        }
                                        // SAFETY: DB remains attached to service_rv.
                                        let after = unsafe { actor_snapshot(&db) };
                                        controller_observations.push((before, offered, after));
                                        assert_eq!(db.recv::<Value>().await.unwrap(), number + 1);
                                    }
                                    auth.send::<Loan>(db).await.unwrap();
                                }
                            );
                        }
                        8 => {
                            branch.recv::<Notice>().await.unwrap();
                            service.send::<ServiceReady>(&()).await.unwrap();
                            for expected in [41, 43, 42, 44, 45] {
                                // SAFETY: service_rv uses TestTransport.
                                let before = unsafe { actor_snapshot(&service) };
                                let branch = service.offer().await.unwrap();
                                // SAFETY: the branch retains service_rv.
                                let offered = unsafe { branch_snapshot(&branch) };
                                let number = match branch.label() {
                                    3 => branch.recv::<Read>().await.unwrap(),
                                    6 => branch.recv::<OtherRead>().await.unwrap(),
                                    label => panic!("unexpected remote DB branch {label}"),
                                };
                                // SAFETY: service remains attached to service_rv.
                                let after = unsafe { actor_snapshot(&service) };
                                observer_observations.push((before, offered, after));
                                assert_eq!(number, expected);
                                service.send::<Value>(&(number + 1)).await.unwrap();
                            }
                            assert_eq!(service.recv::<AfterRead>().await.unwrap(), 47);
                            service.send::<AfterValue>(&48).await.unwrap();
                        }
                        label => panic!("unexpected service holder choice {label}"),
                    }
                }
            );
            }).await.expect("five route visits must complete without an incoming controller frame");
            for (before, offered, _) in a_observations.iter().chain(&controller_observations).chain(
                observer_observations
                    .iter()
                    .filter(|_| matches!(outer, DecisionArm::Left)),
            ) {
                // The preview may hold a RouteBranch operation lease while
                // every pre-existing protocol/issuance observation is unchanged.
                assert_eq!(before.actor, offered.actor);
                assert_eq!(before.run, offered.run);
                assert_eq!(before.counter, offered.counter);
                assert_eq!(before.live_views, offered.live_views);
                assert!(before.view_headers().eq(offered.view_headers()));
                assert_eq!(before.steps, offered.steps);
                assert_eq!(before.choices, offered.choices);
            }
            #[cfg(not(miri))]
            observations::write(
                outer,
                &a_observations,
                &controller_observations,
                &observer_observations,
            );
        });
    }
}

#[cfg(not(miri))]
mod observations;
