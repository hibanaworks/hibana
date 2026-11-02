//! Nested actor work shares one progress state and preserves both fork inputs.
use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::time::Duration;

pub(super) type Steps = g::Seq<
    g::Send<0, 1, Loan>,
    g::Par<
        g::Seq<g::Par<g::Send<10, 2, Read>, g::Send<1, 3, Value>>, g::Send<10, 2, Value>>,
        g::Send<1, 4, Read>,
    >,
>;

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn nested_parallel_actor_work_keeps_a_sibling_free_while_the_join_waits()
-> core::result::Result<(), hibana::EndpointError> {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let resource_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<2>(&g::Program::<Steps>::new());
    let left_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let right_program = crate::runtime::program::project::<4>(&g::Program::<Steps>::new());
    {
        let exchange = async {
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, TestTransport::new())
                .unwrap();
            let sid = SessionId::new(862);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let db = rv.enter(sid, &resource_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut service = rv.enter(sid, &service_program).unwrap();
            let mut left = rv.enter(sid, &left_program).unwrap();
            let mut right = rv.enter(sid, &right_program).unwrap();

            owner.send::<Loan>(db).await.unwrap();
            let mut db = worker.recv::<Loan>().await.unwrap();
            db.send::<Read>(&7).await.unwrap();
            assert_eq!(service.recv::<Read>().await.unwrap(), 7);

            let joined = db.send::<Value>(&22);
            let mut joined = core::pin::pin!(joined);
            match hibana_runtime::race(
                async { core::ops::ControlFlow::Break((&mut joined).await) },
                async {
                    core::ops::ControlFlow::Continue(
                        (async {
                            worker.send::<Read>(&31).await.unwrap();
                            right.recv::<Read>().await.unwrap()
                        })
                        .await,
                    )
                },
            )
            .await
            {
                core::ops::ControlFlow::Break(result) => {
                    panic!("inner join advanced before its prerequisite: {result:?}")
                }
                core::ops::ControlFlow::Continue(value) => assert_eq!(value, 31),
            }
            match hibana_runtime::race(
                async { core::ops::ControlFlow::Break((&mut joined).await) },
                async {
                    let _: () = (hibana_runtime::yield_now()).await;
                    core::ops::ControlFlow::Continue(())
                },
            )
            .await
            {
                core::ops::ControlFlow::Break(result) => {
                    panic!("independent sibling released the inner join: {result:?}")
                }
                core::ops::ControlFlow::Continue(()) => {}
            }

            let (sent, left_value, service_value) = async {
                async { Ok::<_, hibana::EndpointError>((&mut joined).await) };

                async {
                    Ok::<_, hibana::EndpointError>(
                        (async {
                            worker.send::<Value>(&8).await.unwrap();
                            left.recv::<Value>().await.unwrap()
                        })
                        .await,
                    )
                };

                async { Ok::<_, hibana::EndpointError>((service.recv::<Value>()).await) };
            }
            .await
            .expect("all child results are data");
            sent.unwrap();
            assert_eq!(left_value, 8);
            assert_eq!(service_value.unwrap(), 22);
        };
        match hibana_runtime::race(
            async {
                let _: () = (crate::test_support::in_process::sleep(Duration::from_secs(10))).await;
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
                panic!("the send did not resume after its prerequisite committed")
            }
            core::ops::ControlFlow::Continue(()) => {}
        }
    };

    Ok(())
}
