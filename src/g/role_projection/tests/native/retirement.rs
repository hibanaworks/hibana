//! A completed visit may retire while a peer attempts the next visit.
use super::*;
use crate::transport::TransportError;

#[tokio::test(flavor = "current_thread")]
async fn a_completed_resource_can_retire_without_stranding_a_fresh_peer_visit() {
    type Steps =
        g::Seq<g::Send<0, 1, Loan>, g::Par<g::Roll<g::Send<10, 2, Read>>, g::Send<1, 3, Value>>>;
    const LANES: usize = g::ProgramProjection::<Steps, 8>::SOURCE_EFF_LIST.len();
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<Steps>::new());
    let service_program = crate::runtime::program::project::<2>(&g::Program::<Steps>::new());
    let observer_program = crate::runtime::program::project::<3>(&g::Program::<Steps>::new());
    let mailboxes = Mailboxes::new(&[0, 1, 2, 3, 10], LANES).unwrap();
    // SAFETY: this is the sole initial issuer in this private native namespace;
    // the binding outlives both runtimes, every Endpoint and all queued frames.
    let binding = unsafe { crate::runtime::transport::EndpointBinding::new() };
    let left_io = NativeTransport::new(
        mailboxes.directory(),
        mailboxes.descriptors(&[0, 2, 10]).unwrap(),
        Some((10, &binding)),
    )
    .unwrap();
    let right_io = NativeTransport::new(
        mailboxes.directory(),
        mailboxes.descriptors(&[1, 3, 10]).unwrap(),
        None,
    )
    .unwrap();
    let mut left_slab = vec![0; 64 * 1024];
    let mut right_slab = vec![0; 64 * 1024];
    let mut left_storage = SessionKitStorage::uninit();
    let mut right_storage = SessionKitStorage::uninit();
    let left = left_storage
        .init()
        .rendezvous(&mut left_slab, left_io)
        .unwrap();
    let right = right_storage
        .init()
        .rendezvous(&mut right_slab, right_io)
        .unwrap();
    let sid = SessionId::new(9062);
    let mut owner = left.enter(sid, &owner_program).unwrap();
    let db = left.enter(sid, &db_program).unwrap();
    let mut worker = right.enter(sid, &worker_program).unwrap();
    let mut service = left.enter(sid, &service_program).unwrap();
    let mut observer = right.enter(sid, &observer_program).unwrap();
    let ((), value, next_visit, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(
            async {
                let mut db = worker.recv::<Loan>().await.unwrap();
                db.send::<Read>(&7).await.unwrap();
                drop(db);
                // A successful independent send establishes that retiring the
                // completed DB visit did not poison the worker's live actor.
                worker.send::<Value>(&9).await.unwrap();
            },
            observer.recv::<Value>(),
            async {
                assert_eq!(service.recv::<Read>().await.unwrap(), 7);
                service.recv::<Read>().await
            },
            async { owner.send::<Loan>(db).await.unwrap() },
        )
    })
    .await
    .expect("the native carrier deadline must end the fresh visit");
    assert_eq!(value.unwrap(), 9);
    let error = next_visit.expect_err("the retired value cannot send a second request");
    assert_eq!(
        std::format!("{error:?}"),
        std::format!(
            "EndpointError {{ operation: \"recv\", kind: Transport({:?}) }}",
            TransportError::Deadline
        ),
    );
    let repeated = service
        .recv::<Read>()
        .await
        .expect_err("the first transport fault must remain terminal");
    assert!(std::format!("{repeated:?}").contains("SessionFault(TransportClosed)"));
    assert!(
        left.enter(SessionId::new(9063), &db_program).is_err(),
        "completed retirement must not restore initial issuance"
    );
}
