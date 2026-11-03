use super::*;

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn forgotten_unpolled_send_future_does_not_publish_runtime_progress() {
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    let transport = TestTransport::new();
    {
        let program = g::seq(
            g::send::<0, 1, Msg<53, u32>>(),
            g::send::<0, 1, Msg<54, u32>>(),
        );
        let origin_program: RoleProgram<0> = project(&program);
        let target_program: RoleProgram<1> = project(&program);
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");

        let sid = SessionId::new(253);
        let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
        let target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");
        core::hint::black_box(&target_endpoint);

        let payload = 53u32;
        let future = origin_endpoint.send::<Msg<53, u32>>(&payload);
        core::mem::forget(future);

        origin_endpoint
            .send::<Msg<53, u32>>(&payload)
            .await
            .expect("forgotten unpolled send future must not acquire a runtime send lease");
    }
}
