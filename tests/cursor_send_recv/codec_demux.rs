use super::*;

#[test]
fn recv_codec_error_poisons_before_same_generation_continuation() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(12);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
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
            },
        );
    });
}

#[test]
fn demux_binding_without_policy_signals_keeps_empty_transport_payload_nonsemantic() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(13);
                let origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                core::hint::black_box(&origin_endpoint);
                let binding = Box::leak(Box::new(DemuxOnlyBinding));
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(binding)
                    .expect("target endpoint");

                let mut tx = TestTx::default();
                transport.stage_send(&mut tx, 1, 0, 1, &[]);
                assert!(matches!(
                    transport.poll_send_staged(&mut tx),
                    Poll::Ready(Ok(()))
                ));

                let mut recv_future = std::pin::pin!(target_endpoint.recv::<Msg<1, u8>>());
                let waker = futures::task::noop_waker_ref();
                let mut context = Context::from_waker(waker);
                match recv_future.as_mut().poll(&mut context) {
                    Poll::Pending => {}
                    Poll::Ready(Ok(value)) => {
                        panic!("empty transport payload was accepted as semantic data: {value}")
                    }
                    Poll::Ready(Err(error)) => {
                        panic!(
                            "binding without policy signals must wait for binding evidence, got {error:?}"
                        )
                    }
                }
                assert!(
                    transport_queue_is_empty(&transport),
                    "nonsemantic empty demux turns must not be requeued as payload"
                );
            },
        );
    });
}

#[test]
fn cursor_send_and_recv_high_logical_label_roundtrip() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<200, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let target_program: RoleProgram<1> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(200);
                let mut origin_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&origin_program)
                    .enter(NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&target_program)
                    .enter(NoBinding)
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
            },
        );
    });
}

#[test]
fn custom_label_universe_rejects_high_logical_label_on_enter() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(
            &LOW_LABEL_SESSION_SLOT,
            |storage| unsafe { SessionKit::init_in_place(storage) },
            |cluster| {
                let program = g::send::<Role<0>, Role<1>, Msg<200, u32>, 0>();
                let origin_program: RoleProgram<0> = project(&program);
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<LowLabelUniverse, _>::from_resources(
                            (tap_buf, slab),
                            CounterClock::new(),
                        ),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let bad_sid = SessionId::new(201);
                let enter_line = line!() + 5;
                let enter_result = cluster
                    .rendezvous(rv_id)
                    .session(bad_sid)
                    .role(&origin_program)
                    .enter(NoBinding);
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
                assert!(debug.contains("LabelOutOfUniverse"));
                assert!(debug.contains("max: 127"));
                assert!(debug.contains("actual: 200"));
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}
