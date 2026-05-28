use super::*;

#[test]
fn sequential_noncontiguous_lane_steps_progress_in_order() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::seq(
                g::send::<Role<0>, Role<1>, Msg<31, u32>, 0>(),
                g::seq(
                    g::send::<Role<0>, Role<1>, Msg<32, u32>, 1>(),
                    g::send::<Role<0>, Role<1>, Msg<33, u32>, 0>(),
                ),
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

            let sid = SessionId::new(31);
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
    with_fixture(|_clock, tap_buf, slab| {
        let transport = PendingCancelTransport::default();
        let cancel_count = transport.cancel_count();
        with_resident_tls_ref(&PENDING_CANCEL_SESSION_SLOT, |cluster| {
            let program = g::send::<Role<0>, Role<1>, Msg<2, FramePayload>, 0>();
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

            let sid = SessionId::new(203);
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

            let payload = FramePayload(*b"hiba");
            let mut send_future = std::pin::pin!(
                origin_endpoint
                    .flow::<Msg<2, FramePayload>>()
                    .expect("send flow")
                    .send(&payload)
            );
            let waker = futures::task::noop_waker_ref();
            let mut context = Context::from_waker(waker);
            match send_future.as_mut().poll(&mut context) {
                Poll::Pending => {}
                Poll::Ready(Ok(())) => panic!("send unexpectedly progressed"),
                Poll::Ready(Err(error)) => {
                    panic!("send failed before peer dropped: {error:?}");
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
            drop(send_future);
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
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<Role<0>, Role<1>, Msg<2, FramePayload>, 0>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv_id = cluster
                .add_rendezvous_from_config(
                    Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources(
                        (tap_buf, slab),
                        CounterClock::new(),
                    ),
                    transport,
                )
                .expect("register rendezvous");

            let sid = SessionId::new(202);
            let origin_endpoint = cluster
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

            let mut recv_future = std::pin::pin!(target_endpoint.recv::<Msg<2, FramePayload>>());
            let wake_count = Cell::new(0);
            let waker = counting_waker(&wake_count);
            let mut context = Context::from_waker(&waker);
            match recv_future.as_mut().poll(&mut context) {
                Poll::Pending => {}
                Poll::Ready(Ok(payload)) => {
                    core::hint::black_box(&payload);
                    panic!("recv unexpectedly progressed before sender drop");
                }
                Poll::Ready(Err(error)) => {
                    panic!("recv failed before sender drop: {error:?}");
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
