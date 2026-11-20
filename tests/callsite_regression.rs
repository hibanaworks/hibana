mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_early_frame_from_another_arm_survives_its_undelivered_prefix()
-> Result<(), hibana::EndpointError> {
    use core::{
        future::{Future, poll_fn},
        pin::pin,
        task::Poll,
    };
    type Loan<'r> = Msg<0, hibana::Endpoint<'r, 3>>;
    type Read = Msg<1, ()>;
    type Value = Msg<2, ()>;
    type Returned<'r> = Msg<3, hibana::Endpoint<'r, 3>>;
    type Left = Msg<4, ()>;
    type Prefix = Msg<5, ()>;
    type Early = Msg<6, ()>;
    type Right = Msg<7, ()>;
    type Other = Msg<8, ()>;
    type Ack = Msg<9, ()>;
    type Done = Msg<10, ()>;
    let source = g::par(
        g::seq(
            g::send::<0, 1, Loan>(),
            g::seq(
                g::send::<3, 2, Read>(),
                g::seq(g::send::<2, 3, Value>(), g::send::<1, 0, Returned>()),
            ),
        ),
        g::route(
            g::seq(
                g::send::<0, 1, Left>(),
                g::seq(g::send::<0, 2, Prefix>(), g::send::<1, 2, Early>()),
            ),
            g::seq(
                g::send::<0, 1, Right>(),
                g::seq(
                    g::send::<1, 2, Other>(),
                    g::seq(g::send::<2, 1, Ack>(), g::send::<1, 0, Done>()),
                ),
            ),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    let transport = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let session = SessionId::new(1957);
    let mut owner = rv.enter(session, &p0).unwrap();
    let db = rv.enter(session, &p3).unwrap();
    let mut worker = rv.enter(session, &p1).unwrap();
    let mut service = rv.enter(session, &p2).unwrap();
    owner.send::<Loan>(db).await?;
    owner.send::<Left>(&()).await?;
    worker.recv::<Left>().await?;
    worker.send::<Early>(&()).await?;
    let queued = transport.queued_frame(2);
    {
        let mut receive = pin!(service.recv::<Other>());
        let result = poll_fn(|cx| Poll::Ready(receive.as_mut().poll(cx))).await;
        assert!(
            matches!(result, Poll::Ready(Err(error)) if error.is_unavailable()),
            "a legal early left-arm frame cannot fault an excluded receive: {result:?}"
        );
    }
    assert_eq!(transport.queued_frame(2), queued);
    assert!(service.recv::<Early>().await.unwrap_err().is_unavailable());
    assert_eq!(transport.queued_frame(2), queued);
    owner.send::<Prefix>(&()).await?;
    service.recv::<Prefix>().await?;
    service.recv::<Early>().await?;
    let mut db = worker.recv::<Loan>().await?;
    db.send::<Read>(&()).await?;
    service.recv::<Read>().await?;
    service.send::<Value>(&()).await?;
    db.recv::<Value>().await?;
    worker.send::<Returned>(db).await?;
    drop(owner.recv::<Returned>().await?);
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_early_file_reply_cannot_wait_past_its_own_repair_notice()
-> Result<(), hibana::EndpointError> {
    use core::{
        future::{Future, poll_fn},
        pin::pin,
        task::Poll,
    };
    use hibana::Endpoint;
    type Loan<'r> = Msg<200, Endpoint<'r, 3>>;
    type Request = Msg<201, u32>;
    type Reply = Msg<202, u32>;
    type Retry = Msg<203, u32>;
    type Notice = Msg<204, u32>;
    type Stop = Msg<205, u32>;
    type Returned<'r> = Msg<206, Endpoint<'r, 3>>;
    let exchange = g::seq(g::send::<3, 0, Request>(), g::send::<0, 3, Reply>());
    let repair = g::seq(
        g::send::<1, 0, Retry>(),
        g::seq(g::send::<0, 3, Notice>(), exchange),
    );
    let source = g::seq(
        g::send::<2, 0, Loan>(),
        g::seq(
            exchange,
            g::seq(
                g::route(repair, g::send::<1, 0, Stop>()),
                g::send::<0, 2, Returned>(),
            ),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    for cut in 0..=3 {
        let transport = TestTransport::new();
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, transport.clone())
            .unwrap();
        let session = SessionId::new(1952);
        let mut issuer = rv.enter(session, &p2).unwrap();
        let mut holder = rv.enter(session, &p0).unwrap();
        let mut controller = rv.enter(session, &p1).unwrap();
        let file = rv.enter(session, &p3).unwrap();
        issuer.send::<Loan>(file).await?;
        let mut file = holder.recv::<Loan>().await?;
        file.send::<Request>(&11).await?;
        assert_eq!(holder.recv::<Request>().await?, 11);
        holder.send::<Reply>(&13).await?;
        assert_eq!(file.recv::<Reply>().await?, 13);
        if cut >= 1 {
            controller.send::<Retry>(&17).await?;
        }
        if cut >= 2 {
            assert_eq!(holder.recv::<Retry>().await?, 17);
        }
        if cut >= 3 {
            holder.send::<Notice>(&19).await?;
        }
        let queued = (cut == 3).then(|| transport.queued_frame(3));
        let rejected = {
            let mut call = pin!(file.recv::<Reply>());
            poll_fn(|cx| Poll::Ready(call.as_mut().poll(cx))).await
        };
        assert!(
            matches!(rejected, Poll::Ready(Err(ref error)) if error.is_unavailable()),
            "cut {cut}: an incomplete file notice/request cannot be awaited through the same file view: {rejected:?}",
        );
        if let Some(queued) = queued {
            assert_eq!(transport.queued_frame(3), queued);
        }
        if cut < 1 {
            controller.send::<Retry>(&17).await?;
        }
        if cut < 2 {
            assert_eq!(holder.recv::<Retry>().await?, 17);
        }
        if cut < 3 {
            holder.send::<Notice>(&19).await?;
        }
        assert_eq!(file.recv::<Notice>().await?, 19);
        file.send::<Request>(&23).await?;
        assert_eq!(holder.recv::<Request>().await?, 23);
        holder.send::<Reply>(&29).await?;
        assert_eq!(file.recv::<Reply>().await?, 29);
        holder.send::<Returned>(file).await?;
        drop(issuer.recv::<Returned>().await?);
        assert!(transport.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_pending_file_reply_rechecks_a_choice_learned_by_its_holder()
-> Result<(), hibana::EndpointError> {
    use core::{
        future::{Future, poll_fn},
        pin::pin,
        task::Poll,
    };
    use hibana::Endpoint;
    type Loan<'r> = Msg<210, Endpoint<'r, 3>>;
    type Request = Msg<211, u32>;
    type Reply = Msg<212, u32>;
    type Retry = Msg<213, u32>;
    type Notice = Msg<214, u32>;
    type Stop = Msg<215, u32>;
    type Returned<'r> = Msg<216, Endpoint<'r, 3>>;
    type Begin = Msg<217, u32>;
    let exchange = g::seq(g::send::<3, 0, Request>(), g::send::<0, 3, Reply>());
    let source = g::seq(
        g::send::<2, 0, Loan>(),
        g::seq(
            exchange,
            g::seq(
                g::seq(
                    g::send::<0, 1, Begin>(),
                    g::route(
                        g::seq(
                            g::send::<1, 0, Retry>(),
                            g::seq(g::send::<0, 3, Notice>(), exchange),
                        ),
                        g::seq(g::send::<1, 0, Stop>(), g::send::<0, 3, Reply>()),
                    ),
                )
                .roll(),
                g::send::<0, 2, Returned>(),
            ),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    for cancelled in [false, true] {
        let transport = TestTransport::new();
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, transport.clone())
            .unwrap();
        let session = SessionId::new(1953);
        let mut issuer = rv.enter(session, &p2).unwrap();
        let mut holder = rv.enter(session, &p0).unwrap();
        let mut controller = rv.enter(session, &p1).unwrap();
        let file = rv.enter(session, &p3).unwrap();
        issuer.send::<Loan>(file).await?;
        let mut file = holder.recv::<Loan>().await?;
        file.send::<Request>(&11).await?;
        assert_eq!(holder.recv::<Request>().await?, 11);
        holder.send::<Reply>(&13).await?;
        assert_eq!(file.recv::<Reply>().await?, 13);
        holder.send::<Begin>(&15).await?;
        assert_eq!(controller.recv::<Begin>().await?, 15);
        {
            let mut receive = pin!(file.recv::<Reply>());
            let first = poll_fn(|cx| Poll::Ready(receive.as_mut().poll(cx))).await;
            assert!(
                matches!(first, Poll::Pending),
                "cancelled {cancelled}: {first:?}"
            );
            if !cancelled {
                controller.send::<Retry>(&17).await?;
                assert_eq!(holder.recv::<Retry>().await?, 17);
                holder.send::<Notice>(&19).await?;
                let next = poll_fn(|cx| Poll::Ready(receive.as_mut().poll(cx))).await;
                assert!(
                    matches!(next, Poll::Ready(Err(ref error)) if error.is_unavailable()),
                    "pending receive must reject without consuming the selected notice: {next:?}"
                );
            }
        }
        if cancelled {
            controller.send::<Retry>(&17).await?;
            assert_eq!(holder.recv::<Retry>().await?, 17);
            holder.send::<Notice>(&19).await?;
            let error = file.recv::<Reply>().await.unwrap_err();
            assert!(
                error.is_unavailable(),
                "cancelled receive must preserve the selected notice: {error:?}"
            );
        }
        assert_eq!(file.recv::<Notice>().await?, 19);
        file.send::<Request>(&23).await?;
        assert_eq!(holder.recv::<Request>().await?, 23);
        holder.send::<Reply>(&29).await?;
        assert_eq!(file.recv::<Reply>().await?, 29);
        holder.send::<Returned>(file).await?;
        drop(issuer.recv::<Returned>().await?);
        assert!(transport.queue_is_empty());
    }
    Ok(())
}

use common::TestTransport;
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError, ResolverRef},
    },
};

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_rolled_receive_is_unavailable_after_its_resource_permanently_leaves()
-> Result<(), hibana::EndpointError> {
    use core::{
        future::{Future, poll_fn},
        pin::pin,
        task::Poll,
    };
    use hibana::Endpoint;
    type Resource<'r> = Msg<230, Endpoint<'r, 2>>;
    type File<'r> = Msg<231, Endpoint<'r, 3>>;
    type Rejected = Msg<232, ()>;
    type Feedback = Msg<233, ()>;
    type Ready = Msg<234, ()>;
    type Request = Msg<235, ()>;
    type ResultMessage = Msg<236, ()>;
    type Approved = Msg<237, ()>;
    type Published = Msg<238, ()>;
    type Next = Msg<239, ()>;
    let retry = g::seq(
        g::send::<2, 0, Rejected>(),
        g::seq(
            g::send::<0, 3, Feedback>(),
            g::seq(
                g::send::<0, 2, Ready>(),
                g::seq(g::send::<3, 2, Request>(), g::send::<2, 0, ResultMessage>()),
            ),
        ),
    );
    let approved = || {
        g::seq(
            g::send::<2, 4, Approved>(),
            g::seq(
                g::send::<1, 4, Resource>(),
                g::seq(
                    g::send::<2, 0, ResultMessage>(),
                    g::seq(g::send::<0, 3, Published>(), g::send::<4, 5, Next>()),
                ),
            ),
        )
    };
    let source = g::seq(
        g::send::<6, 1, Resource>(),
        g::seq(
            g::send::<6, 5, File>(),
            g::route(approved(), g::seq(retry.roll(), approved())).resolve::<1955>(),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    let p4 = project::<4>(&source);
    let p5 = project::<5>(&source);
    let p6 = project::<6>(&source);
    let transport = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    rv.set_resolver(
        &p2,
        ResolverRef::<1955>::decision_state(&(), |_| Ok(DecisionArm::Right)),
    )
    .unwrap();
    let session = SessionId::new(1955);
    let mut repository = rv.enter(session, &p0).unwrap();
    let mut initial = rv.enter(session, &p1).unwrap();
    let mut issuer = rv.enter(session, &p6).unwrap();
    let resource = rv.enter(session, &p2).unwrap();
    let file = rv.enter(session, &p3).unwrap();
    let mut finalizer = rv.enter(session, &p4).unwrap();
    let mut agent = rv.enter(session, &p5).unwrap();
    issuer.send::<Resource>(resource).await?;
    let mut resource = initial.recv::<Resource>().await?;
    issuer.send::<File>(file).await?;
    let mut file = agent.recv::<File>().await?;
    resource.send::<Rejected>(&()).await?;
    repository.recv::<Rejected>().await?;
    repository.send::<Feedback>(&()).await?;
    file.recv::<Feedback>().await?;
    repository.send::<Ready>(&()).await?;
    resource.recv::<Ready>().await?;
    file.send::<Request>(&()).await?;
    resource.recv::<Request>().await?;
    resource.send::<ResultMessage>(&()).await?;
    repository.recv::<ResultMessage>().await?;
    {
        let mut receiving = pin!(file.recv::<Feedback>());
        let outcome = poll_fn(|cx| Poll::Ready(receiving.as_mut().poll(cx))).await;
        assert!(matches!(outcome, Poll::Pending), "{outcome:?}");
    }
    resource.send::<Approved>(&()).await?;
    initial.send::<Resource>(resource).await?;
    finalizer.recv::<Approved>().await?;
    let mut resource = finalizer.recv::<Resource>().await?;
    resource.send::<ResultMessage>(&()).await?;
    repository.recv::<ResultMessage>().await?;
    repository.send::<Published>(&()).await?;
    let queued = transport.queued_frame(3);
    {
        let mut receiving = pin!(file.recv::<Feedback>());
        let outcome = poll_fn(|cx| Poll::Ready(receiving.as_mut().poll(cx))).await;
        assert!(
            matches!(outcome, Poll::Ready(Err(error)) if error.is_unavailable()),
            "an arrived causal witness must exclude the obsolete receive: {outcome:?}"
        );
    }
    assert_eq!(transport.queued_frame(3), queued);
    {
        let branch = file.offer().await?;
        assert_eq!(branch.label(), 238);
    }
    file.recv::<Published>().await?;
    {
        let mut receiving = pin!(file.recv::<Feedback>());
        let outcome = poll_fn(|cx| Poll::Ready(receiving.as_mut().poll(cx))).await;
        assert!(
            matches!(outcome, Poll::Ready(Err(error)) if error.is_unavailable()),
            "the old review cannot await a sender whose resource permanently left: {outcome:?}"
        );
    }
    {
        let mut peers = ();
        let mut borrowed = file.borrow(&mut peers);
        let mut receiving = pin!(borrowed.recv::<Feedback>());
        let outcome = poll_fn(|cx| Poll::Ready(receiving.as_mut().poll(cx))).await;
        assert!(
            matches!(outcome, Poll::Ready(Err(error)) if error.is_unavailable()),
            "the borrowed file must reject that same obsolete receive: {outcome:?}"
        );
    }
    assert!(transport.queue_is_empty());
    finalizer.send::<Next>(&()).await?;
    agent.recv::<Next>().await?;
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn retiring_a_sender_does_not_erase_an_already_accepted_roll_message()
-> Result<(), hibana::EndpointError> {
    type Loan<'r> = Msg<240, hibana::Endpoint<'r, 2>>;
    type File<'r> = Msg<241, hibana::Endpoint<'r, 3>>;
    type Ask = Msg<242, ()>;
    type Feedback = Msg<243, u32>;
    type Confirmed = Msg<244, ()>;
    type Published = Msg<245, ()>;
    let source = g::seq(
        g::send::<6, 1, Loan>(),
        g::seq(
            g::send::<6, 7, File>(),
            g::seq(
                g::seq(g::send::<2, 0, Ask>(), g::send::<0, 3, Feedback>()).roll(),
                g::seq(
                    g::send::<1, 4, Loan>(),
                    g::seq(g::send::<2, 5, Confirmed>(), g::send::<5, 3, Published>()),
                ),
            ),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    let p4 = project::<4>(&source);
    let p5 = project::<5>(&source);
    let p6 = project::<6>(&source);
    let p7 = project::<7>(&source);
    let transport = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let session = SessionId::new(1956);
    let mut repository = rv.enter(session, &p0).unwrap();
    let mut initial = rv.enter(session, &p1).unwrap();
    let mut issuer = rv.enter(session, &p6).unwrap();
    let resource = rv.enter(session, &p2).unwrap();
    let file = rv.enter(session, &p3).unwrap();
    let mut finalizer = rv.enter(session, &p4).unwrap();
    let mut observer = rv.enter(session, &p5).unwrap();
    let mut agent = rv.enter(session, &p7).unwrap();
    issuer.send::<Loan>(resource).await?;
    let mut resource = initial.recv::<Loan>().await?;
    issuer.send::<File>(file).await?;
    let mut file = agent.recv::<File>().await?;
    resource.send::<Ask>(&()).await?;
    repository.recv::<Ask>().await?;
    repository.send::<Feedback>(&17).await?;
    assert_eq!(file.recv::<Feedback>().await?, 17);
    resource.send::<Ask>(&()).await?;
    repository.recv::<Ask>().await?;
    repository.send::<Feedback>(&19).await?;
    initial.send::<Loan>(resource).await?;
    let mut resource = finalizer.recv::<Loan>().await?;
    resource.send::<Confirmed>(&()).await?;
    observer.recv::<Confirmed>().await?;
    observer.send::<Published>(&()).await?;
    file.recv::<Published>().await?;
    assert_eq!(file.recv::<Feedback>().await?, 19);
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_empty_receive_can_be_refused_after_another_view_learns_the_choice()
-> Result<(), hibana::EndpointError> {
    use core::{
        future::{Future, poll_fn},
        pin::pin,
        task::Poll,
    };
    type Loan<'r> = Msg<220, hibana::Endpoint<'r, 3>>;
    type Retry = Msg<221, u32>;
    type Notice = Msg<222, u32>;
    type Stop = Msg<223, u32>;
    type Reply = Msg<224, u32>;
    type Returned<'r> = Msg<225, hibana::Endpoint<'r, 3>>;
    let source = g::seq(
        g::send::<2, 0, Loan>(),
        g::seq(
            g::route(
                g::seq(g::send::<1, 0, Retry>(), g::send::<1, 3, Notice>()),
                g::par(g::send::<1, 0, Stop>(), g::send::<1, 3, Reply>()),
            ),
            g::send::<0, 2, Returned>(),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    let transport = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(1954);
    let mut issuer = rv.enter(sid, &p2).unwrap();
    let mut holder = rv.enter(sid, &p0).unwrap();
    let mut controller = rv.enter(sid, &p1).unwrap();
    let file = rv.enter(sid, &p3).unwrap();
    issuer.send::<Loan>(file).await?;
    let mut file = holder.recv::<Loan>().await?;
    {
        let mut receive = pin!(file.recv::<Reply>());
        let pending = poll_fn(|cx| Poll::Ready(receive.as_mut().poll(cx))).await;
        assert!(matches!(pending, Poll::Pending), "{pending:?}");
        controller.send::<Retry>(&17).await?;
        assert_eq!(holder.recv::<Retry>().await?, 17);
        let refused = poll_fn(|cx| Poll::Ready(receive.as_mut().poll(cx))).await;
        assert!(
            matches!(refused, Poll::Ready(Err(error)) if error.is_unavailable()),
            "an empty pending receive must not fault the run when another view learns its exclusion: {refused:?}"
        );
    }
    controller.send::<Notice>(&19).await?;
    assert_eq!(file.recv::<Notice>().await?, 19);
    holder.send::<Returned>(file).await?;
    drop(issuer.recv::<Returned>().await?);
    assert!(transport.queue_is_empty());
    Ok(())
}

fn assert_endpoint_debug_boundary(error: hibana::EndpointError, operation: &str) {
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(&format!("operation: \"{operation}\"")),
        "EndpointError Debug must include operation {operation}: {error:?}"
    );
    assert_compact_debug(&rendered);
}

fn assert_attach_debug_boundary(error: hibana::runtime::AttachError, operation: &str) {
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(&format!("operation: \"{operation}\"")),
        "AttachError Debug must include operation {operation}: {error:?}"
    );
    assert_compact_debug(&rendered);
}

fn assert_resolver_debug_boundary(error: ResolverError, operation: &str) {
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains(&format!("operation: \"{operation}\"")),
        "ResolverError Debug must include operation {operation}: {error:?}"
    );
    assert_compact_debug(&rendered);
}

fn assert_compact_debug(rendered: &str) {
    for forbidden in ["file", "line", "column"] {
        assert!(
            !rendered.contains(forbidden),
            "public error Debug must not carry source location {forbidden}: {rendered}"
        );
    }
}

static UNIT_RESOLVER_STATE: () = ();
const SCHEMA_ROUTE_RESOLVER: u16 = 4120;

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn co_resident_file_requests_select_their_own_implicit_arm()
-> Result<(), hibana::EndpointError> {
    type FirstFile<'r> = Msg<1, hibana::Endpoint<'r, 9>>;
    type SecondFile<'r> = Msg<2, hibana::Endpoint<'r, 10>>;
    type First = Msg<3, u32>;
    type Second = Msg<4, u32>;
    type NoticeFirst = Msg<5, ()>;
    type NoticeSecond = Msg<6, ()>;
    let source = g::seq(
        g::par(g::send::<0, 2, FirstFile>(), g::send::<1, 2, SecondFile>()),
        g::route(
            g::seq(g::send::<9, 0, First>(), g::send::<2, 1, NoticeFirst>()),
            g::seq(g::send::<10, 1, Second>(), g::send::<2, 0, NoticeSecond>()),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p9 = project::<9>(&source);
    let p10 = project::<10>(&source);
    for right in [false, true] {
        let transport = TestTransport::new();
        let mut slab = [0; 128 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, transport.clone())
            .unwrap();
        let session = SessionId::new(8199);
        let mut first = rv.enter(session, &p0).unwrap();
        let first_file = rv.enter(session, &p9).unwrap();
        let mut second = rv.enter(session, &p1).unwrap();
        let second_file = rv.enter(session, &p10).unwrap();
        let mut service = rv.enter(session, &p2).unwrap();
        async {
            async { first.send::<FirstFile>(first_file).await };
            async { second.send::<SecondFile>(second_file).await };
            Ok(())
        }
        .await?;
        let (mut first_file, mut second_file) = async {
            let [mut a, mut b] = service.par()?;
            async { a.recv::<FirstFile>().await };
            async { b.recv::<SecondFile>().await };
        }
        .await?;
        async {
            async {
                if right {
                    second_file.send::<Second>(&73).await?;
                    service.send::<NoticeSecond>(&()).await?;
                    assert!(
                        first_file
                            .send::<First>(&99)
                            .await
                            .unwrap_err()
                            .is_unavailable()
                    );
                } else {
                    first_file.send::<First>(&73).await?;
                    service.send::<NoticeFirst>(&()).await?;
                    assert!(
                        second_file
                            .send::<Second>(&99)
                            .await
                            .unwrap_err()
                            .is_unavailable()
                    );
                }
                Ok(())
            };
            async {
                if right {
                    first.recv::<NoticeSecond>().await?;
                } else {
                    assert_eq!(first.recv::<First>().await?, 73);
                }
                Ok(())
            };
            async {
                if right {
                    assert_eq!(second.recv::<Second>().await?, 73);
                } else {
                    second.recv::<NoticeFirst>().await?;
                }
                Ok(())
            };
            Ok(())
        }
        .await?;
        assert!(transport.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn another_legal_choice_frame_survives_a_nonmatching_receive()
-> Result<(), hibana::EndpointError> {
    type Retry = Msg<181, u32>;
    type Finished = Msg<182, u32>;
    const CHOOSE: u16 = 8182;
    let source =
        g::route(g::send::<0, 1, Retry>().roll(), g::send::<0, 1, Finished>()).resolve::<CHOOSE>();
    let sender_program = project::<0>(&source);
    let receiver_program = project::<1>(&source);
    for arrival in 0..3 {
        let queued = arrival != 0;
        let transport = TestTransport::new();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, transport.clone())
            .unwrap();
        rv.set_resolver(
            &sender_program,
            ResolverRef::<CHOOSE>::decision_state(&(), |_| Ok(DecisionArm::Right)),
        )
        .unwrap();
        let session = SessionId::new(8182 + u32::from(queued));
        let mut sender = rv.enter(session, &sender_program).unwrap();
        let mut receiver = rv.enter(session, &receiver_program).unwrap();
        if queued {
            sender.send::<Finished>(&73).await?;
        }
        if arrival == 2 {
            let preview = receiver.offer().await?;
            assert_eq!(preview.label(), 182);
            drop(preview);
        }
        async {
            async {
                let rejected = receiver.recv::<Retry>().await.unwrap_err();
                assert!(rejected.is_unavailable(), "queued={queued}: {rejected:?}");
                assert_eq!(receiver.recv::<Finished>().await?, 73);
            };
            async {
                if !queued {
                    in_process::sleep(core::time::Duration::from_millis(1)).await;
                    sender.send::<Finished>(&73).await?;
                }
                Ok::<(), hibana::EndpointError>(())
            };
            Ok::<(), hibana::EndpointError>(())
        }
        .await?;
        assert!(transport.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_excluded_receive_does_not_wait_for_another_alternative_lane()
-> Result<(), hibana::EndpointError> {
    type Retry = Msg<181, u32>;
    type Finished = Msg<182, u32>;
    type FromFirst = Msg<183, u32>;
    type FromSecond = Msg<184, u32>;
    type Done = Msg<185, u32>;
    type RetryReceived = Msg<180, ()>;
    const CHOOSE: u16 = 8183;
    const RETRY_FROM: u16 = 8184;
    let source = g::route(
        g::route(
            g::seq(g::send::<0, 2, FromFirst>(), g::send::<0, 1, Retry>()),
            g::seq(g::send::<0, 2, FromSecond>(), g::send::<2, 1, Retry>()),
        )
        .resolve::<RETRY_FROM>()
        .roll(),
        g::seq(g::send::<0, 2, Done>(), g::send::<0, 1, Finished>()),
    )
    .resolve::<CHOOSE>();
    // Both Retry inputs were previously unexecuted here. Without a receiver
    // acknowledgement their first inputs cannot distinguish repeated visits.
    let diagnostic = g::diagnose(&source).expect("an unfenced retry is not projectable");
    assert!(matches!(
        diagnostic.problem,
        g::ProjectionProblem::ReceiveLaneCausality
    ));
    let source = g::route(
        g::route(
            g::seq(
                g::seq(g::send::<0, 2, FromFirst>(), g::send::<0, 1, Retry>()),
                g::send::<1, 0, RetryReceived>(),
            ),
            g::seq(
                g::seq(g::send::<0, 2, FromSecond>(), g::send::<2, 1, Retry>()),
                g::send::<1, 0, RetryReceived>(),
            ),
        )
        .resolve::<RETRY_FROM>()
        .roll(),
        g::seq(g::send::<0, 2, Done>(), g::send::<0, 1, Finished>()),
    )
    .resolve::<CHOOSE>();
    let sender_program = project::<0>(&source);
    let receiver_program = project::<1>(&source);
    let other_program = project::<2>(&source);
    let transport = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    rv.set_resolver(
        &sender_program,
        ResolverRef::<CHOOSE>::decision_state(&(), |_| Ok(DecisionArm::Right)),
    )
    .unwrap();
    let session = SessionId::new(8183);
    let mut sender = rv.enter(session, &sender_program).unwrap();
    let mut receiver = rv.enter(session, &receiver_program).unwrap();
    let mut other = rv.enter(session, &other_program).unwrap();
    sender.send::<Done>(&73).await?;
    other.recv::<Done>().await?;
    sender.send::<Finished>(&73).await?;
    let rejected = hibana_runtime::race(receiver.recv::<Retry>(), async {
        in_process::sleep(core::time::Duration::from_millis(20)).await;
        panic!("a different choice's pending lane hid the arrived alternative");
    })
    .await
    .unwrap_err();
    assert!(rejected.is_unavailable(), "{rejected:?}");
    assert_eq!(receiver.recv::<Finished>().await?, 73);
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_independent_arrived_frame_does_not_reject_a_pending_receive()
-> Result<(), hibana::EndpointError> {
    type First = Msg<186, u32>;
    type Second = Msg<187, u32>;
    let source = g::par(g::send::<0, 1, First>(), g::send::<2, 1, Second>());
    let first_program = project::<0>(&source);
    let receiver_program = project::<1>(&source);
    let second_program = project::<2>(&source);
    let transport = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let session = SessionId::new(8185);
    let mut first = rv.enter(session, &first_program).unwrap();
    let mut receiver = rv.enter(session, &receiver_program).unwrap();
    let mut second = rv.enter(session, &second_program).unwrap();
    second.send::<Second>(&19).await?;
    async {
        async {
            assert_eq!(receiver.recv::<First>().await?, 73);
            assert_eq!(receiver.recv::<Second>().await?, 19);
        };
        async {
            in_process::sleep(core::time::Duration::from_millis(1)).await;
            first.send::<First>(&73).await?;
        };
        Ok::<(), hibana::EndpointError>(())
    }
    .await?;
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn offer_receives_the_input_prefix_before_calling_its_result_resolver()
-> Result<(), hibana::EndpointError> {
    use std::cell::Cell;
    type Request = Msg<61, u32>;
    type Accepted = Msg<62, u32>;
    type Rejected = Msg<63, u32>;
    const DECIDE: u16 = 4121;
    let request = g::send::<0, 1, Request>();
    let accepted = g::send::<1, 0, Accepted>();
    let retry = g::seq(g::send::<1, 0, Rejected>(), request);
    let global = g::seq(
        request,
        g::route(accepted, g::seq(retry.roll(), accepted)).resolve::<DECIDE>(),
    );
    let client_program = project::<0>(&global);
    let server_program = project::<1>(&global);
    let input = (Cell::new(None::<u32>), Cell::new(0usize));
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    rv.set_resolver(
        &server_program,
        ResolverRef::<DECIDE>::decision_state(&input, |input| {
            input.1.set(input.1.get() + 1);
            match input.0.get() {
                Some(0) => Ok(DecisionArm::Right),
                Some(_) => Ok(DecisionArm::Left),
                None => Err(ResolverError::reject()),
            }
        }),
    )
    .unwrap();
    let mut client = rv.enter(SessionId::new(4121), &client_program).unwrap();
    let mut server = rv.enter(SessionId::new(4121), &server_program).unwrap();
    client.send::<Request>(&7).await?;
    let preview = server.offer().await?;
    assert_eq!(preview.label(), 61);
    assert_eq!(
        input.1.get(),
        0,
        "the resolver's input receive has not committed"
    );
    let value = preview.recv::<Request>().await?;
    input.0.set(Some(value));
    server.send::<Accepted>(&value).await?;
    assert_eq!(client.recv::<Accepted>().await?, 7);
    assert!(input.1.get() > 0);
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn parallel_offer_receives_its_input_before_resolving_the_reply()
-> Result<(), hibana::EndpointError> {
    use std::cell::Cell;
    type IoRequest = Msg<64, u32>;
    type IoReply = Msg<65, u32>;
    type Request = Msg<66, u32>;
    type Accepted = Msg<67, u32>;
    type Rejected = Msg<68, u32>;
    type Exit = Msg<69, u32>;
    const DECIDE: u16 = 4122;
    let request = g::send::<0, 1, Request>();
    let accepted = g::send::<1, 0, Accepted>();
    let retry = g::seq(g::send::<1, 0, Rejected>(), request);
    let operation = g::seq(
        request,
        g::route(accepted, g::seq(retry.roll(), accepted)).resolve::<DECIDE>(),
    );
    let source = g::seq(
        g::par(
            g::seq(g::send::<0, 1, IoRequest>(), g::send::<1, 0, IoReply>()).roll(),
            operation,
        ),
        g::send::<0, 1, Exit>(),
    );
    let client_program = project::<0>(&source);
    let server_program = project::<1>(&source);
    let input = (Cell::new(None::<u32>), Cell::new(0usize));
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    rv.set_resolver(
        &server_program,
        ResolverRef::<DECIDE>::decision_state(&input, |input| {
            input.1.set(input.1.get() + 1);
            match input.0.get() {
                Some(0) => Ok(DecisionArm::Right),
                Some(_) => Ok(DecisionArm::Left),
                None => Err(ResolverError::reject()),
            }
        }),
    )
    .unwrap();
    let mut client = rv.enter(SessionId::new(4122), &client_program).unwrap();
    let mut server = rv.enter(SessionId::new(4122), &server_program).unwrap();
    client.send::<IoRequest>(&3).await?;
    {
        let [mut io, mut operation] = server.par()?;
        let preview = hibana_runtime::race(io.offer(), operation.offer()).await?;
        assert_eq!(preview.label(), 64);
        assert_eq!(preview.recv::<IoRequest>().await?, 3);
        io.send::<IoReply>(&3).await?;
        assert_eq!(client.recv::<IoReply>().await?, 3);
        client.send::<Request>(&7).await?;
        let preview = hibana_runtime::race(io.offer(), operation.offer()).await?;
        assert_eq!(preview.label(), 66);
        assert_eq!(
            input.1.get(),
            0,
            "a future reply cannot resolve before its input receive"
        );
        let value = preview.recv::<Request>().await?;
        input.0.set(Some(value));
        drop(operation.offer().await?);
        let value = input.0.take().unwrap();
        operation.send::<Accepted>(&value).await?;
        assert_eq!(client.recv::<Accepted>().await?, value);
    }
    client.send::<Exit>(&0).await?;
    assert_eq!(server.recv::<Exit>().await?, 0);
    assert!(input.1.get() > 0);
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_unavailable_borrowed_send_preserves_queued_input_and_later_rolls()
-> Result<(), hibana::EndpointError> {
    let source = g::seq(
        g::send::<0, 1, Msg<1, u32>>(),
        g::seq(
            g::send::<0, 1, Msg<3, u32>>().roll(),
            g::send::<1, 0, Msg<2, u32>>(),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let transport = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut store = SessionKitStorage::uninit();
    let rv = store
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(1946);
    let mut origin = rv.enter(sid, &p0).unwrap();
    let mut target = rv.enter(sid, &p1).unwrap();
    origin.send::<Msg<1, u32>>(&17).await?;
    let queued = transport.queued_frame(1);
    let rejected = origin.send::<Msg<1, u32>>(&19).await.unwrap_err();
    assert!(rejected.is_unavailable(), "{rejected:?}");
    assert_eq!(transport.queued_frame(1), queued);
    assert_eq!(target.recv::<Msg<1, u32>>().await?, 17);
    for value in [23, 29] {
        origin.send::<Msg<3, u32>>(&value).await?;
        assert_eq!(target.recv::<Msg<3, u32>>().await?, value);
    }
    target.send::<Msg<2, u32>>(&31).await?;
    assert_eq!(origin.recv::<Msg<2, u32>>().await?, 31);
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_unavailable_owned_send_faults_without_restoring_moved_authority()
-> Result<(), hibana::EndpointError> {
    use hibana::Endpoint;
    type Loan<'r> = Msg<1, Endpoint<'r, 2>>;
    type Returned<'r> = Msg<2, Endpoint<'r, 2>>;
    let source = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(g::send::<1, 0, Returned>(), g::send::<0, 1, Msg<3, u32>>()),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let transport = TestTransport::new();
    let [mut owner_slab, mut holder_slab] = [[0; 64 * 1024]; 2];
    let mut owner_store = SessionKitStorage::uninit();
    let mut holder_store = SessionKitStorage::uninit();
    let owner = owner_store
        .init()
        .rendezvous(&mut owner_slab, transport.clone())
        .unwrap();
    let holder = holder_store
        .init()
        .rendezvous(&mut holder_slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(1947);
    let mut issuer = owner.enter(sid, &p0).unwrap();
    let resource = owner.enter(sid, &p2).unwrap();
    let mut worker = holder.enter(sid, &p1).unwrap();
    issuer.send::<Loan>(resource).await?;
    let resource = worker.recv::<Loan>().await?;
    worker.send::<Returned>(resource).await?;
    let resource = issuer.recv::<Returned>().await?;
    let rejected = issuer.send::<Loan>(resource).await.unwrap_err();
    assert!(!rejected.is_unavailable(), "{rejected:?}");
    assert!(owner.enter(sid, &p2).is_err());
    let fault = issuer.send::<Msg<3, u32>>(&17).await.unwrap_err();
    assert!(format!("{fault:?}").contains("SessionFault"), "{fault:?}");
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_unavailable_contract_in_another_fragment_is_still_terminal()
-> Result<(), hibana::EndpointError> {
    let source = g::par(
        g::send::<0, 1, Msg<1, u32>>(),
        g::send::<0, 1, Msg<2, u32>>(),
    );
    let p0 = project::<0>(&source);
    let transport = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut store = SessionKitStorage::uninit();
    let rv = store
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let mut origin = rv.enter(SessionId::new(1948), &p0).unwrap();
    let [mut first, mut second] = origin.par()?;
    first.send::<Msg<1, u32>>(&17).await?;
    let queued = transport.queued_frame(1);
    let rejected = second.send::<Msg<1, u32>>(&19).await.unwrap_err();
    assert!(!rejected.is_unavailable(), "{rejected:?}");
    let fault = second.send::<Msg<2, u32>>(&23).await.unwrap_err();
    assert!(format!("{fault:?}").contains("SessionFault"), "{fault:?}");
    assert_eq!(transport.queued_frame(1), queued);
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_unavailable_receive_does_not_fault_the_next_permitted_call()
-> Result<(), hibana::EndpointError> {
    let source = g::seq(
        g::send::<0, 1, Msg<1, u32>>(),
        g::seq(
            g::send::<0, 1, Msg<3, u32>>(),
            g::send::<1, 0, Msg<2, u32>>(),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let transport = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut store = SessionKitStorage::uninit();
    let rv = store
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(1943);
    let mut origin = rv.enter(sid, &p0).unwrap();
    let mut target = rv.enter(sid, &p1).unwrap();
    origin.send::<Msg<1, u32>>(&17).await?;
    origin.send::<Msg<3, u32>>(&23).await?;
    assert_eq!(target.recv::<Msg<1, u32>>().await?, 17);
    let queued = transport.queued_frame(1);
    let rejected = target.recv::<Msg<1, u32>>().await.unwrap_err();
    assert!(rejected.is_unavailable(), "{rejected:?}");
    assert_eq!(transport.queued_frame(1), queued);
    assert_eq!(target.recv::<Msg<3, u32>>().await?, 23);
    target.send::<Msg<2, u32>>(&29).await?;
    assert_eq!(origin.recv::<Msg<2, u32>>().await?, 29);
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_unavailable_receive_preserves_a_different_parked_preview()
-> Result<(), hibana::EndpointError> {
    let source = g::seq(
        g::send::<0, 1, Msg<1, u32>>(),
        g::seq(
            g::route(
                g::send::<0, 1, Msg<2, u32>>(),
                g::send::<0, 1, Msg<4, u32>>(),
            ),
            g::send::<1, 0, Msg<3, u32>>(),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let transport = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut store = SessionKitStorage::uninit();
    let rv = store
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(1945);
    let mut origin = rv.enter(sid, &p0).unwrap();
    let mut target = rv.enter(sid, &p1).unwrap();
    origin.send::<Msg<1, u32>>(&17).await?;
    target.recv::<Msg<1, u32>>().await?;
    origin.send::<Msg<2, u32>>(&23).await?;
    let branch = target.offer().await?;
    assert_eq!(branch.label(), 2);
    drop(branch);
    let rejected = target.recv::<Msg<1, u32>>().await.unwrap_err();
    assert!(rejected.is_unavailable(), "{rejected:?}");
    assert_eq!(target.recv::<Msg<2, u32>>().await?, 23);
    target.send::<Msg<3, u32>>(&29).await?;
    assert_eq!(origin.recv::<Msg<3, u32>>().await?, 29);
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_unavailable_owned_receive_keeps_the_actual_returned_authority_unique()
-> Result<(), hibana::EndpointError> {
    use hibana::Endpoint;
    type Loan<'r> = Msg<1, Endpoint<'r, 2>>;
    type Returned<'r> = Msg<4, Endpoint<'r, 2>>;
    let source = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::send::<2, 3, Msg<2, u32>>(),
            g::seq(
                g::send::<3, 2, Msg<3, ()>>(),
                g::seq(g::send::<1, 0, Returned>(), g::send::<1, 0, Msg<5, u32>>()),
            ),
        ),
    );
    let p0 = project::<0>(&source);
    let p1 = project::<1>(&source);
    let p2 = project::<2>(&source);
    let p3 = project::<3>(&source);
    let transport = TestTransport::new();
    let [mut owner_slab, mut holder_slab] = [[0; 64 * 1024]; 2];
    let mut owner_store = SessionKitStorage::uninit();
    let mut holder_store = SessionKitStorage::uninit();
    let owner = owner_store
        .init()
        .rendezvous(&mut owner_slab, transport.clone())
        .unwrap();
    let holder = holder_store
        .init()
        .rendezvous(&mut holder_slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(1944);
    let mut issuer = owner.enter(sid, &p0).unwrap();
    let resource = owner.enter(sid, &p2).unwrap();
    let mut service = owner.enter(sid, &p3).unwrap();
    let mut worker = holder.enter(sid, &p1).unwrap();
    issuer.send::<Loan>(resource).await?;
    let mut resource = worker.recv::<Loan>().await?;
    resource.send::<Msg<2, u32>>(&7).await?;
    assert_eq!(service.recv::<Msg<2, u32>>().await?, 7);
    service.send::<Msg<3, ()>>(&()).await?;
    resource.recv::<Msg<3, ()>>().await?;
    worker.send::<Returned>(resource).await?;
    let _resource = issuer.recv::<Returned>().await?;
    let rejected = match worker.recv::<Loan>().await {
        Err(error) => error,
        Ok(_) => panic!("completed loan was issued again"),
    };
    assert!(rejected.is_unavailable(), "{rejected:?}");
    assert!(transport.queue_is_empty());
    assert!(holder.enter(sid, &p2).is_err());
    worker.send::<Msg<5, u32>>(&11).await?;
    assert_eq!(issuer.recv::<Msg<5, u32>>().await?, 11);
    assert!(transport.queue_is_empty());
    Ok(())
}

fn reject_from_unit(_: &()) -> Result<DecisionArm, ResolverError> {
    Err(ResolverError::reject())
}

fn select_right_from_unit(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Right)
}

fn project_pair<const MSG_ID: u8>() -> (RoleProgram<0>, RoleProgram<1>) {
    let program = g::send::<0, 1, Msg<MSG_ID, u32>>();
    (project(&program), project(&program))
}

#[test]
fn runtime_registers_multiple_rendezvous_from_resident_resources() {
    {
        let mut slab = [0; 6 * 16 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (role0, _) = project_pair::<7>();
            let transport = TestTransport::new();
            const CHUNK: usize = 16 * 1024;
            let (chunk0, rest) = slab.split_at_mut(CHUNK);
            let (chunk1, rest) = rest.split_at_mut(CHUNK);
            let (chunk2, rest) = rest.split_at_mut(CHUNK);
            let (chunk3, rest) = rest.split_at_mut(CHUNK);
            let (chunk4, rest) = rest.split_at_mut(CHUNK);
            let (chunk5, _) = rest.split_at_mut(CHUNK);
            for (idx, chunk) in [chunk0, chunk1, chunk2, chunk3, chunk4, chunk5]
                .into_iter()
                .enumerate()
            {
                let rv = cluster
                    .rendezvous(chunk, transport.clone())
                    .expect("runtime registry must grow from each resident rendezvous resource");
                let endpoint = rv
                    .enter(SessionId::new(700 + idx as u32), &role0)
                    .expect("wide rendezvous budget must still attach endpoints");
                core::hint::black_box(&endpoint);
                drop(endpoint);
            }
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn endpoint_errors_keep_debug_boundaries() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (origin_program, target_program) = project_pair::<11>();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("rv");
            let sid = SessionId::new(411);
            let mut origin = rv.enter(sid, &origin_program).expect("origin");
            let target = rv.enter(sid, &target_program).expect("target");

            let send_error = (origin.send::<Msg<12, u32>>(&1234))
                .await
                .expect_err("wrong send label must fail");
            assert_endpoint_debug_boundary(send_error, "send");
            let poisoned = (origin.send::<Msg<11, u32>>(&1234))
                .await
                .expect_err("wrong send label must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));

            drop(origin);
            drop(target);
            let sid = SessionId::new(412);
            let mut origin = rv.enter(sid, &origin_program).expect("fresh origin");
            let mut target = rv.enter(sid, &target_program).expect("fresh target");
            (origin.send::<Msg<11, u32>>(&1234)).await.expect("send");

            let recv_result = (target.recv::<Msg<11, i32>>()).await;
            let recv_error = match recv_result {
                Ok(_) => panic!("wrong recv payload must fail"),
                Err(error) => error,
            };
            assert_endpoint_debug_boundary(recv_error, "recv");
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn payload_schema_mismatch_cannot_publish_send_or_consume_recv() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (origin_program, target_program) = project_pair::<13>();
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("rendezvous");
            let sid = SessionId::new(412);
            let mut origin = rv.enter(sid, &origin_program).expect("origin");
            let target = rv.enter(sid, &target_program).expect("target");

            let send_error = (origin.send::<Msg<13, i32>>(&1234_i32))
                .await
                .expect_err("same-width wrong send schema must fail before publication");
            assert!(format!("{send_error:?}").contains("PayloadMismatch"));
            assert!(transport.queue_is_empty());

            let poisoned = (origin.send::<Msg<13, u32>>(&1234_u32))
                .await
                .expect_err("wrong send schema must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));
            assert!(transport.queue_is_empty());

            drop(origin);
            drop(target);
            let sid = SessionId::new(414);
            let mut origin = rv.enter(sid, &origin_program).expect("fresh origin");
            let mut target = rv.enter(sid, &target_program).expect("fresh target");
            (origin.send::<Msg<13, u32>>(&1234_u32))
                .await
                .expect("fresh session must publish the declared payload");
            assert!(!transport.queue_is_empty());

            let recv_error = (target.recv::<Msg<13, i32>>())
                .await
                .expect_err("same-width wrong recv schema must fail before frame consumption");
            assert!(format!("{recv_error:?}").contains("PayloadMismatch"));
            assert!(
                !transport.queue_is_empty(),
                "schema rejection must leave the queued frame unconsumed"
            );
            let poisoned = (target.recv::<Msg<13, u32>>())
                .await
                .expect_err("wrong recv schema must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn declared_receive_is_recoverable_while_another_schema_with_its_label_is_enabled() {
    type First = Msg<13, u32>;
    type Later = Msg<13, u16>;
    let program = g::seq(g::send::<0, 1, First>(), g::send::<0, 1, Later>());
    let origin_program: RoleProgram<0> = project(&program);
    let target_program: RoleProgram<1> = project(&program);
    let mut slab = [0; 64 * 1024];
    let transport = TestTransport::new();
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(415);
    let mut origin = rv.enter(sid, &origin_program).unwrap();
    let mut target = rv.enter(sid, &target_program).unwrap();

    origin.send::<First>(&100).await.unwrap();
    let error = target.recv::<Later>().await.unwrap_err();
    assert!(error.is_unavailable(), "{error:?}");
    assert!(!transport.queue_is_empty());
    assert_eq!(target.recv::<First>().await.unwrap(), 100);
    origin.send::<Later>(&200).await.unwrap();
    assert_eq!(target.recv::<Later>().await.unwrap(), 200);
    assert!(transport.queue_is_empty());
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn payload_schema_tracks_selected_route_when_labels_match() {
    {
        let mut slab = [0; 64 * 1024];
        let transport = TestTransport::new();
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let program = g::route(
                g::send::<0, 1, Msg<14, u32>>(),
                g::send::<0, 1, Msg<14, i32>>(),
            )
            .resolve::<SCHEMA_ROUTE_RESOLVER>();
            let origin_program: RoleProgram<0> = project(&program);
            let target_program: RoleProgram<1> = project(&program);
            let rv = cluster
                .rendezvous(&mut slab, transport.clone())
                .expect("rendezvous");
            rv.set_resolver(
                &origin_program,
                ResolverRef::<SCHEMA_ROUTE_RESOLVER>::decision_state(
                    &UNIT_RESOLVER_STATE,
                    select_right_from_unit,
                ),
            )
            .expect("install schema route resolver");
            let sid = SessionId::new(413);
            let mut origin = rv.enter(sid, &origin_program).expect("origin");
            let target = rv.enter(sid, &target_program).expect("target");

            let mismatch = (origin.send::<Msg<14, u32>>(&23))
                .await
                .expect_err("selected right arm must reject the left payload schema");
            assert!(format!("{mismatch:?}").contains("PayloadMismatch"));
            assert!(transport.queue_is_empty());

            let poisoned = (origin.send::<Msg<14, i32>>(&-17))
                .await
                .expect_err("wrong selected-arm schema must poison the session");
            assert!(format!("{poisoned:?}").contains("SessionFault(ProtocolViolation)"));

            drop(origin);
            drop(target);
            let sid = SessionId::new(414);
            let mut origin = rv.enter(sid, &origin_program).expect("fresh origin");
            let mut target = rv.enter(sid, &target_program).expect("fresh target");
            (origin.send::<Msg<14, i32>>(&-17))
                .await
                .expect("selected right arm and payload schema must agree");
            let branch = (target.offer()).await.expect("offer right arm");
            assert_eq!(branch.label(), 14);
            assert_eq!(
                (branch.recv::<Msg<14, i32>>())
                    .await
                    .expect("branch receive schema must match the selected arm"),
                -17
            );
            assert!(transport.queue_is_empty());
        };
    };
}

#[test]
fn attach_and_resolver_errors_keep_debug_boundaries() {
    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let rendezvous_result = cluster.rendezvous(&mut slab[..1], TestTransport::new());
            let rendezvous_error = match rendezvous_result {
                Ok(_) => panic!("resource-constrained rendezvous slab must fail"),
                Err(error) => error,
            };
            assert_attach_debug_boundary(rendezvous_error, "rendezvous");
        };
    };

    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (origin_program, _target_program) = project_pair::<42>();
            let rv = cluster
                .rendezvous(&mut slab[..2048], TestTransport::new())
                .expect("rv");
            let sid = SessionId::new(441);
            let endpoint = rv
                .enter(sid, &origin_program)
                .expect("first role lease must attach");
            let enter_result = rv.enter(sid, &origin_program);
            let enter_error = match enter_result {
                Ok(_) => panic!("duplicate live session-role lease must fail"),
                Err(error) => error,
            };
            assert_attach_debug_boundary(enter_error, "enter");
            drop(endpoint);

            let resolver =
                ResolverRef::<77>::decision_state(&UNIT_RESOLVER_STATE, reject_from_unit);
            let set_resolver_result = rv.set_resolver(&origin_program, resolver);
            let set_resolver_error = match set_resolver_result {
                Ok(_) => panic!("resolver without projected site must fail"),
                Err(error) => error,
            };
            assert_resolver_debug_boundary(set_resolver_error, "set_resolver");
        };
    };

    let reject_error = ResolverError::reject();
    assert_resolver_debug_boundary(reject_error, "reject");
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn completed_sender_drop_preserves_queued_peer_progress() {
    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (send_program, recv_program) = project_pair::<21>();
            let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
            let sid = SessionId::new(921);
            let mut sender = rv.enter(sid, &send_program).unwrap();
            let mut receiver = rv.enter(sid, &recv_program).unwrap();
            (sender.send::<Msg<21, u32>>(&7)).await.unwrap();
            drop(sender);
            assert_eq!((receiver.recv::<Msg<21, u32>>()).await.unwrap(), 7);
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn dropping_unexecuted_endpoint_faults_the_session() {
    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (send_program, recv_program) = project_pair::<22>();
            let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
            let sid = SessionId::new(922);
            let sender = rv.enter(sid, &send_program).unwrap();
            let mut receiver = rv.enter(sid, &recv_program).unwrap();
            drop(sender);
            let error = (receiver.recv::<Msg<22, u32>>()).await.unwrap_err();
            assert_endpoint_debug_boundary(error, "recv");
            assert!(format!("{error:?}").contains("SessionFault(EndpointDropped)"));
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn drop_preserves_a_committed_protocol_fault() {
    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let (send_program, recv_program) = project_pair::<23>();
            let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
            let sid = SessionId::new(923);
            let mut sender = rv.enter(sid, &send_program).unwrap();
            let mut receiver = rv.enter(sid, &recv_program).unwrap();
            assert!((sender.send::<Msg<24, u32>>(&7)).await.is_err());
            drop(sender);
            let error = (receiver.recv::<Msg<23, u32>>()).await.unwrap_err();
            assert!(format!("{error:?}").contains("SessionFault(ProtocolViolation)"));
        };
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn completed_parallel_drop_accepts_both_committed_arm_orders() {
    for right_first in [false, true] {
        {
            let mut slab = [0; 64 * 1024];
            {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                let global = g::par(
                    g::send::<0, 1, Msg<30, u32>>(),
                    g::send::<0, 2, Msg<31, u32>>(),
                );
                let sender_program = project::<0>(&global);
                let left_program = project::<1>(&global);
                let right_program = project::<2>(&global);
                let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
                let sid = SessionId::new(930);
                let mut sender = rv.enter(sid, &sender_program).unwrap();
                let mut left = rv.enter(sid, &left_program).unwrap();
                let mut right = rv.enter(sid, &right_program).unwrap();
                (async {
                    if right_first {
                        sender.send::<Msg<31, u32>>(&31).await.unwrap();
                        sender.send::<Msg<30, u32>>(&30).await.unwrap();
                    } else {
                        sender.send::<Msg<30, u32>>(&30).await.unwrap();
                        sender.send::<Msg<31, u32>>(&31).await.unwrap();
                    }
                    // Successful peer progress after Drop proves the completed
                    // sender did not poison either parallel arm.
                    drop(sender);
                    assert_eq!(left.recv::<Msg<30, u32>>().await.unwrap(), 30);
                    assert_eq!(right.recv::<Msg<31, u32>>().await.unwrap(), 31);
                })
                .await;
            };
        };
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn incomplete_parallel_drop_faults_each_remaining_arm() {
    for completed_arm in [None, Some(false), Some(true)] {
        {
            let mut slab = [0; 64 * 1024];
            {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                let global = g::par(
                    g::send::<0, 1, Msg<32, u32>>(),
                    g::send::<0, 2, Msg<33, u32>>(),
                );
                let sender_program = project::<0>(&global);
                let left_program = project::<1>(&global);
                let right_program = project::<2>(&global);
                let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
                let sid = SessionId::new(932);
                let mut sender = rv.enter(sid, &sender_program).unwrap();
                let mut left = rv.enter(sid, &left_program).unwrap();
                let mut right = rv.enter(sid, &right_program).unwrap();
                (async {
                    match completed_arm {
                        Some(true) => {
                            sender.send::<Msg<33, u32>>(&33).await.unwrap();
                            assert_eq!(right.recv::<Msg<33, u32>>().await.unwrap(), 33);
                        }
                        Some(false) => {
                            sender.send::<Msg<32, u32>>(&32).await.unwrap();
                            assert_eq!(left.recv::<Msg<32, u32>>().await.unwrap(), 32);
                        }
                        None => {}
                    }
                    // In particular, the lexical last arm cannot hide its
                    // unexecuted sibling from the existing Drop/fault path.
                    drop(sender);
                    let left_error = left.recv::<Msg<32, u32>>().await.unwrap_err();
                    let right_error = right.recv::<Msg<33, u32>>().await.unwrap_err();
                    for error in [left_error, right_error] {
                        assert!(format!("{error:?}").contains("SessionFault(EndpointDropped)"));
                    }
                })
                .await;
            };
        };
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn parallel_drop_uses_committed_route_and_fresh_roll_visit() {
    for complete_visits in 0..=3 {
        for begin_next_visit in [false, true] {
            {
                let mut slab = [0; 64 * 1024];
                {
                    let mut storage = SessionKitStorage::uninit();
                    let cluster = storage.init();
                    let global = g::par(
                        g::seq(
                            g::send::<0, 1, Msg<36, ()>>(),
                            g::route(
                                g::send::<1, 0, Msg<37, u32>>(),
                                g::send::<1, 0, Msg<38, u32>>(),
                            ),
                        )
                        .roll(),
                        g::send::<0, 2, Msg<39, ()>>(),
                    );
                    let sender_program = project::<0>(&global);
                    let peer_program = project::<1>(&global);
                    let other_program = project::<2>(&global);
                    let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
                    let sid = SessionId::new(936);
                    let mut sender = rv.enter(sid, &sender_program).unwrap();
                    let mut peer = rv.enter(sid, &peer_program).unwrap();
                    let mut other = rv.enter(sid, &other_program).unwrap();
                    (async {
                        sender.send::<Msg<39, ()>>(&()).await.unwrap();
                        for visit in 0..complete_visits {
                            sender.send::<Msg<36, ()>>(&()).await.unwrap();
                            peer.recv::<Msg<36, ()>>().await.unwrap();
                            if visit % 2 == 1 {
                                peer.send::<Msg<38, u32>>(&38).await.unwrap();
                                assert_eq!(sender.recv::<Msg<38, u32>>().await.unwrap(), 38);
                            } else {
                                peer.send::<Msg<37, u32>>(&37).await.unwrap();
                                assert_eq!(sender.recv::<Msg<37, u32>>().await.unwrap(), 37);
                            }
                        }
                        if begin_next_visit {
                            sender.send::<Msg<36, ()>>(&()).await.unwrap();
                            peer.recv::<Msg<36, ()>>().await.unwrap();
                        }
                        drop(sender);
                        if complete_visits == 0 || begin_next_visit {
                            let error = other.recv::<Msg<39, ()>>().await.unwrap_err();
                            assert!(format!("{error:?}").contains("SessionFault(EndpointDropped)"));
                        } else {
                            // Unchosen route alternatives are not outstanding;
                            // Drop must leave the queued sibling consumable.
                            other.recv::<Msg<39, ()>>().await.unwrap();
                        }
                    })
                    .await;
                };
            };
        }
    }
}

fn routefree_parallel_sends() -> impl hibana::runtime::program::Projectable {
    g::par(
        g::send::<2, 3, Msg<40, u32>>(),
        g::send::<2, 4, Msg<41, u32>>(),
    )
}

fn unrelated_route_then_parallel_sends() -> impl hibana::runtime::program::Projectable {
    g::seq(
        g::route(
            g::send::<0, 1, Msg<42, ()>>(),
            g::send::<0, 1, Msg<43, ()>>(),
        ),
        routefree_parallel_sends(),
    )
}

fn unrelated_rolled_route_then_parallel_sends() -> impl hibana::runtime::program::Projectable {
    g::seq(
        g::route(
            g::send::<0, 1, Msg<42, ()>>(),
            g::send::<0, 1, Msg<43, ()>>(),
        )
        .roll(),
        routefree_parallel_sends(),
    )
}

#[derive(Clone, Copy, Debug)]
enum ParallelCompletionProbe {
    Complete,
    MisplacedOffer,
    ReplaySend,
    ReplayRecv,
    IncompleteDrop,
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn routefree_parallel_progress_preserves_all_completion_and_fault_cases() {
    let direct = routefree_parallel_sends();
    let shifted = unrelated_route_then_parallel_sends();
    let rolled = unrelated_rolled_route_then_parallel_sends();
    let programs = [
        (
            "direct",
            project::<2>(&direct),
            project::<3>(&direct),
            project::<4>(&direct),
            project::<0>(&direct),
        ),
        (
            "shifted",
            project::<2>(&shifted),
            project::<3>(&shifted),
            project::<4>(&shifted),
            project::<0>(&shifted),
        ),
        (
            "rolled",
            project::<2>(&rolled),
            project::<3>(&rolled),
            project::<4>(&rolled),
            project::<0>(&rolled),
        ),
    ];
    // All previous27 cases plus the three left-first misplaced-offer cases.
    for (name, sender_program, left_program, right_program, observer_program) in &programs {
        for right_first in [false, true] {
            for probe in [
                ParallelCompletionProbe::Complete,
                ParallelCompletionProbe::MisplacedOffer,
                ParallelCompletionProbe::ReplaySend,
                ParallelCompletionProbe::ReplayRecv,
                ParallelCompletionProbe::IncompleteDrop,
            ] {
                eprintln!("source={name}, right_first={right_first}, probe={probe:?}");

                {
                    let mut slab = [0; 64 * 1024];
                    {
                        let mut storage = SessionKitStorage::uninit();
                        let cluster = storage.init();
                        let transport = TestTransport::new();
                        let rv = cluster.rendezvous(&mut slab, transport.clone()).unwrap();
                        let sid = SessionId::new(940);
                        let mut sender = rv.enter(sid, sender_program).unwrap();
                        let mut left = rv.enter(sid, left_program).unwrap();
                        let mut right = rv.enter(sid, right_program).unwrap();
                        (async {
                if right_first {
                    sender.send::<Msg<41, u32>>(&41).await.unwrap();
                    assert_eq!(right.recv::<Msg<41, u32>>().await.unwrap(), 41);
                } else {
                    sender.send::<Msg<40, u32>>(&40).await.unwrap();
                    assert_eq!(left.recv::<Msg<40, u32>>().await.unwrap(), 40);
                }
                assert!(transport.queue_is_empty());
                match probe {
                    ParallelCompletionProbe::Complete => {
                        if right_first {
                            sender.send::<Msg<40, u32>>(&40).await.unwrap();
                            assert_eq!(left.recv::<Msg<40, u32>>().await.unwrap(), 40);
                        } else {
                            sender.send::<Msg<41, u32>>(&41).await.unwrap();
                            assert_eq!(right.recv::<Msg<41, u32>>().await.unwrap(), 41);
                        }
                    }
                    ParallelCompletionProbe::IncompleteDrop => {}
                    ParallelCompletionProbe::MisplacedOffer => {
                        use core::{
                            future::Future,
                            pin::pin,
                            task::{Context, Poll},
                        };
                        let mut cx = Context::from_waker(std::task::Waker::noop());
                        // Regression: route-free offer traversal previously
                        // indexed a zero-capacity visit set here and panicked.
                        match pin!(sender.offer()).poll(&mut cx) {
                            Poll::Ready(Err(error)) => {
                                assert_eq!(
                                    format!("{error:?}"),
                                    "EndpointError { operation: \"offer\", kind: PhaseInvariant }"
                                );
                            }
                            Poll::Ready(Ok(_)) => panic!("send-only continuation cannot offer"),
                            Poll::Pending => panic!("misplaced offer must reject immediately"),
                        };
                    }
                    ParallelCompletionProbe::ReplayRecv => {
                        use core::{
                            future::Future,
                            pin::pin,
                            task::{Context, Poll},
                        };
                        let mut cx = Context::from_waker(std::task::Waker::noop());
                        let rejected = if right_first {
                            matches!(
                                pin!(right.recv::<Msg<41, u32>>()).poll(&mut cx),
                                Poll::Ready(Err(_))
                            )
                        } else {
                            matches!(
                                pin!(left.recv::<Msg<40, u32>>()).poll(&mut cx),
                                Poll::Ready(Err(_))
                            )
                        };
                        assert!(
                            rejected,
                            "completed receive replay must reject without waiting"
                        );
                        assert!(transport.queue_is_empty());
                    }
                    ParallelCompletionProbe::ReplaySend => {
                        let replay = if right_first {
                            sender.send::<Msg<41, u32>>(&141).await
                        } else {
                            sender.send::<Msg<40, u32>>(&140).await
                        };
                        assert!(replay.is_err(), "a completed parallel send cannot replay");
                        assert!(
                            transport.queue_is_empty(),
                            "replay must not publish a frame"
                        );
                    }
                }
            }).await;
                        assert!(transport.queue_is_empty());
                        drop(sender);
                        match probe {
                            ParallelCompletionProbe::Complete => {
                                // Existing session admission rejects a faulted generation.
                                // Another role can enter only if Drop left it healthy.
                                drop(rv.enter(sid, observer_program).unwrap());
                            }
                            ParallelCompletionProbe::MisplacedOffer
                            | ParallelCompletionProbe::ReplaySend
                            | ParallelCompletionProbe::ReplayRecv
                            | ParallelCompletionProbe::IncompleteDrop => {
                                let error = if right_first {
                                    (left.recv::<Msg<40, u32>>()).await.unwrap_err()
                                } else {
                                    (right.recv::<Msg<41, u32>>()).await.unwrap_err()
                                };
                                assert!(format!("{error:?}").contains("SessionFault"));
                                if matches!(probe, ParallelCompletionProbe::IncompleteDrop) {
                                    assert!(format!("{error:?}").contains("EndpointDropped"));
                                }
                            }
                        }
                    };
                };
            }
        }
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn filtered_route_shift_preserves_passive_child_and_full_commit_chain() {
    for choose_inner_right in [false, true] {
        {
            let mut slab = [0; 64 * 1024];
            {
                let mut storage = SessionKitStorage::uninit();
                let cluster = storage.init();
                // The first route is unrelated to roles 2/3. Their retained
                // outer/inner scopes move from slots 1/2 to slots 0/1.
                let graph = g::seq(
                    g::route(
                        g::send::<0, 1, Msg<50, ()>>(),
                        g::send::<0, 1, Msg<51, ()>>(),
                    ),
                    g::route(
                        g::route(
                            g::send::<2, 3, Msg<52, u32>>(),
                            g::send::<2, 3, Msg<53, u32>>(),
                        ),
                        g::send::<2, 3, Msg<54, u32>>(),
                    ),
                );
                let sender_program = project::<2>(&graph);
                let receiver_program = project::<3>(&graph);
                let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
                let sid = SessionId::new(1941);
                let mut sender = rv.enter(sid, &sender_program).unwrap();
                let mut receiver = rv.enter(sid, &receiver_program).unwrap();
                (async {
                    if choose_inner_right {
                        sender.send::<Msg<53, u32>>(&53).await.unwrap();
                        drop(sender);
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(branch.label(), 53);
                        assert_eq!(branch.recv::<Msg<53, u32>>().await.unwrap(), 53);
                    } else {
                        sender.send::<Msg<52, u32>>(&52).await.unwrap();
                        drop(sender);
                        let branch = receiver.offer().await.unwrap();
                        assert_eq!(branch.label(), 52);
                        assert_eq!(branch.recv::<Msg<52, u32>>().await.unwrap(), 52);
                    }
                })
                .await;
                drop(receiver);
            };
        };
    }
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn filtered_entirely_empty_role_drops_without_visit_storage() {
    {
        let mut slab = [0; 64 * 1024];
        {
            let mut storage = SessionKitStorage::uninit();
            let cluster = storage.init();
            let graph = g::route(
                g::send::<0, 2, Msg<55, ()>>(),
                g::send::<0, 2, Msg<56, ()>>(),
            );
            let program = project::<1>(&graph);
            let rv = cluster.rendezvous(&mut slab, TestTransport::new()).unwrap();
            let sid = SessionId::new(1942);
            let sender_program = project::<0>(&graph);
            let receiver_program = project::<2>(&graph);
            let mut sender = rv.enter(sid, &sender_program).unwrap();
            let mut receiver = rv.enter(sid, &receiver_program).unwrap();
            let endpoint = rv.enter(sid, &program).unwrap();
            drop(endpoint);
            (async {
                sender.send::<Msg<55, ()>>(&()).await.unwrap();
                receiver.recv::<Msg<55, ()>>().await.unwrap();
            })
            .await;
        };
    };
}

/// Success transfers the reply authority away from the service. Its ordinary
/// file-I/O view remains usable; no extra rejection can be sent by that service.
#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn successful_close_returns_its_authority_without_reopening_a_rejection()
-> Result<(), hibana::EndpointError> {
    use core::{
        cell::Cell,
        future::Future,
        task::{Context, Poll, Waker},
    };
    use hibana::Endpoint;
    type Authority = g::Msg<10, Endpoint<'static, 36>>;
    type Request = g::Msg<162, u32>;
    type Approved = g::Msg<163, ()>;
    type Rejected = g::Msg<164, ()>;
    type Read = g::Msg<90, ()>;
    type Value = g::Msg<91, u32>;
    const CHECK: u16 = 8120;
    for rejected in [0, 2] {
        let request = g::send::<1, 36, Request>();
        let approved = g::seq(g::send::<36, 1, Approved>(), g::send::<0, 1, Authority>());
        let retry = g::seq(g::send::<36, 1, Rejected>(), request);
        let operation = g::seq(
            request,
            g::route(approved, g::seq(retry.roll(), approved)).resolve::<CHECK>(),
        );
        let global = g::par(
            operation,
            g::seq(g::send::<1, 0, Read>(), g::send::<0, 1, Value>()),
        );
        let owner_program = project::<0>(&global);
        let client_program = project::<1>(&global);
        let authority_program = project::<36>(&global);
        let input = Cell::new(None::<u32>);
        let transport = common::TestTransport::new();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, transport.clone())
            .unwrap();
        rv.set_resolver(
            &authority_program,
            ResolverRef::<CHECK>::decision_state(&input, |input| match input.get() {
                Some(0) => Ok(DecisionArm::Right),
                Some(_) => Ok(DecisionArm::Left),
                None => Err(ResolverError::reject()),
            }),
        )
        .unwrap();
        let sid = SessionId::new(8120);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let mut client = rv.enter(sid, &client_program).unwrap();
        let mut authority = rv.enter(sid, &authority_program).unwrap();
        for _ in 0..rejected {
            let value = 0;
            client.send::<Request>(&value).await?;
            let request = authority.offer().await?;
            assert_eq!(request.label(), 162);
            input.set(Some(request.recv::<Request>().await?));
            assert_eq!(authority.offer().await?.label(), 164);
            authority.send::<Rejected>(&()).await?;
            client.recv::<Rejected>().await?;
        }
        client.send::<Request>(&1).await?;
        let request = authority.offer().await?;
        assert_eq!(request.label(), 162);
        input.set(Some(request.recv::<Request>().await?));
        drop(authority.offer().await?);
        authority.send::<Approved>(&()).await?;
        client.recv::<Approved>().await?;
        owner.send::<Authority>(authority).await?;
        let mut returned = client.recv::<Authority>().await?;
        assert!(transport.queue_is_empty());
        {
            let mut preview = core::pin::pin!(owner.offer());
            assert!(matches!(
                preview
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            ));
        }
        {
            let mut file = client.borrow(&mut returned);
            let mut repeated = core::pin::pin!(file.send::<Request>(&1));
            let observed = repeated
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()));
            assert!(
                matches!(&observed, Poll::Ready(Err(error)) if error.is_unavailable()),
                "closed authority admitted or waited for a new request: {observed:?}"
            );
        }
        assert!(transport.queue_is_empty());
        drop(returned);
        client.send::<Read>(&()).await?;
        let request = owner.offer().await?;
        assert_eq!(request.label(), 90);
        request.recv::<Read>().await?;
        owner.send::<Value>(&7).await?;
        assert_eq!(client.recv::<Value>().await?, 7);
        assert!(transport.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_parallel_continuation_can_follow_rejected_close_retries()
-> Result<(), hibana::EndpointError> {
    use core::cell::Cell;
    type Request = g::Msg<162, u32>;
    type Approved = g::Msg<163, ()>;
    type Rejected = g::Msg<164, ()>;
    type First = g::Msg<28, ()>;
    type Second = g::Msg<29, ()>;
    const CHECK: u16 = 8122;
    for rejected in [0, 2] {
        let request = g::send::<0, 1, Request>();
        let approved = g::seq(
            g::par(g::send::<1, 2, First>(), g::send::<1, 3, Second>()),
            g::send::<1, 0, Approved>(),
        );
        let retry = g::seq(g::send::<1, 0, Rejected>(), request);
        let global = g::seq(
            request,
            g::route(approved, g::seq(retry.roll(), approved)).resolve::<CHECK>(),
        );
        let client_program = project::<0>(&global);
        let server_program = project::<1>(&global);
        let first_program = project::<2>(&global);
        let second_program = project::<3>(&global);
        let input = Cell::new(None::<u32>);
        let transport = common::TestTransport::new();
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage
            .init()
            .rendezvous(&mut slab, transport.clone())
            .unwrap();
        rv.set_resolver(
            &server_program,
            ResolverRef::<CHECK>::decision_state(&input, |input| match input.get() {
                Some(0) => Ok(DecisionArm::Right),
                Some(_) => Ok(DecisionArm::Left),
                None => Err(ResolverError::reject()),
            }),
        )
        .unwrap();
        let sid = SessionId::new(8122);
        let mut client = rv.enter(sid, &client_program).unwrap();
        let mut server = rv.enter(sid, &server_program).unwrap();
        let mut first = rv.enter(sid, &first_program).unwrap();
        let mut second = rv.enter(sid, &second_program).unwrap();
        for _ in 0..rejected {
            client.send::<Request>(&0).await?;
            input.set(Some(server.recv::<Request>().await?));
            assert_eq!(server.offer().await?.label(), 164);
            server.send::<Rejected>(&()).await?;
            client.recv::<Rejected>().await?;
        }
        client.send::<Request>(&1).await?;
        input.set(Some(server.recv::<Request>().await?));
        drop(server.offer().await?);
        {
            let [mut left, mut right] = server.par()?;
            async {
                async {
                    left.send::<First>(&()).await?;
                };
                async {
                    right.send::<Second>(&()).await?;
                };
                Ok(())
            }
            .await?;
        }
        first.recv::<First>().await?;
        second.recv::<Second>().await?;
        server.send::<Approved>(&()).await?;
        client.recv::<Approved>().await?;
        assert!(transport.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_close_retry_input_is_not_lost_when_an_unrelated_parallel_borrow_is_rejected()
-> Result<(), hibana::EndpointError> {
    type Request = g::Msg<162, ()>;
    type Other = g::Msg<164, ()>;
    type Reply = g::Msg<163, ()>;
    type First = g::Msg<28, ()>;
    type Second = g::Msg<29, ()>;
    let visit = g::route(
        g::seq(g::send::<0, 1, Request>(), g::send::<1, 0, Reply>()),
        g::seq(g::send::<0, 1, Other>(), g::send::<1, 0, Reply>()),
    );
    let global = g::seq(
        visit.roll(),
        g::par(g::send::<1, 2, First>(), g::send::<1, 3, Second>()),
    );
    let client_program = project::<0>(&global);
    let server_program = project::<1>(&global);
    let first_program = project::<2>(&global);
    let second_program = project::<3>(&global);
    let transport = common::TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(8123);
    let mut client = rv.enter(sid, &client_program).unwrap();
    let mut server = rv.enter(sid, &server_program).unwrap();
    let mut first = rv.enter(sid, &first_program).unwrap();
    let mut second = rv.enter(sid, &second_program).unwrap();
    client.send::<Request>(&()).await?;
    server.recv::<Request>().await?;
    server.send::<Reply>(&()).await?;
    client.recv::<Reply>().await?;
    client.send::<Request>(&()).await?;
    let input = server.offer().await?;
    assert_eq!(input.label(), 162);
    drop(input);
    assert!(
        server.par::<2>().is_err(),
        "borrow discarded accepted input"
    );
    server.recv::<Request>().await?;
    server.send::<Reply>(&()).await?;
    client.recv::<Reply>().await?;
    {
        let [mut left, mut right] = server.par()?;
        async {
            async {
                left.send::<First>(&()).await?;
            };
            async {
                right.send::<Second>(&()).await?;
            };
            Ok(())
        }
        .await?;
    }
    first.recv::<First>().await?;
    second.recv::<Second>().await?;
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_unadmitted_wire_send_keeps_its_required_receive_and_later_send_available()
-> Result<(), hibana::EndpointError> {
    type Input = g::Msg<180, u32>;
    type Output = g::Msg<181, u32>;
    let global = g::seq(g::send::<1, 0, Input>(), g::send::<0, 1, Output>());
    let owner_program = project::<0>(&global);
    let peer_program = project::<1>(&global);
    let transport = TestTransport::new();
    let mut slab = [0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let session = SessionId::new(8121);
    let mut owner = rv.enter(session, &owner_program).unwrap();
    let mut peer = rv.enter(session, &peer_program).unwrap();
    let error = owner.send::<Output>(&7).await.unwrap_err();
    assert!(error.is_unavailable(), "{error:?}");
    assert!(transport.queue_is_empty());
    peer.send::<Input>(&5).await?;
    assert_eq!(owner.recv::<Input>().await?, 5);
    owner.send::<Output>(&7).await?;
    assert_eq!(peer.recv::<Output>().await?, 7);
    assert!(transport.queue_is_empty());
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_borrowed_retry_after_handoff_leaves_the_next_review_usable()
-> Result<(), hibana::EndpointError> {
    use core::{
        future::{Future, poll_fn},
        pin::pin,
        task::Poll,
    };
    use hibana::Endpoint;
    type Loan<'run> = Msg<207, Endpoint<'run, 3>>;
    type Material = Msg<208, u32>;
    type Review = Msg<209, u32>;
    type Outcome = Msg<210, u32>;
    type Edit = Msg<211, u32>;
    type Edited = Msg<212, u32>;
    type Settled = Msg<213, u32>;
    type Rework = Msg<214, u32>;
    let assessment = g::seq(g::send::<1, 2, Material>(), g::send::<2, 1, Review>());
    let rejected = g::seq(
        g::seq(g::send::<1, 5, Rework>(), g::send::<1, 2, Outcome>()),
        g::seq(
            g::send::<5, 3, Edit>(),
            g::seq(g::send::<3, 5, Edited>(), assessment),
        ),
    );
    let approved = g::seq(
        g::send::<1, 2, Settled>(),
        g::seq(g::send::<3, 5, Settled>(), g::send::<1, 0, Loan>()),
    );
    let source = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            assessment,
            g::seq(
                g::route(approved, g::seq(rejected.roll(), approved)),
                g::seq(
                    g::send::<0, 4, Loan>(),
                    g::seq(
                        g::send::<4, 2, Material>(),
                        g::seq(g::send::<2, 4, Review>(), g::send::<4, 2, Outcome>()),
                    ),
                ),
            ),
        ),
    );
    let transport = TestTransport::new();
    let mut slab = [0; 128 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let session = SessionId::new(1958);
    let mut owner = rv.enter(session, &project::<0>(&source)).unwrap();
    let mut earlier = rv.enter(session, &project::<1>(&source)).unwrap();
    let mut reviewer = rv.enter(session, &project::<2>(&source)).unwrap();
    let document = rv.enter(session, &project::<3>(&source)).unwrap();
    let mut later = rv.enter(session, &project::<4>(&source)).unwrap();
    let mut author = rv.enter(session, &project::<5>(&source)).unwrap();
    owner.send::<Loan>(document).await?;
    let mut document = earlier.recv::<Loan>().await?;
    earlier.send::<Material>(&1).await?;
    assert_eq!(reviewer.recv::<Material>().await?, 1);
    reviewer.send::<Review>(&2).await?;
    assert_eq!(earlier.recv::<Review>().await?, 2);
    earlier.send::<Rework>(&3).await?;
    assert_eq!(author.recv::<Rework>().await?, 3);
    earlier.send::<Outcome>(&3).await?;
    assert_eq!(reviewer.recv::<Outcome>().await?, 3);
    author.send::<Edit>(&4).await?;
    assert_eq!(document.recv::<Edit>().await?, 4);
    document.send::<Edited>(&5).await?;
    assert_eq!(author.recv::<Edited>().await?, 5);
    earlier.send::<Material>(&6).await?;
    assert_eq!(reviewer.recv::<Material>().await?, 6);
    reviewer.send::<Review>(&7).await?;
    assert_eq!(earlier.recv::<Review>().await?, 7);
    earlier.send::<Settled>(&8).await?;
    assert_eq!(reviewer.recv::<Settled>().await?, 8);
    document.send::<Settled>(&8).await?;
    assert_eq!(author.recv::<Settled>().await?, 8);
    earlier.send::<Loan>(document).await?;
    let document = owner.recv::<Loan>().await?;
    owner.send::<Loan>(document).await?;
    let _document = later.recv::<Loan>().await?;
    later.send::<Material>(&9).await?;
    assert_eq!(reviewer.recv::<Material>().await?, 9);
    assert!(transport.queue_is_empty());
    let old_start = {
        let mut call = pin!(earlier.send::<Rework>(&10));
        poll_fn(|cx| Poll::Ready(call.as_mut().poll(cx))).await
    };
    assert!(
        matches!(old_start, Poll::Ready(Err(error)) if error.is_unavailable()),
        "{old_start:?}"
    );
    assert!(transport.queue_is_empty());
    reviewer.send::<Review>(&11).await?;
    assert_eq!(later.recv::<Review>().await?, 11);
    later.send::<Outcome>(&12).await?;
    assert_eq!(reviewer.recv::<Outcome>().await?, 12);
    assert!(transport.queue_is_empty());
    Ok(())
}
