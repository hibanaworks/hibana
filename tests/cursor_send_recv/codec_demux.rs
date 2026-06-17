use super::*;

#[test]
fn recv_codec_error_poisons_before_same_generation_continuation() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(12);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            futures::executor::block_on(origin_endpoint.send::<Msg<1, u32>>(&42))
                .expect("send succeeds");

            let err = match futures::executor::block_on(target_endpoint.recv::<Msg<1, u64>>()) {
                Ok(_) => panic!("recv with wrong payload shape must fail"),
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

            let err = match futures::executor::block_on(target_endpoint.recv::<Msg<1, u32>>()) {
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
        });
    });
}

#[test]
fn cursor_send_and_recv_high_logical_label_roundtrip() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<200, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");

            let sid = SessionId::new(200);
            let mut origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            let () =
                futures::executor::block_on(origin_endpoint.send::<Msg<200, u32>>(&0xC8C8_C8C8))
                    .expect("send succeeds");
            let payload = futures::executor::block_on(target_endpoint.recv::<Msg<200, u32>>())
                .expect("recv succeeds");
            assert_eq!(payload, 0xC8C8_C8C8);
            assert!(transport.queue_is_empty());
        });
    });
}
