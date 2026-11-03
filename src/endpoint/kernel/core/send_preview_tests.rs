use crate::{
    Endpoint, g,
    runtime::{SessionKitStorage, ids::SessionId, program::project, resolver::ResolverRef},
    test_support::transport::TestTransport,
};

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_send_before_its_remaining_receive_does_not_invoke_the_resolver()
-> core::result::Result<(), hibana::EndpointError> {
    type Start = g::Msg<10, ()>;
    type Need = g::Msg<11, ()>;
    type Left = g::Msg<12, ()>;
    type Right = g::Msg<13, ()>;
    let global = g::seq(
        g::seq(g::send::<0, 1, Start>(), g::send::<2, 1, Need>()),
        g::route(g::send::<1, 0, Left>(), g::send::<1, 0, Right>()).resolve::<17>(),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let network = TestTransport::new();
    let mut owner_slab = [0; 32 * 1024];
    let mut worker_slab = [0; 32 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let owner_runtime = owner_storage
        .init()
        .rendezvous(&mut owner_slab, network.clone())
        .unwrap();
    let worker_runtime = worker_storage
        .init()
        .rendezvous(&mut worker_slab, network.clone())
        .unwrap();
    worker_runtime
        .set_resolver(
            &worker_program,
            ResolverRef::<17>::decision_state(&(), |_| {
                panic!("resolver invoked before its required receive")
            }),
        )
        .unwrap();
    let session = SessionId::new(1700);
    let mut owner = owner_runtime.enter(session, &owner_program).unwrap();
    let mut worker = worker_runtime.enter(session, &worker_program).unwrap();
    async {
        owner.send::<Start>(&()).await.unwrap();
        worker.recv::<Start>().await.unwrap();
        let result = (worker.send::<Left>(&())).await;
        assert!(
            result.is_err(),
            "send before the required receive was accepted"
        );
    }
    .await;
    assert!(network.queue_is_empty());

    Ok(())
}

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn a_received_endpoint_does_not_enable_the_later_resolver_before_its_receive()
-> core::result::Result<(), hibana::EndpointError> {
    type Loan<'r> = g::Msg<10, Endpoint<'r, 3>>;
    type Need = g::Msg<11, ()>;
    type Left = g::Msg<12, ()>;
    type Right = g::Msg<13, ()>;
    let global = g::seq(
        g::seq(g::send::<0, 1, Loan>(), g::send::<2, 1, Need>()),
        g::route(g::send::<1, 0, Left>(), g::send::<1, 0, Right>()).resolve::<17>(),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let resource_program = project::<3>(&global);
    let network = TestTransport::new();
    let mut owner_slab = [0; 32 * 1024];
    let mut worker_slab = [0; 32 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let owner_runtime = owner_storage
        .init()
        .rendezvous(&mut owner_slab, network.clone())
        .unwrap();
    let worker_runtime = worker_storage
        .init()
        .rendezvous(&mut worker_slab, network.clone())
        .unwrap();
    worker_runtime
        .set_resolver(
            &worker_program,
            ResolverRef::<17>::decision_state(&(), |_| {
                panic!("resolver invoked before its post-delegation receive")
            }),
        )
        .unwrap();
    let session = SessionId::new(1701);
    let mut owner = owner_runtime.enter(session, &owner_program).unwrap();
    let mut worker = worker_runtime.enter(session, &worker_program).unwrap();
    let resource = owner_runtime.enter(session, &resource_program).unwrap();
    async {
        owner.send::<Loan>(resource).await.unwrap();
        let _resource = worker.recv::<Loan>().await.unwrap();
        let result = (worker.send::<Left>(&())).await;
        assert!(
            result.is_err(),
            "send before the required receive was accepted"
        );
    }
    .await;
    assert!(network.queue_is_empty());

    Ok(())
}
