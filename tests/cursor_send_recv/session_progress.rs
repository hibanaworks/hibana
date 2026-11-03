use super::*;

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn sequential_noncontiguous_lane_steps_progress_in_order() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::seq(
                g::send::<0, 1, Msg<31, u32>>(),
                g::seq(
                    g::send::<0, 1, Msg<32, u32>>(),
                    g::send::<0, 1, Msg<33, u32>>(),
                ),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(31);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            (origin_endpoint.send::<Msg<31, u32>>(&31))
                .await
                .expect("lane 0 first send");
            (origin_endpoint.send::<Msg<32, u32>>(&32))
                .await
                .expect("lane 1 middle send");
            (origin_endpoint.send::<Msg<33, u32>>(&33))
                .await
                .expect("lane 0 final send");

            assert_eq!(
                (target_endpoint.recv::<Msg<31, u32>>())
                    .await
                    .expect("lane 0 first recv"),
                31
            );
            assert_eq!(
                (target_endpoint.recv::<Msg<32, u32>>())
                    .await
                    .expect("lane 1 middle recv"),
                32
            );
            assert_eq!(
                (target_endpoint.recv::<Msg<33, u32>>())
                    .await
                    .expect("lane 0 final recv"),
                33
            );
            assert!(transport.queue_is_empty());
        };
    };
}
