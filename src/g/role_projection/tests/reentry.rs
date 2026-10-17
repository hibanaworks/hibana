//! Reenter a loan body without treating old continuation bits as fresh work.
use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use core::time::Duration;

pub(super) type Request = g::Msg<18, u32>;
pub(super) type Tail = g::Msg<20, ()>;
pub(super) type Done = g::Msg<21, ()>;
pub(super) type Steps = g::Seq<
    g::Roll<g::Seq<g::Send<0, 1, Loan>, g::Seq<g::Send<10, 1, Request>, g::Send<1, 0, Loan>>>>,
    g::Seq<g::Send<0, 1, Tail>, g::Send<1, 0, Done>>,
>;

#[test]
fn a_loan_reenters_after_its_sequential_continuation_without_reusing_old_work() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let mut slab = [0u8; 64 * 1024];
            let mut storage = SessionKitStorage::<TestTransport>::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, TestTransport::new())
                .unwrap();
            let sid = SessionId::new(865);
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let db = rv.enter(sid, &db_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let exchange = async {
                tokio::join!(
                    async {
                        owner.send::<Loan>(db).await.unwrap();
                        let db = owner.recv::<Loan>().await.unwrap();
                        owner.send::<Tail>(&()).await.unwrap();
                        owner.recv::<Done>().await.unwrap();
                        owner.send::<Loan>(db).await.unwrap();
                        drop(owner.recv::<Loan>().await.unwrap());
                    },
                    async {
                        let mut db = worker.recv::<Loan>().await.unwrap();
                        db.send::<Request>(&41).await.unwrap();
                        assert_eq!(worker.recv::<Request>().await.unwrap(), 41);
                        worker.send::<Loan>(db).await.unwrap();
                        worker.recv::<Tail>().await.unwrap();
                        worker.send::<Done>(&()).await.unwrap();

                        let mut db = worker.recv::<Loan>().await.unwrap();
                        {
                            let request = worker.recv::<Request>();
                            tokio::pin!(request);
                            tokio::select! {
                                biased;
                                result = &mut request => panic!("old visit released a fresh receive: {result:?}"),
                                () = tokio::task::yield_now() => {},
                            }
                            let (sent, received) = tokio::join!(
                                db.send::<Request>(&42),
                                &mut request,
                            );
                            sent.unwrap();
                            assert_eq!(received.unwrap(), 42);
                        }
                        worker.send::<Loan>(db).await.unwrap();
                    },
                );
            };
            tokio::select! {
                biased;
                () = tokio::time::sleep(Duration::from_secs(30)) => panic!("fresh loan visit stopped progressing"),
                () = exchange => {},
            }
        });
}
