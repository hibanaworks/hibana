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

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_completed_right_arm_reopens_the_public_route_without_receiving()
-> core::result::Result<(), hibana::EndpointError> {
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
    {
        (async {
            async {
                async {
                    let _: () = (async {
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
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
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
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };
            }
            .await
            .expect("all child results are data");
        })
        .await;
    };

    Ok(())
}

mod remote_controller {
    use super::*;
    const OUTER: DecisionArm = DecisionArm::Left;
    include!("conditional_controller/owned.rs");
}

mod co_resident_controller {
    use super::*;
    const OUTER: DecisionArm = DecisionArm::Right;
    include!("conditional_controller/owned.rs");
}

#[cfg(not(miri))]
mod observations;
