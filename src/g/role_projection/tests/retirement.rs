//! A completed view can retire while independent projected work remains.
use super::*;
use crate::{
    endpoint::proof_snapshot::{actor_fault, actor_snapshot, view_tag},
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

#[cfg(not(miri))]
mod observations;
#[cfg(not(miri))]
pub(super) use observations::{RetirementOrigin, RetirementSample, write_retirements};

#[test]
fn a_completed_view_retires_without_cancelling_independent_work() {
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Send<10, 3, Read>,
            g::Seq<g::Par<g::Send<1, 4, Value>, g::Send<11, 5, Value>>, g::Send<1, 6, OtherLoan>>,
        >,
    >;
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let value_program = crate::runtime::program::project::<4>(&g::Program::<Steps>::new());
    let auth_peer_program = crate::runtime::program::project::<5>(&g::Program::<Steps>::new());
    let receiver_program = crate::runtime::program::project::<6>(&g::Program::<Steps>::new());
    let auth_program = crate::runtime::program::project::<11>(&g::Program::<Steps>::new());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let network = TestTransport::new();
        let mut slab = std::vec![0; 80 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, network.clone())
            .unwrap();
        let sid = SessionId::new(9041);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &db_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut auth = rv.enter(sid, &auth_program).unwrap();
        let mut service = rv.enter(sid, &service_program).unwrap();
        let mut value_peer = rv.enter(sid, &value_program).unwrap();
        let mut auth_peer = rv.enter(sid, &auth_peer_program).unwrap();
        let mut receiver = rv.enter(sid, &receiver_program).unwrap();
        owner.send::<Loan>(db).await.unwrap();
        let mut db = worker.recv::<Loan>().await.unwrap();
        db.send::<Read>(&7).await.unwrap();
        assert_eq!(service.recv::<Read>().await.unwrap(), 7);
        let tag = view_tag(&db);
        // SAFETY: WORKER retains this TestTransport actor and its issued DB view.
        let before = unsafe { actor_snapshot(&worker) };
        assert_eq!(before.fault, None);
        drop(db);
        // SAFETY: retiring DB does not destroy the worker's distinct view.
        let after = unsafe { actor_snapshot(&worker) };
        assert_eq!(before.steps, after.steps);
        assert_eq!(before.choices, after.choices);
        assert_eq!(before.counter, after.counter);
        assert_eq!(before.completion_mutations, after.completion_mutations);
        assert_eq!(before.retained_rolls, after.retained_rolls);
        assert_eq!(before.live_views, after.live_views + 1);
        // SAFETY: storage still retains the retired DB record.
        assert!(!unsafe { tag.matches_header() });
        assert_eq!(after.fault, None);
        // Controlled transport delay proves the completed view's drop does not
        // make an independent projected send wait for its sibling's readiness.
        network.pause_send(1);
        let (worker_sent, auth_sent, (), ()) = tokio::join!(
            worker.send::<Value>(&8),
            auth.send::<Value>(&9),
            async {
                assert_eq!(auth_peer.recv::<Value>().await.unwrap(), 9);
                network.resume_send();
            },
            async {
                assert_eq!(value_peer.recv::<Value>().await.unwrap(), 8);
            },
        );
        worker_sent.unwrap();
        auth_sent.unwrap();
        worker.send::<OtherLoan>(auth).await.unwrap();
        drop(receiver.recv::<OtherLoan>().await.unwrap());
        // SAFETY: WORKER is live after both independent operations complete.
        assert_eq!(unsafe { actor_fault(&worker) }, None);
        assert!(network.queue_is_empty());
        #[cfg(not(miri))]
        {
            use crate::test_support::{
                lean_choreo::LeanChoreo, lean_descriptor::descriptor_source,
            };
            write_retirements(
                "CompletedViewRetirementGenerated",
                Steps::lean_source(),
                descriptor_source(
                    crate::global::compiled::images::RoleDescriptorRef::from_resident(
                        &RoleProjection::<1, Steps, 8>::IMAGE_REF,
                    ),
                ),
                &[RetirementSample {
                    name: "completed",
                    before,
                    after,
                    origin: RetirementOrigin::Application,
                    complete: true,
                    constructor: None,
                }],
            );
        }
    });
}
