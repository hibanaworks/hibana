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

#[test]
fn a_branch_frame_only_selects_the_addressed_logical_view() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
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
                    tokio::join!(
                        async {
                            owner.send::<Loan>(db).await.unwrap();
                            let branch = owner.offer().await.unwrap();
                            match branch.label() {
                                14 => branch.send::<WorkerInput>(&41).await.unwrap(),
                                15 => branch.send::<ResourceInput>(&41).await.unwrap(),
                                label => panic!("unexpected ingress choice {label}"),
                            }
                            assert_eq!(owner.recv::<Answer>().await.unwrap(), 42);
                            drop(owner.recv::<Loan>().await.unwrap());
                        },
                        async {
                            let mut db = worker.recv::<Loan>().await.unwrap();
                            let number = tokio::select! {
                                biased;
                                number = async {
                                    let branch = worker.offer().await.unwrap();
                                    assert_eq!(branch.label(), 14);
                                    branch.recv::<WorkerInput>().await.unwrap()
                                } => number,
                                number = async {
                                    let branch = db.offer().await.unwrap();
                                    assert_eq!(branch.label(), 15);
                                    branch.recv::<ResourceInput>().await.unwrap()
                                } => number,
                            };
                            worker.send::<Answer>(&(number + 1)).await.unwrap();
                            worker.send::<Loan>(db).await.unwrap();
                        },
                    );
                };
                tokio::select! {
                    biased;
                    () = tokio::time::sleep(Duration::from_secs(30)) => {
                        panic!("choice addressed to a co-resident logical view stopped progressing")
                    }
                    () = exchange => {},
                }
            }
        });
}
