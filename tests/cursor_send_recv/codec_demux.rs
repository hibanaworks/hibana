use super::*;

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn recv_codec_error_poisons_before_same_generation_continuation() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::send::<0, 1, Msg<1, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(12);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            (origin_endpoint.send::<Msg<1, u32>>(&42))
                .await
                .expect("send succeeds");
            transport.truncate_next_payload(1, 3);

            let err = match (target_endpoint.recv::<Msg<1, u32>>()).await {
                Ok(_) => panic!("recv with malformed bytes must fail"),
                Err(err) => err,
            };
            assert!(format!("{err:?}").contains("operation: \"recv\""));
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("Codec"),
                "first recv fault must preserve codec evidence: {rendered}"
            );
            assert!(
                !rendered.contains("SessionFault"),
                "first recv fault must not be replaced by session poison: {rendered}"
            );

            let err = match (target_endpoint.recv::<Msg<1, u32>>()).await {
                Ok(_) => {
                    panic!("poisoned generation must not continue after recv decode fault")
                }
                Err(err) => err,
            };
            assert!(format!("{err:?}").contains("operation: \"recv\""));
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("SessionFault") && rendered.contains("DecodeFailed"),
                "continuation must report the poisoned session cause: {rendered}"
            );
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn cursor_send_and_recv_high_logical_label_roundtrip() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::send::<0, 1, Msg<200, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(200);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            let () = (origin_endpoint.send::<Msg<200, u32>>(&0xC8C8_C8C8))
                .await
                .expect("send succeeds");
            let payload = (target_endpoint.recv::<Msg<200, u32>>())
                .await
                .expect("recv succeeds");
            assert_eq!(payload, 0xC8C8_C8C8);
            assert!(transport.queue_is_empty());
        };
    };
}
