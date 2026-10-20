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

#[test]
fn nested_parallel_actor_work_keeps_a_sibling_free_while_the_join_waits() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let resource_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<2>(&g::Program::<Steps>::new());
    let left_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let right_program = crate::runtime::program::project::<4>(&g::Program::<Steps>::new());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
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
                tokio::pin!(joined);
                tokio::select! {
                    biased;
                    result = &mut joined => panic!("inner join advanced before its prerequisite: {result:?}"),
                    value = async {
                        worker.send::<Read>(&31).await.unwrap();
                        right.recv::<Read>().await.unwrap()
                    } => assert_eq!(value, 31),
                }
                tokio::select! {
                    biased;
                    result = &mut joined => panic!("independent sibling released the inner join: {result:?}"),
                    () = tokio::task::yield_now() => {},
                }

                let (sent, left_value, service_value) = tokio::join!(
                    biased;
                    &mut joined,
                    async {
                        worker.send::<Value>(&8).await.unwrap();
                        left.recv::<Value>().await.unwrap()
                    },
                    service.recv::<Value>(),
                );
                sent.unwrap();
                assert_eq!(left_value, 8);
                assert_eq!(service_value.unwrap(), 22);
            };
            tokio::select! {
                biased;
                () = tokio::time::sleep(Duration::from_secs(10)) => {
                    panic!("Tokio did not resume the joined send after its prerequisite committed")
                }
                () = exchange => {},
            }
        });
}
