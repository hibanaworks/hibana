//! A rolled loan keeps its enclosing choice while issuing a fresh visit.
use super::*;
use crate::{
    endpoint::proof_snapshot::actor_snapshot,
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::time::Duration;

pub(super) type AlternateLoan = g::Msg<23, Endpoint<'static, 10>>;
pub(super) type AlternateRead = g::Msg<24, u32>;
type AlternateBody = g::Seq<
    g::Send<0, 1, AlternateLoan>,
    g::Seq<
        g::Send<10, 3, AlternateRead>,
        g::Seq<g::Send<3, 10, Value>, g::Send<1, 0, AlternateLoan>>,
    >,
>;
pub(super) type Steps = g::Route<g::Roll<DbLoan>, g::Roll<AlternateBody>>;

#[test]
fn an_enclosing_choice_survives_two_delegated_roll_visits() {
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let mut observations = std::vec::Vec::new();
            for choice in [1, 23] {
                let network = TestTransport::new();
                let mut owner_slab = std::vec![0; 64 * 1024];
                let mut worker_slab = std::vec![0; 64 * 1024];
                let mut service_slab = std::vec![0; 64 * 1024];
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
                    .rendezvous(&mut service_slab, network)
                    .unwrap();
                let sid = SessionId::new(866);
                let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
                let db = owner_rv.enter(sid, &db_program).unwrap();
                let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
                let mut service = service_rv.enter(sid, &service_program).unwrap();
                let exchange = async {
                    let ((db, owner_commits), worker_commit, service_commit) = tokio::join!(
                        async {
                            let mut db = db;
                            let mut commits = std::vec::Vec::new();
                            for _ in 0..2 {
                                // SAFETY: owner_rv attaches a TestTransport actor.
                                let before = unsafe { actor_snapshot(&owner) };
                                match choice {
                                    1 => owner.send::<Loan>(db).await.unwrap(),
                                    23 => owner.send::<AlternateLoan>(db).await.unwrap(),
                                    _ => unreachable!(),
                                }
                                // SAFETY: sending preserves the actor's transport.
                                let after = unsafe { actor_snapshot(&owner) };
                                commits.push((choice, before, after));
                                db = match choice {
                                    1 => owner.recv::<Loan>().await.unwrap(),
                                    23 => owner.recv::<AlternateLoan>().await.unwrap(),
                                    _ => unreachable!(),
                                };
                            }
                            (db, commits)
                        },
                        async {
                            let branch = worker.offer().await.unwrap();
                            assert_eq!(branch.label(), choice);
                            match branch.label() {
                                1 => {
                                    let mut db = branch.recv::<Loan>().await.unwrap();
                                    db.send::<Read>(&41).await.unwrap();
                                    assert_eq!(db.recv::<Value>().await.unwrap(), 42);
                                    worker.send::<Loan>(db).await.unwrap();

                                    // SAFETY: worker_rv attaches a TestTransport actor.
                                    let before = unsafe { actor_snapshot(&worker) };
                                    let mut db = worker.recv::<Loan>().await.unwrap();
                                    // SAFETY: the same actor survives receipt.
                                    let after = unsafe { actor_snapshot(&worker) };
                                    db.send::<Read>(&42).await.unwrap();
                                    assert_eq!(db.recv::<Value>().await.unwrap(), 43);
                                    worker.send::<Loan>(db).await.unwrap();
                                    (choice, before, after)
                                }
                                23 => {
                                    let mut db = branch.recv::<AlternateLoan>().await.unwrap();
                                    db.send::<AlternateRead>(&41).await.unwrap();
                                    assert_eq!(db.recv::<Value>().await.unwrap(), 42);
                                    worker.send::<AlternateLoan>(db).await.unwrap();

                                    // SAFETY: worker_rv attaches a TestTransport actor.
                                    let before = unsafe { actor_snapshot(&worker) };
                                    let mut db = worker.recv::<AlternateLoan>().await.unwrap();
                                    // SAFETY: the same actor survives receipt.
                                    let after = unsafe { actor_snapshot(&worker) };
                                    db.send::<AlternateRead>(&42).await.unwrap();
                                    assert_eq!(db.recv::<Value>().await.unwrap(), 43);
                                    worker.send::<AlternateLoan>(db).await.unwrap();
                                    (choice, before, after)
                                }
                                label => panic!("unexpected loan label {label}"),
                            }
                        },
                        async {
                            let branch = service.offer().await.unwrap();
                            match branch.label() {
                                3 => {
                                    let received = branch.recv::<Read>().await.unwrap();
                                    assert_eq!(received, 41);
                                    service.send::<Value>(&(received + 1)).await.unwrap();
                                    // SAFETY: service_rv attaches a TestTransport actor.
                                    let before = unsafe { actor_snapshot(&service) };
                                    let received = service.recv::<Read>().await.unwrap();
                                    // SAFETY: receiving preserves the transport.
                                    let after = unsafe { actor_snapshot(&service) };
                                    assert_eq!(received, 42);
                                    service.send::<Value>(&(received + 1)).await.unwrap();
                                    (choice, before, after)
                                }
                                24 => {
                                    let received = branch.recv::<AlternateRead>().await.unwrap();
                                    assert_eq!(received, 41);
                                    service.send::<Value>(&(received + 1)).await.unwrap();
                                    // SAFETY: service_rv attaches a TestTransport actor.
                                    let before = unsafe { actor_snapshot(&service) };
                                    let received = service.recv::<AlternateRead>().await.unwrap();
                                    // SAFETY: receiving preserves the transport.
                                    let after = unsafe { actor_snapshot(&service) };
                                    assert_eq!(received, 42);
                                    service.send::<Value>(&(received + 1)).await.unwrap();
                                    (choice, before, after)
                                }
                                label => panic!("unexpected request label {label}"),
                            }
                        },
                    );
                    observations.extend(owner_commits);
                    observations.push(worker_commit);
                    observations.push(service_commit);
                    // A body restart never selects the other enclosing arm.
                    let rejected = match choice {
                        1 => owner.send::<AlternateLoan>(db).await,
                        23 => owner.send::<Loan>(db).await,
                        _ => unreachable!(),
                    };
                    assert!(rejected.is_err());
                };
                tokio::select! {
                    biased;
                    () = tokio::time::sleep(Duration::from_secs(30)) => panic!("enclosed loan stopped progressing"),
                    () = exchange => {},
                }
            }
            let reentries = observations.iter().filter(|(_, before, _)| {
                before.steps.iter().any(|step| step.reentry_scope.is_some())
            });
            assert_eq!(reentries.clone().count(), 6);
            for (_, before, after) in reentries {
                assert_eq!(before.choices, after.choices);
                assert!(before.choices.iter().any(|(_, arm)| arm.is_some()));
            }
            #[cfg(not(miri))]
            super::reentry_commit::write_observations(&observations);
        });
}
