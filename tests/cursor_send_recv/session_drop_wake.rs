use super::*;

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
fn dropping_live_endpoint_poison_wakes_waiting_peer() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<2, FramePayload>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");

            let sid = SessionId::new(202);
            let origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

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
                    assert!(format!("{error:?}").contains("operation: \"recv\""));
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

#[test]
fn overflow_session_waiter_survives_assoc_entry_compaction() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<2, FramePayload>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(slab, transport)
                .expect("register rendezvous");

            let sid_a = SessionId::new(301);
            let sid_b = SessionId::new(302);
            let origin_a = rv.enter(sid_a, &origin_program).expect("origin A");
            let target_a = rv.enter(sid_a, &target_program).expect("target A");
            let origin_b = rv.enter(sid_b, &origin_program).expect("origin B");
            let mut target_b = rv.enter(sid_b, &target_program).expect("target B");

            let mut recv_future = std::pin::pin!(target_b.recv::<Msg<2, FramePayload>>());
            let wake_count = Cell::new(0);
            let waker = counting_waker(&wake_count);
            let mut context = Context::from_waker(&waker);
            assert!(
                recv_future.as_mut().poll(&mut context).is_pending(),
                "session B recv must park before peer drop"
            );
            assert_eq!(wake_count.get(), 0);

            drop(origin_a);
            drop(target_a);
            assert_eq!(
                wake_count.get(),
                0,
                "compacting an unrelated session must not wake session B"
            );

            drop(origin_b);
            assert!(
                wake_count.get() > 0,
                "session B waiter must move from overflow storage into the inline slot"
            );
            match recv_future.as_mut().poll(&mut context) {
                Poll::Ready(Err(error)) => {
                    assert!(
                        format!("{error:?}").contains("EndpointDropped"),
                        "compacted waiter must observe the peer drop fault: {error:?}"
                    );
                }
                Poll::Ready(Ok(payload)) => {
                    core::hint::black_box(&payload);
                    panic!("recv unexpectedly progressed after peer drop");
                }
                Poll::Pending => panic!("compacted session waiter remained pending"),
            }
        });
    });
}
