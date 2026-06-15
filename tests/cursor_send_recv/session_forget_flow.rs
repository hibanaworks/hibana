use super::*;

#[test]
fn forgotten_flow_leaves_endpoint_fail_closed() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, Msg<51, u32>>(),
                g::send::<0, 1, Msg<52, u32>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(Config::from_resources(slab), transport)
                .expect("register rendezvous");

            let sid = SessionId::new(251);
            let mut origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            let target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");
            core::hint::black_box(&target_endpoint);

            let flow = origin_endpoint
                .flow::<Msg<51, u32>>()
                .expect("first flow preview");
            core::mem::forget(flow);

            match origin_endpoint.flow::<Msg<51, u32>>() {
                Ok(_) => panic!("forgotten flow must reject even the same send preview"),
                Err(error) => {
                    assert_eq!(error.operation(), "flow");
                    assert!(
                        format!("{error:?}").contains("PhaseInvariant"),
                        "busy endpoint must report phase invariant evidence: {error:?}"
                    );
                }
            }
        });
    });
}
