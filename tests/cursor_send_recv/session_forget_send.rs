use super::*;

#[test]
fn forgotten_unpolled_send_future_does_not_publish_runtime_progress() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, Msg<53, u32>>(),
                g::send::<0, 1, Msg<54, u32>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");

            let sid = SessionId::new(253);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");
            core::hint::black_box(&target_endpoint);

            let payload = 53u32;
            let future = origin_endpoint.send::<Msg<53, u32>>(&payload);
            core::mem::forget(future);

            futures::executor::block_on(origin_endpoint.send::<Msg<53, u32>>(&payload))
                .expect("forgotten unpolled send future must not acquire a runtime send lease");
        });
    });
}
