//! One actor receives a choice through either of its distinct logical views.
use super::*;
use crate::{
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        resolver::{DecisionArm, ResolverRef},
    },
    test_support::transport::TestTransport,
};
use core::time::Duration;

type WorkerInput = g::Msg<14, u32>;
type ResourceInput = g::Msg<15, u32>;
type Answer = g::Msg<16, u32>;
pub(super) type Steps = g::Seq<
    g::Send<0, 1, Loan>,
    g::Seq<
        g::Resolve<g::Route<g::Send<0, 1, WorkerInput>, g::Send<0, 10, ResourceInput>>, 711>,
        g::Seq<g::Send<1, 0, Answer>, g::Send<1, 0, Loan>>,
    >,
>;

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_branch_frame_only_selects_the_addressed_logical_view()
-> core::result::Result<(), hibana::EndpointError> {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let resource_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    {
        for selected in [DecisionArm::Left, DecisionArm::Right] {
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, TestTransport::new())
                .unwrap();
            rv.set_resolver(
                &owner_program,
                ResolverRef::<711>::decision_state(&selected, |arm| Ok(*arm)),
            )
            .unwrap();
            let sid = SessionId::new(864);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let db = rv.enter(sid, &resource_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let exchange = async {
                async {
                    async {
                        let _: () = (async {
                            owner.send::<Loan>(db).await.unwrap();
                            let branch = owner.offer().await.unwrap();
                            match branch.label() {
                                14 => branch.send::<WorkerInput>(&41).await.unwrap(),
                                15 => branch.send::<ResourceInput>(&41).await.unwrap(),
                                label => panic!("unexpected ingress choice {label}"),
                            }
                            assert_eq!(owner.recv::<Answer>().await.unwrap(), 42);
                            drop(owner.recv::<Loan>().await.unwrap());
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        let _: () = (async {
                            let mut db = worker.recv::<Loan>().await.unwrap();
                            let number = match hibana_runtime::race(
                                async {
                                    core::ops::ControlFlow::Break(
                                        (async {
                                            let branch = worker.offer().await.unwrap();
                                            assert_eq!(branch.label(), 14);
                                            branch.recv::<WorkerInput>().await.unwrap()
                                        })
                                        .await,
                                    )
                                },
                                async {
                                    core::ops::ControlFlow::Continue(
                                        (async {
                                            let branch = db.offer().await.unwrap();
                                            assert_eq!(branch.label(), 15);
                                            branch.recv::<ResourceInput>().await.unwrap()
                                        })
                                        .await,
                                    )
                                },
                            )
                            .await
                            {
                                core::ops::ControlFlow::Break(number) => number,
                                core::ops::ControlFlow::Continue(number) => number,
                            };
                            worker.send::<Answer>(&(number + 1)).await.unwrap();
                            worker.send::<Loan>(db).await.unwrap();
                        })
                        .await;
                        Ok::<_, hibana::EndpointError>(())
                    };
                }
                .await
                .expect("all child results are data");
            };
            match hibana_runtime::race(
                async {
                    let _: () =
                        (crate::test_support::in_process::sleep(Duration::from_secs(30))).await;
                    core::ops::ControlFlow::Break(())
                },
                async {
                    let _: () = (exchange).await;
                    core::ops::ControlFlow::Continue(())
                },
            )
            .await
            {
                core::ops::ControlFlow::Break(()) => {
                    panic!("choice addressed to a co-resident logical view stopped progressing")
                }
                core::ops::ControlFlow::Continue(()) => {}
            }
        }
    };

    Ok(())
}
