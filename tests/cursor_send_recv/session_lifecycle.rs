use super::*;

#[test]
fn sequential_noncontiguous_lane_steps_progress_in_order() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
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
                .rendezvous(
                    Config::from_resources((tap_buf, slab), CounterClock::zero()),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(31);
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
                    .flow::<Msg<31, u32>>()
                    .expect("lane 0 first flow")
                    .send(&31),
            )
            .expect("lane 0 first send");
            futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<32, u32>>()
                    .expect("lane 1 middle flow")
                    .send(&32),
            )
            .expect("lane 1 middle send");
            futures::executor::block_on(
                origin_endpoint
                    .flow::<Msg<33, u32>>()
                    .expect("lane 0 final flow")
                    .send(&33),
            )
            .expect("lane 0 final send");

            assert_eq!(
                futures::executor::block_on(target_endpoint.recv::<Msg<31, u32>>())
                    .expect("lane 0 first recv"),
                31
            );
            assert_eq!(
                futures::executor::block_on(target_endpoint.recv::<Msg<32, u32>>())
                    .expect("lane 1 middle recv"),
                32
            );
            assert_eq!(
                futures::executor::block_on(target_endpoint.recv::<Msg<33, u32>>())
                    .expect("lane 0 final recv"),
                33
            );
            assert!(transport_queue_is_empty(&transport));
        });
    });
}

#[test]
fn forgotten_flow_leaves_endpoint_fail_closed() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, Msg<51, u32>>(),
                g::send::<0, 1, Msg<52, u32>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources((tap_buf, slab), CounterClock::zero()),
                    transport,
                )
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

#[test]
fn forgotten_send_future_leaves_endpoint_fail_closed() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<0, 1, Msg<53, u32>>(),
                g::send::<0, 1, Msg<54, u32>>(),
            );
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources((tap_buf, slab), CounterClock::zero()),
                    transport,
                )
                .expect("register rendezvous");

            let sid = SessionId::new(253);
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

            let payload = 53u32;
            let future = origin_endpoint
                .flow::<Msg<53, u32>>()
                .expect("first flow preview")
                .send(&payload);
            core::mem::forget(future);

            match origin_endpoint.flow::<Msg<53, u32>>() {
                Ok(_) => panic!("forgotten send future must reject even the same send preview"),
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

#[test]
fn forgotten_recv_future_leaves_endpoint_fail_closed() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<55, u32>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources((tap_buf, slab), CounterClock::zero()),
                    transport,
                )
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

unsafe fn clone_count_waker(data: *const ()) -> RawWaker {
    RawWaker::new(data, &COUNT_WAKER_VTABLE)
}

unsafe fn wake_count_waker(data: *const ()) {
    let count = unsafe { &*data.cast::<Cell<usize>>() };
    count.set(count.get() + 1);
}

unsafe fn drop_count_waker(data: *const ()) {
    core::hint::black_box(data);
}

static COUNT_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_count_waker,
    wake_count_waker,
    wake_count_waker,
    drop_count_waker,
);

fn counting_waker(count: &Cell<usize>) -> Waker {
    let data = core::ptr::from_ref(count).cast::<()>();
    unsafe { Waker::from_raw(RawWaker::new(data, &COUNT_WAKER_VTABLE)) }
}

#[test]
fn send_session_fault_cancels_pending_transport_state_once() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        let transport = PendingCancelTransport::new();
        let cancel_count = transport.cancel_count();
        with_resident_tls_ref(&PENDING_CANCEL_SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<2, FramePayload>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources((tap_buf, slab), CounterClock::zero()),
                    transport.clone(),
                )
                .expect("register rendezvous");

            let sid = SessionId::new(203);
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

            let payload = FramePayload(*b"hiba");
            {
                let flow = origin_endpoint
                    .flow::<Msg<2, FramePayload>>()
                    .expect("send flow");
                let send_error_line = line!() + 1;
                let mut send_future = std::pin::pin!(flow.send(&payload));
                let waker = futures::task::noop_waker_ref();
                let mut context = Context::from_waker(waker);
                if let Poll::Ready(result) = send_future.as_mut().poll(&mut context) {
                    match result {
                        Ok(()) => panic!("send unexpectedly progressed"),
                        Err(error) => {
                            panic!("send failed before peer dropped: {error:?}");
                        }
                    }
                }
                assert_eq!(
                    cancel_count.get(),
                    0,
                    "initial pending send must not cancel before a terminal fault"
                );

                drop(target_endpoint);

                match send_future.as_mut().poll(&mut context) {
                    Poll::Ready(Err(error)) => {
                        assert_eq!(error.operation(), "send");
                        assert!(
                            error
                                .file()
                                .ends_with("tests/cursor_send_recv/session_lifecycle.rs")
                        );
                        assert_eq!(error.line(), send_error_line);
                        assert!(
                            format!("{error:?}").contains("EndpointDropped"),
                            "send error must keep session fault evidence: {error:?}"
                        );
                    }
                    Poll::Ready(Ok(())) => panic!("send unexpectedly progressed after peer drop"),
                    Poll::Pending => panic!("poisoned send remained pending"),
                }
                assert_eq!(
                    cancel_count.get(),
                    1,
                    "session fault send failure must cancel the pending transport send exactly once"
                );
            }
            assert_eq!(
                cancel_count.get(),
                1,
                "completed send future drop must not cancel the same pending send twice"
            );
            assert!(
                transport.queue_is_empty(),
                "cancelled pending send must not leave a frame available for later flush"
            );
        });
    });
}

#[test]
fn dropping_live_endpoint_poison_wakes_waiting_peer() {
    with_runtime_workspace(|_clock, tap_buf, slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<2, FramePayload>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(
                    Config::from_resources((tap_buf, slab), CounterClock::zero()),
                    transport,
                )
                .expect("register rendezvous");

            let sid = SessionId::new(202);
            let origin_endpoint = rv
                .session(sid)
                .role(&origin_program)
                .enter()
                .expect("origin endpoint");
            let mut target_endpoint = rv
                .session(sid)
                .role(&target_program)
                .enter()
                .expect("target endpoint");

            let mut recv_future = std::pin::pin!(target_endpoint.recv::<Msg<2, FramePayload>>());
            let wake_count = Cell::new(0);
            let waker = counting_waker(&wake_count);
            let mut context = Context::from_waker(&waker);
            if let Poll::Ready(result) = recv_future.as_mut().poll(&mut context) {
                match result {
                    Ok(payload) => {
                        core::hint::black_box(&payload);
                        panic!("recv unexpectedly progressed before sender drop");
                    }
                    Err(error) => {
                        panic!("recv failed before sender drop: {error:?}");
                    }
                }
            }
            assert_eq!(
                wake_count.get(),
                0,
                "initial pending recv must only register its waiter"
            );

            drop(origin_endpoint);

            assert!(
                wake_count.get() > 0,
                "live endpoint drop must wake peers waiting in the same session"
            );
            match recv_future.as_mut().poll(&mut context) {
                Poll::Ready(Err(error)) => {
                    assert_eq!(error.operation(), "recv");
                    assert!(
                        format!("{error:?}").contains("EndpointDropped"),
                        "waiting peer must observe EndpointDropped evidence: {error:?}"
                    );
                }
                Poll::Ready(Ok(payload)) => {
                    core::hint::black_box(&payload);
                    panic!("recv unexpectedly progressed after sender drop");
                }
                Poll::Pending => panic!("poisoned waiting peer remained pending"),
            }
        });
    });
}
