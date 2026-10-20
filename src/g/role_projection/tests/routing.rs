//! Nested route previews on a delegated view preserve the independent sibling.
use super::*;
use crate::{
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        resolver::{DecisionArm, ResolverRef},
    },
    test_support::transport::TestTransport,
};
use core::cell::Cell;
use core::time::Duration;

type Nested = g::Msg<6, u32>;
type Alternative = g::Msg<7, u32>;
type NestedValue = g::Msg<8, u32>;
type AlternativeValue = g::Msg<9, u32>;
pub(super) type Steps = g::Seq<
    g::Send<0, 1, Loan>,
    g::Seq<
        g::Par<
            g::Resolve<
                g::Route<
                    g::Resolve<
                        g::Route<
                            g::Seq<g::Send<10, 2, Read>, g::Send<2, 10, Value>>,
                            g::Seq<g::Send<10, 2, Nested>, g::Send<2, 10, NestedValue>>,
                        >,
                        705,
                    >,
                    g::Seq<g::Send<10, 2, Alternative>, g::Send<2, 10, AlternativeValue>>,
                >,
                704,
            >,
            g::Send<1, 3, Value>,
        >,
        g::Send<1, 0, Loan>,
    >,
>;

#[test]
fn delegated_nested_choices_keep_preview_and_parallel_progress_separate() {
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<2>(&g::Program::<Steps>::new());
    let side_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for input in [0u32, 1, 2] {
                let policy_input = Cell::new(input);
                let mut slab = [0u8; 64 * 1024];
                let mut storage = SessionKitStorage::<TestTransport>::uninit();
                let rv = storage
                    .init()
                    .rendezvous(&mut slab, TestTransport::new())
                    .unwrap();
                rv.set_resolver(
                    &db_program,
                    ResolverRef::<704>::decision_state(&policy_input, |input| {
                        Ok(if input.get() < 2 {
                            DecisionArm::Left
                        } else {
                            DecisionArm::Right
                        })
                    }),
                )
                .unwrap();
                rv.set_resolver(
                    &db_program,
                    ResolverRef::<705>::decision_state(&policy_input, |input| {
                        Ok(if input.get() == 0 {
                            DecisionArm::Left
                        } else {
                            DecisionArm::Right
                        })
                    }),
                )
                .unwrap();
                let sid = SessionId::new(863);
                let mut owner = rv.enter(sid, &owner_program).unwrap();
                let db = rv.enter(sid, &db_program).unwrap();
                let mut worker = rv.enter(sid, &worker_program).unwrap();
                let mut service = rv.enter(sid, &service_program).unwrap();
                let mut side = rv.enter(sid, &side_program).unwrap();
                let exchange = async {
                    tokio::join!(
                        async {
                            owner.send::<Loan>(db).await.unwrap();
                            drop(owner.recv::<Loan>().await.unwrap());
                        },
                        async {
                            let mut db = worker.recv::<Loan>().await.unwrap();
                            let (received, side_sent) = tokio::join!(
                                async {
                                    let preview = db.offer().await.unwrap();
                                    let label = preview.label();
                                    drop(preview);
                                    tokio::task::yield_now().await;
                                    let branch = db.offer().await.unwrap();
                                    assert_eq!(branch.label(), label);
                                    match branch.label() {
                                        3 => {
                                            branch.send::<Read>(&(input + 7)).await.unwrap();
                                            policy_input.set((input + 1) % 3);
                                            db.recv::<Value>().await.unwrap()
                                        }
                                        6 => {
                                            branch.send::<Nested>(&(input + 7)).await.unwrap();
                                            policy_input.set((input + 1) % 3);
                                            db.recv::<NestedValue>().await.unwrap()
                                        }
                                        7 => {
                                            branch.send::<Alternative>(&(input + 7)).await.unwrap();
                                            policy_input.set((input + 1) % 3);
                                            db.recv::<AlternativeValue>().await.unwrap()
                                        }
                                        label => panic!("unexpected delegated branch {label}"),
                                    }
                                },
                                worker.send::<Value>(&31),
                            );
                            assert_eq!(received, input + 8);
                            side_sent.unwrap();
                            worker.send::<Loan>(db).await.unwrap();
                        },
                        async {
                            let branch = service.offer().await.unwrap();
                            let value = match branch.label() {
                                3 => {
                                    let value = branch.recv::<Read>().await.unwrap();
                                    service.send::<Value>(&(value + 1)).await.unwrap();
                                    value
                                }
                                6 => {
                                    let value = branch.recv::<Nested>().await.unwrap();
                                    service.send::<NestedValue>(&(value + 1)).await.unwrap();
                                    value
                                }
                                7 => {
                                    let value = branch.recv::<Alternative>().await.unwrap();
                                    service
                                        .send::<AlternativeValue>(&(value + 1))
                                        .await
                                        .unwrap();
                                    value
                                }
                                label => panic!("unexpected service branch {label}"),
                            };
                            assert_eq!(value, input + 7);
                        },
                        async { assert_eq!(side.recv::<Value>().await.unwrap(), 31) },
                    );
                };
                tokio::select! {
                    biased;
                    () = tokio::time::sleep(Duration::from_secs(30)) => {
                        panic!("nested delegated choices stopped progressing for input {input}")
                    }
                    () = exchange => {},
                }
            }
        });
}
