use super::*;

#[test]
fn send_session_fault_cancels_pending_transport_state_once() {
    with_runtime_workspace(|slab| {
        let transport = PendingCancelTransport::new();
        let cancel_count = transport.cancel_count();
        with_resident_tls_ref(&PENDING_CANCEL_SESSION_SLOT, |cluster| {
            let program = g::send::<0, 1, Msg<2, FramePayload>>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(slab, transport.clone())
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
                let mut send_future =
                    std::pin::pin!(origin_endpoint.send::<Msg<2, FramePayload>>(&payload));
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
                        assert!(format!("{error:?}").contains("operation: \"send\""));
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
