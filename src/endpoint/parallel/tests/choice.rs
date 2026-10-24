use super::*;
use core::{future::Future, task::Poll};

#[test]
fn a_local_preview_keeps_its_event_scope_inside_a_selected_parallel_arm() {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};

    type Left = g::Msg<192, ()>;
    type Local = g::Msg<193, ()>;
    type Notice = g::Msg<194, u32>;
    let global = g::route(
        g::send::<0, 1, Left>(),
        g::par(g::send::<0, 0, Local>(), g::send::<0, 1, Notice>()),
    )
    .resolve::<191>();
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let network = TestTransport::new();
    let mut sender_slab = [0; 32 * 1024];
    let mut receiver_slab = [0; 32 * 1024];
    let mut sender_storage = SessionKitStorage::uninit();
    let mut receiver_storage = SessionKitStorage::uninit();
    let sender_runtime = sender_storage
        .init()
        .rendezvous(&mut sender_slab, network.clone())
        .unwrap();
    let receiver_runtime = receiver_storage
        .init()
        .rendezvous(&mut receiver_slab, network.clone())
        .unwrap();
    sender_runtime
        .set_resolver(
            &sender_program,
            ResolverRef::<191>::decision_state(&(), |_| Ok(DecisionArm::Right)),
        )
        .unwrap();
    let session = SessionId::new(1698);
    let mut sender = sender_runtime.enter(session, &sender_program).unwrap();
    let mut receiver = receiver_runtime.enter(session, &receiver_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                tokio::join!(
                    async {
                        let branch = sender.offer().await.unwrap();
                        assert_eq!(branch.label(), 193);
                        drop(branch);
                        let [mut local, mut outgoing] = sender.par().unwrap();
                        let (sent, local) =
                            tokio::join!(outgoing.send::<Notice>(&73), local.recv::<Local>());
                        local.unwrap();
                        sent.unwrap();
                    },
                    async {
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(branch.label(), 194);
                        drop(branch);
                        let [mut incoming] = receiver.par().unwrap();
                        assert_eq!(incoming.recv::<Notice>().await.unwrap(), 73);
                    },
                );
            })
            .await
            .expect("the selected parallel arm stalled");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_pending_route_send_does_not_block_an_independent_receive() {
    type Left = g::Msg<181, u32>;
    type Right = g::Msg<182, u32>;
    type Notice = g::Msg<183, u32>;
    let global = g::par(
        g::route(g::send::<0, 1, Left>(), g::send::<0, 1, Right>()),
        g::send::<1, 0, Notice>(),
    );
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let mut slab = [0; 32 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1680);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                let [mut choosing, mut notices] = sender.par().unwrap();
                let [mut receiving, mut notifying] = receiver.par().unwrap();
                network.pause_send(0);
                let sending = choosing.send::<Right>(&7);
                tokio::pin!(sending);
                core::future::poll_fn(|cx| {
                    assert!(sending.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
                notifying.send::<Notice>(&8).await.unwrap();
                assert_eq!(notices.recv::<Notice>().await.unwrap(), 8);
                network.resume_send();
                let (sent, received) = tokio::join!(sending.as_mut(), async {
                    let branch = receiving.offer().await.unwrap();
                    assert_eq!(branch.label(), 182);
                    branch.recv::<Right>().await.unwrap()
                });
                sent.unwrap();
                assert_eq!(received, 7);
            })
            .await
            .expect("a pending route send cannot serialize its independent sibling");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_resolver_preview_allows_parallel_sends_before_the_first_send_commits() {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};
    use core::cell::Cell;

    type A = g::Msg<184, u32>;
    type B = g::Msg<185, u32>;
    type C = g::Msg<186, u32>;
    type D = g::Msg<187, u32>;
    let global = g::route(
        g::par(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::par(g::send::<0, 1, C>(), g::send::<0, 1, D>()),
    )
    .resolve::<190>();
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let decisions = Cell::new(0);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    rv.set_resolver(
        &sender_program,
        ResolverRef::<190>::decision_state(&decisions, |calls: &Cell<u32>| {
            calls.set(calls.get() + 1);
            Ok(DecisionArm::Right)
        }),
    )
    .unwrap();
    let sid = SessionId::new(1681);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                let branch = sender.offer().await.unwrap();
                assert_eq!(branch.label(), 186);
                drop(branch);
                assert!(sender.par::<3>().is_err());
                let [mut first, mut second] = sender
                    .par()
                    .expect("the existing resolver preview must permit independent borrows");
                network.pause_send(0);
                let first_send = first.send::<C>(&41);
                let second_send = second.send::<D>(&42);
                tokio::pin!(first_send, second_send);
                core::future::poll_fn(|cx| {
                    assert!(first_send.as_mut().poll(cx).is_pending());
                    assert!(second_send.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
                network.resume_send();
                // The second arm reaches the wire while the first arm remains
                // pending and has not been polled again since transport resume.
                second_send.as_mut().await.unwrap();
                let branch = receiver.offer().await.unwrap();
                assert_eq!(branch.label(), 187);
                assert_eq!(branch.recv::<D>().await.unwrap(), 42);
                let (sent, received) = tokio::join!(first_send.as_mut(), receiver.recv::<C>());
                sent.unwrap();
                assert_eq!(received.unwrap(), 41);
                assert_eq!(decisions.get(), 1);
            })
            .await
            .expect("independent route arms must not wait for the first send's commit");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_received_preview_moves_to_its_own_parallel_borrow() {
    type A = g::Msg<188, u32>;
    type B = g::Msg<189, u32>;
    type C = g::Msg<190, u32>;
    type D = g::Msg<191, u32>;
    let global = g::route(
        g::par(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::par(g::send::<0, 1, C>(), g::send::<0, 1, D>()),
    );
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1682);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                sender.send::<D>(&42).await.unwrap();
                let branch = receiver.offer().await.unwrap();
                assert_eq!(branch.label(), 191);
                drop(branch);
                assert!(receiver.par::<3>().is_err());
                let [mut receive_c, mut receive_d] = receiver.par().unwrap();
                let [mut send_c, sent_d] = sender.par().unwrap();
                drop(sent_d);
                let pending_c = receive_c.recv::<C>();
                tokio::pin!(pending_c);
                core::future::poll_fn(|cx| {
                    assert!(pending_c.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
                assert_eq!(receive_d.recv::<D>().await.unwrap(), 42);
                let (sent, received) = tokio::join!(send_c.send::<C>(&41), pending_c.as_mut());
                sent.unwrap();
                assert_eq!(received.unwrap(), 41);
            })
            .await
            .expect("a parked frame must follow its own independent fragment");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_moved_preview_payload_survives_an_independent_fault() {
    type A = g::Msg<192, u32>;
    type B = g::Msg<193, &'static [u8]>;
    type C = g::Msg<194, u32>;
    type WrongC = g::Msg<194, u64>;
    type D = g::Msg<195, &'static [u8]>;
    let global = g::route(
        g::par(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::par(g::send::<0, 1, C>(), g::send::<0, 1, D>()),
    );
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    let sid = SessionId::new(1683);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            sender
                .send::<D>(&b"borrowed frame in second fragment".as_slice())
                .await
                .unwrap();
            let branch = receiver.offer().await.unwrap();
            assert_eq!(branch.label(), 195);
            drop(branch);
            let [mut receive_c, mut receive_d] = receiver.par().unwrap();
            let [mut send_c, sent_d] = sender.par().unwrap();
            drop(sent_d);
            let data = receive_d.recv::<D>().await.unwrap();
            send_c.send::<C>(&41).await.unwrap();
            assert!(receive_c.recv::<WrongC>().await.is_err());
            assert_eq!(data, b"borrowed frame in second fragment");
        });
}

#[test]
fn repeated_parallel_previews_use_the_current_resolver_choice() {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};
    use core::cell::Cell;

    type A = g::Msg<196, u32>;
    type B = g::Msg<197, u32>;
    type C = g::Msg<198, u32>;
    type D = g::Msg<199, u32>;
    let global = g::route(
        g::par(g::send::<0, 1, A>(), g::send::<0, 1, B>()),
        g::par(g::send::<0, 1, C>(), g::send::<0, 1, D>()),
    )
    .resolve::<191>()
    .roll();
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let desired = Cell::new(DecisionArm::Right);
    let calls = Cell::new(0);
    let policy_input = (&desired, &calls);
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let network = TestTransport::new();
    let rv = storage
        .init()
        .rendezvous(&mut slab, network.clone())
        .unwrap();
    rv.set_resolver(
        &sender_program,
        ResolverRef::<191>::decision_state(
            &policy_input,
            |input: &(&Cell<DecisionArm>, &Cell<u32>)| {
                input.1.set(input.1.get() + 1);
                Ok(input.0.get())
            },
        ),
    )
    .unwrap();
    let sid = SessionId::new(1684);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(10), async {
                for (decision, number) in [
                    (DecisionArm::Right, 1),
                    (DecisionArm::Left, 2),
                    (DecisionArm::Right, 3),
                ] {
                    desired.set(decision);
                    let label = sender.offer().await.unwrap().label();
                    match label {
                        196 => {
                            assert_eq!(decision, DecisionArm::Left);
                            let [mut first, mut second] = sender.par().unwrap();
                            second.send::<B>(&number).await.unwrap();
                            let observed = receiver.offer().await.unwrap().label();
                            assert_eq!(observed, 197);
                            let [mut receive_first, mut receive_second] = receiver.par().unwrap();
                            assert_eq!(receive_second.recv::<B>().await.unwrap(), number);
                            let (sent, received) =
                                tokio::join!(first.send::<A>(&number), receive_first.recv::<A>());
                            sent.unwrap();
                            assert_eq!(received.unwrap(), number);
                        }
                        198 => {
                            assert_eq!(decision, DecisionArm::Right);
                            let [mut first, mut second] = sender.par().unwrap();
                            second.send::<D>(&number).await.unwrap();
                            let observed = receiver.offer().await.unwrap().label();
                            assert_eq!(observed, 199);
                            let [mut receive_first, mut receive_second] = receiver.par().unwrap();
                            assert_eq!(receive_second.recv::<D>().await.unwrap(), number);
                            let (sent, received) =
                                tokio::join!(first.send::<C>(&number), receive_first.recv::<C>());
                            sent.unwrap();
                            assert_eq!(received.unwrap(), number);
                        }
                        _ => panic!("unexpected resolver preview label {label}"),
                    }
                }
                assert_eq!(calls.get(), 3);
            })
            .await
            .expect("parallel borrows must use this visit's preview, not the previous choice");
        });
    assert!(network.queue_is_empty());
}

#[test]
fn a_resource_view_can_preview_a_message_from_its_current_owner() {
    use crate::runtime::resolver::{DecisionArm, ResolverRef};

    type Loan<'r> = g::Msg<201, crate::Endpoint<'r, 3>>;
    type Notice = g::Msg<202, ()>;
    type Ping = g::Msg<203, ()>;
    let global = g::route(
        g::send::<0, 1, Loan<'_>>(),
        g::par(g::send::<0, 3, Notice>(), g::send::<0, 1, Ping>()),
    )
    .resolve::<201>();
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let resource_program = project::<3>(&global);
    let network = TestTransport::new();
    let mut owner_slab = [0; 32 * 1024];
    let mut worker_slab = [0; 32 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let owner_runtime = owner_storage
        .init()
        .rendezvous(&mut owner_slab, network.clone())
        .unwrap();
    let worker_runtime = worker_storage
        .init()
        .rendezvous(&mut worker_slab, network.clone())
        .unwrap();
    owner_runtime
        .set_resolver(
            &owner_program,
            ResolverRef::<201>::decision_state(&(), |_| Ok(DecisionArm::Right)),
        )
        .unwrap();
    let session = SessionId::new(1699);
    let mut owner = owner_runtime.enter(session, &owner_program).unwrap();
    let mut worker = worker_runtime.enter(session, &worker_program).unwrap();
    let mut resource = owner_runtime.enter(session, &resource_program).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(core::time::Duration::from_secs(1), async {
                owner.send::<Notice>(&()).await.unwrap();
                let branch = resource.offer().await.unwrap();
                assert_eq!(branch.label(), 202);
                branch.recv::<Notice>().await.unwrap();
                owner.send::<Ping>(&()).await.unwrap();
                let branch = worker.offer().await.unwrap();
                assert_eq!(branch.label(), 203);
                branch.recv::<Ping>().await.unwrap();
            })
            .await
            .expect("the resource view's selected receive stalled");
        });
    assert!(network.queue_is_empty());
}
