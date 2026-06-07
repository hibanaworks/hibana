use super::*;

#[test]
fn recv_codec_error_poisons_before_same_generation_continuation() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<1, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(12);
            let mut origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");

            futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<1, u32>>()
                    .expect("send flow")
                    .send(&42),
            )
            .expect("send succeeds");

            let recv_line = line!() + 1;
            let err = match futures::executor::block_on(target_endpoint.recv::<Msg<1, u64>>()) {
                Ok(_) => panic!("recv with wrong payload shape must fail"),
                Err(err) => err,
            };
            assert_eq!(err.operation(), "recv");
            assert!(
                err.file()
                    .ends_with("tests/cursor_send_recv/codec_demux.rs")
            );
            assert_eq!(err.line(), recv_line);
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("Codec"),
                "first recv fault must preserve codec evidence: {rendered}"
            );
            assert!(
                !rendered.contains("SessionFault"),
                "first recv fault must not be replaced by session poison: {rendered}"
            );

            let continuation_line = line!() + 1;
            let err = match futures::executor::block_on(target_endpoint.recv::<Msg<1, u32>>()) {
                Ok(_) => {
                    panic!("poisoned generation must not continue after recv decode fault")
                }
                Err(err) => err,
            };
            assert_eq!(err.operation(), "recv");
            assert!(
                err.file()
                    .ends_with("tests/cursor_send_recv/codec_demux.rs")
            );
            assert_eq!(err.line(), continuation_line);
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
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<200, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(200);
            let mut origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");

            let () = futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<200, u32>>()
                    .expect("send flow")
                    .send(&0xC8C8_C8C8),
            )
            .expect("send succeeds");
            let payload = futures::executor::block_on(target_endpoint.recv::<Msg<200, u32>>())
                .expect("recv succeeds");
            assert_eq!(payload, 0xC8C8_C8C8);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn custom_label_universe_rejects_high_logical_label_on_enter() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&LOW_LABEL_SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<200, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::<LowLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let bad_sid = SessionId::new(201);
            let enter_line = line!() + 1;
            let enter_result = rv.session(bad_sid).role(&origin_program).enter();
            let err = match enter_result {
                Ok(_) => panic!("custom label universe must reject high logical label"),
                Err(err) => err,
            };

            let debug = format!("{err:?}");
            assert_eq!(err.operation(), "enter");
            assert!(
                err.file()
                    .ends_with("tests/cursor_send_recv/codec_demux.rs")
            );
            assert_eq!(err.line(), enter_line);
            assert!(debug.contains("label 200 > rv-label 127"));
            assert!(transport_queue_is_empty(&transport));
        });
    });
}
