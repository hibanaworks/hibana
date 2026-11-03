use super::*;

struct WakeCount(std::sync::atomic::AtomicUsize);
impl std::task::Wake for WakeCount {
    fn wake(self: std::sync::Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &std::sync::Arc<Self>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn dropping_live_endpoint_poison_wakes_waiting_peer() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::send::<0, 1, Msg<2, FramePayload>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");

            let sid = SessionId::new(202);
            let origin_endpoint = rv.enter(sid, &origin_program).expect("origin endpoint");
            let mut target_endpoint = rv.enter(sid, &target_program).expect("target endpoint");

            let mut recv_future = std::pin::pin!(target_endpoint.recv::<Msg<2, FramePayload>>());
            let wake_count = std::sync::Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
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
                wake_count.0.load(std::sync::atomic::Ordering::Relaxed),
                0,
                "initial pending recv must only register its waiter"
            );

            drop(origin_endpoint);

            assert!(
                wake_count.0.load(std::sync::atomic::Ordering::Relaxed) > 0,
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
        };
    };
}

#[test]
fn endpoint_waiter_isolated_from_unrelated_session_compaction() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::send::<0, 1, Msg<2, FramePayload>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport)
                .expect("register rendezvous");

            let sid_a = SessionId::new(301);
            let sid_b = SessionId::new(302);
            let origin_a = rv.enter(sid_a, &origin_program).expect("origin A");
            let target_a = rv.enter(sid_a, &target_program).expect("target A");
            let origin_b = rv.enter(sid_b, &origin_program).expect("origin B");
            let mut target_b = rv.enter(sid_b, &target_program).expect("target B");

            let mut recv_future = std::pin::pin!(target_b.recv::<Msg<2, FramePayload>>());
            let wake_count = std::sync::Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            let mut context = Context::from_waker(&waker);
            assert!(
                recv_future.as_mut().poll(&mut context).is_pending(),
                "session B recv must park before peer drop"
            );
            assert_eq!(wake_count.0.load(std::sync::atomic::Ordering::Relaxed), 0);

            drop(origin_a);
            drop(target_a);
            assert_eq!(
                wake_count.0.load(std::sync::atomic::Ordering::Relaxed),
                0,
                "unrelated session retirement must not wake session B"
            );

            drop(origin_b);
            assert!(
                wake_count.0.load(std::sync::atomic::Ordering::Relaxed) > 0,
                "session B endpoint waiter must observe its peer drop"
            );
            match recv_future.as_mut().poll(&mut context) {
                Poll::Ready(Err(error)) => {
                    assert!(
                        format!("{error:?}").contains("EndpointDropped"),
                        "endpoint waiter must observe the peer drop fault: {error:?}"
                    );
                }
                Poll::Ready(Ok(payload)) => {
                    core::hint::black_box(&payload);
                    panic!("recv unexpectedly progressed after peer drop");
                }
                Poll::Pending => panic!("session B endpoint waiter remained pending"),
            }
        };
    };
}
