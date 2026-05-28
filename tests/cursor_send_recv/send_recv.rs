use super::*;

#[test]
fn cursor_send_and_recv_roundtrip() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(1);
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
                    .flow::<Msg<1, u32>>()
                    .expect("send flow")
                    .send(&42),
            )
            .expect("send succeeds");
            let payload = futures::executor::block_on(target_endpoint.recv::<Msg<1, u32>>())
                .expect("recv succeeds");
            assert_eq!(payload, 42u32);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn completed_recv_future_repoll_is_fail_fast_and_does_not_advance_again() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<Role<0>, Role<1>, Msg<41, u32>, 0>(),
                g::send::<Role<0>, Role<1>, Msg<41, u32>, 0>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(41);
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
                    .flow::<Msg<41, u32>>()
                    .expect("first send flow")
                    .send(&11),
            )
            .expect("first send succeeds");
            futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<41, u32>>()
                    .expect("second send flow")
                    .send(&22),
            )
            .expect("second send succeeds");

            let mut recv_future = Box::pin(target_endpoint.recv::<Msg<41, u32>>());
            let waker = futures::task::noop_waker_ref();
            let mut context = Context::from_waker(waker);
            match Future::poll(recv_future.as_mut(), &mut context) {
                Poll::Ready(Ok(value)) => assert_eq!(value, 11),
                Poll::Ready(Err(error)) => panic!("first recv failed: {error:?}"),
                Poll::Pending => panic!("first recv must be ready"),
            }

            let repoll = catch_unwind(AssertUnwindSafe(|| {
                let _ = Future::poll(recv_future.as_mut(), &mut context);
            }));
            assert!(
                repoll.is_err(),
                "completed recv future must fail fast on post-Ready poll"
            );
            drop(recv_future);

            let second = futures::executor::block_on(target_endpoint.recv::<Msg<41, u32>>())
                .expect("second recv remains available");
            assert_eq!(
                second, 22,
                "completed recv future repoll must not consume the next descriptor"
            );
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn completed_send_future_repoll_is_fail_fast_and_does_not_advance_again() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<Role<0>, Role<1>, Msg<42, u32>, 0>(),
                g::send::<Role<0>, Role<1>, Msg<42, u32>, 0>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(42);
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

            let first = 11u32;
            let mut send_future = Box::pin(
                origin_endpoint
                    .flow::<Msg<42, u32>>()
                    .expect("first send flow")
                    .send(&first),
            );
            let waker = futures::task::noop_waker_ref();
            let mut context = Context::from_waker(waker);
            match Future::poll(send_future.as_mut(), &mut context) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(error)) => panic!("first send failed: {error:?}"),
                Poll::Pending => panic!("first send must be ready"),
            }

            let repoll = catch_unwind(AssertUnwindSafe(|| {
                let _ = Future::poll(send_future.as_mut(), &mut context);
            }));
            assert!(
                repoll.is_err(),
                "completed send future must fail fast on post-Ready poll"
            );
            drop(send_future);

            let second = 22u32;
            futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<42, u32>>()
                    .expect("second send flow")
                    .send(&second),
            )
            .expect("second send succeeds");

            let first_recv = futures::executor::block_on(target_endpoint.recv::<Msg<42, u32>>())
                .expect("first recv remains available");
            let second_recv = futures::executor::block_on(target_endpoint.recv::<Msg<42, u32>>())
                .expect("second recv remains available");
            assert_eq!(first_recv, 11);
            assert_eq!(
                second_recv, 22,
                "completed send future repoll must not consume the next descriptor"
            );
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn flow_error_captures_public_callsite() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(11);
            let mut origin_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&origin_program)
                .enter(NoBinding)
                .expect("origin endpoint");
            let target_endpoint = cluster
                .rendezvous(rv_id)
                .session(sid)
                .role(&target_program)
                .enter(NoBinding)
                .expect("target endpoint");
            core::hint::black_box(&target_endpoint);

            let flow_line = line!() + 1;
            let err = match origin_endpoint.flow::<Msg<2, u32>>() {
                Ok(_) => panic!("flow with wrong logical label must fail"),
                Err(err) => err,
            };
            assert_eq!(err.operation(), "flow");
            assert!(err.file().ends_with("tests/cursor_send_recv/send_recv.rs"));
            assert_eq!(err.line(), flow_line);
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("LabelMismatch"),
                "failed send preview must report the preview mismatch: {rendered}"
            );
            assert!(
                !rendered.contains("SessionFault"),
                "failed send preview must not poison before send consumes progress: {rendered}"
            );

            let offer_line = line!() + 1;
            let err = match futures::executor::block_on(origin_endpoint.offer()) {
                Ok(_) => panic!("offer at deterministic send step must fail"),
                Err(err) => err,
            };
            assert_eq!(err.operation(), "offer");
            assert!(err.file().ends_with("tests/cursor_send_recv/send_recv.rs"));
            assert_eq!(err.line(), offer_line);
            assert_progress_invariant_fault(&err);
            assert!(transport_queue_is_empty(&transport));
        });
    });
}
