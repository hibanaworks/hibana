use super::*;

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn forgotten_recv_future_leaves_endpoint_fail_closed() {
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    let transport = TestTransport::new();
    {
        let program = g::send::<0, 1, Msg<55, u32>>();
        let origin_program: RoleProgram<0> = project(&program);
        let target_program: RoleProgram<1> = project(&program);
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");

        let sid = SessionId::new(255);
        let origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
        core::hint::black_box(&origin_endpoint);
        let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

        let future = target_endpoint.recv::<Msg<55, u32>>();
        core::mem::forget(future);

        let error = target_endpoint
            .recv::<Msg<55, u32>>()
            .await
            .expect_err("forgotten recv future must leave endpoint fail-closed");
        assert!(format!("{error:?}").contains("operation: \"recv\""));
        assert!(
            format!("{error:?}").contains("PhaseInvariant")
                || format!("{error:?}").contains("ProgressInvariantViolated"),
            "busy endpoint must report terminal progress evidence: {error:?}"
        );
    }
}
