//! The same global drives in-memory and native-process fixtures.
use super::*;
use crate::runtime::{
    SessionKitStorage,
    ids::SessionId,
    resolver::{DecisionArm, ResolverRef},
};
use crate::test_support::transport::TestTransport;
use core::cell::Cell;

pub(super) type Subtract = g::Msg<6, u32>;
pub(super) type AfterRead = g::Msg<7, u32>;
pub(super) type AfterValue = g::Msg<8, u32>;
pub(super) type Finished = g::Msg<9, Endpoint<'static, 10>>;
type Change = g::Resolve<
    g::Route<
        g::Seq<g::Send<10, 2, Read>, g::Send<2, 10, Value>>,
        g::Seq<g::Send<10, 2, Subtract>, g::Send<2, 10, Value>>,
    >,
    702,
>;
type LoanCycle = g::Roll<g::Seq<g::Send<0, 1, Loan>, g::Seq<Change, g::Send<1, 0, Loan>>>>;
type Side = g::Roll<g::Seq<g::Send<3, 4, Read>, g::Send<4, 3, Value>>>;
pub(super) type Global = g::Seq<
    g::Send<2, 0, Loan>,
    g::Seq<
        g::Par<LoanCycle, Side>,
        g::Seq<
            g::Send<10, 2, AfterRead>,
            g::Seq<g::Send<2, 10, AfterValue>, g::Send<0, 2, Finished>>,
        >,
    >,
>;

#[test]
fn service_grants_a_resource_then_observes_choices_in_parallel_rolled_loans() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let () = g::ProgramProjection::<Global, 32>::VALIDATION;
        let owner_program = crate::global::role_program::role_program_from_image::<0>(
            &RoleProjection::<0, Global, 32>::IMAGE_REF,
        );
        let worker_program = crate::global::role_program::role_program_from_image::<1>(
            &RoleProjection::<1, Global, 32>::IMAGE_REF,
        );
        let service_program = crate::global::role_program::role_program_from_image::<2>(
            &RoleProjection::<2, Global, 32>::IMAGE_REF,
        );
        let db_program = crate::global::role_program::role_program_from_image::<10>(
            &RoleProjection::<10, Global, 32>::IMAGE_REF,
        );
        let left_program = crate::global::role_program::role_program_from_image::<3>(
            &RoleProjection::<3, Global, 32>::IMAGE_REF,
        );
        let right_program = crate::global::role_program::role_program_from_image::<4>(
            &RoleProjection::<4, Global, 32>::IMAGE_REF,
        );
        let network = TestTransport::new();
        let mut owner_slab = std::vec![0; 64 * 1024];
        let mut worker_slab = std::vec![0; 64 * 1024];
        let mut service_slab = std::vec![0; 64 * 1024];
        let mut side_slab = std::vec![0; 64 * 1024];
        let delta = Cell::new(3u32);
        let value = Cell::new(41u32);
        let mut owner_storage = SessionKitStorage::uninit();
        let mut worker_storage = SessionKitStorage::uninit();
        let mut service_storage = SessionKitStorage::uninit();
        let mut side_storage = SessionKitStorage::uninit();
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
            .rendezvous(&mut service_slab, network.clone())
            .unwrap();
        let side_rv = side_storage
            .init()
            .rendezvous(&mut side_slab, network)
            .unwrap();
        worker_rv
            .set_resolver(
                &db_program,
                ResolverRef::<702>::decision_state(&delta, |number| {
                    Ok(if number.get() % 2 == 1 {
                        DecisionArm::Left
                    } else {
                        DecisionArm::Right
                    })
                }),
            )
            .unwrap();
        let sid = SessionId::new(847);
        let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
        let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
        let mut service = service_rv.enter(sid, &service_program).unwrap();
        let db = service_rv.enter(sid, &db_program).unwrap();
        let mut left = side_rv.enter(sid, &left_program).unwrap();
        let mut right = side_rv.enter(sid, &right_program).unwrap();
        tokio::join!(
            async {
                service.send::<Loan>(db).await.unwrap();
                for expected in [44, 40] {
                    let branch = service.offer().await.unwrap_or_else(|error| {
                        panic!("service offer before value={expected}: {error:?}")
                    });
                    let updated = match branch.label() {
                        3 => value.get() + branch.recv::<Read>().await.unwrap(),
                        6 => value.get() - branch.recv::<Subtract>().await.unwrap(),
                        label => panic!("unexpected choice {label}"),
                    };
                    assert_eq!(updated, expected);
                    value.set(updated);
                    service.send::<Value>(&updated).await.unwrap();
                }
                let updated = value.get() + service.recv::<AfterRead>().await.unwrap();
                value.set(updated);
                service.send::<AfterValue>(&updated).await.unwrap();
                drop(service.recv::<Finished>().await.unwrap());
            },
            async {
                let mut db = owner.recv::<Loan>().await.unwrap();
                for _ in 0..2 {
                    owner.send::<Loan>(db).await.unwrap();
                    db = owner.recv::<Loan>().await.unwrap();
                }
                db.send::<AfterRead>(&5).await.unwrap();
                assert_eq!(db.recv::<AfterValue>().await.unwrap(), 45);
                owner.send::<Finished>(db).await.unwrap();
            },
            async {
                for (number, expected) in [(3, 44), (4, 40)] {
                    delta.set(number);
                    let mut db = worker.recv::<Loan>().await.unwrap();
                    let branch = db.offer().await.unwrap();
                    match branch.label() {
                        3 => branch.send::<Read>(&number).await.unwrap(),
                        6 => branch.send::<Subtract>(&number).await.unwrap(),
                        label => panic!("unexpected resolver choice {label}"),
                    }
                    assert_eq!(db.recv::<Value>().await.unwrap(), expected);
                    worker.send::<Loan>(db).await.unwrap();
                }
            },
            async {
                for number in [10, 20] {
                    left.send::<Read>(&number).await.unwrap();
                    assert_eq!(left.recv::<Value>().await.unwrap(), number + 1);
                }
            },
            async {
                for number in [10, 20] {
                    assert_eq!(right.recv::<Read>().await.unwrap(), number);
                    right.send::<Value>(&(number + 1)).await.unwrap();
                }
            }
        );
        assert_eq!(value.get(), 45);
    });
}
