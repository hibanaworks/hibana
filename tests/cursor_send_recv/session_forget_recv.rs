use super::*;

#[test]
fn forgotten_recv_future_leaves_endpoint_fail_closed() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<55, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(Config::from_resources(slab), transport)
                .expect("register rendezvous");

            let sid = SessionId::new(255);
            let origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            core::hint::black_box(&origin_endpoint);
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");

            let future = target_endpoint.recv::<Msg<55, u32>>();
            core::mem::forget(future);

            let error = futures::executor::block_on(target_endpoint.recv::<Msg<55, u32>>())
                .expect_err("forgotten recv future must leave endpoint fail-closed");
            assert_eq!(error.operation(), "recv");
            assert!(
                format!("{error:?}").contains("PhaseInvariant")
                    || format!("{error:?}").contains("ProgressInvariantViolated"),
                "busy endpoint must report terminal progress evidence: {error:?}"
            );
        });
    });
}
