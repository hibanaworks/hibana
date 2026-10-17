//! The same inner route can be controlled remotely or through a co-resident view.
use super::*;
use crate::runtime::{
    SessionKitStorage,
    ids::SessionId,
    resolver::{DecisionArm, ResolverRef},
};
use crate::test_support::transport::TestTransport;
use core::cell::Cell;

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
        .build()
        .unwrap();
    for outer in [DecisionArm::Left, DecisionArm::Right] {
        runtime.block_on(async {
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
                            for number in [41, 42] {
                                a_number.set(number);
                                let branch = db.offer().await.unwrap();
                                match branch.label() {
                                    3 => branch.send::<Read>(&number).await.unwrap(),
                                    6 => branch.send::<OtherRead>(&number).await.unwrap(),
                                    label => panic!("unexpected DB branch {label}"),
                                }
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
                                    for expected in [41, 42] {
                                        let branch = service.offer().await.unwrap();
                                        let number = match branch.label() {
                                            3 => branch.recv::<Read>().await.unwrap(),
                                            6 => branch.recv::<OtherRead>().await.unwrap(),
                                            label => panic!("unexpected service branch {label}"),
                                        };
                                        assert_eq!(number, expected);
                                        service.send::<Value>(&(number + 1)).await.unwrap();
                                    }
                                    assert_eq!(service.recv::<AfterRead>().await.unwrap(), 47);
                                    service.send::<AfterValue>(&48).await.unwrap();
                                },
                                async {
                                    for number in [41, 42] {
                                        service_number.set(number);
                                        let branch = db.offer().await.unwrap();
                                        match branch.label() {
                                            3 => branch.send::<Read>(&number).await.unwrap(),
                                            6 => branch.send::<OtherRead>(&number).await.unwrap(),
                                            label => panic!("unexpected local DB branch {label}"),
                                        }
                                        assert_eq!(db.recv::<Value>().await.unwrap(), number + 1);
                                    }
                                    auth.send::<Loan>(db).await.unwrap();
                                }
                            );
                        }
                        8 => {
                            branch.recv::<Notice>().await.unwrap();
                            service.send::<ServiceReady>(&()).await.unwrap();
                            for expected in [41, 42] {
                                let branch = service.offer().await.unwrap();
                                let number = match branch.label() {
                                    3 => branch.recv::<Read>().await.unwrap(),
                                    6 => branch.recv::<OtherRead>().await.unwrap(),
                                    label => panic!("unexpected remote DB branch {label}"),
                                };
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
        });
    }
}
