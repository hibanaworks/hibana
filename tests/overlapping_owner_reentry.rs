mod common;

use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use hibana::{
    Endpoint, g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        resolver::{DecisionArm, ResolverRef},
    },
};

#[test]
fn an_incoming_continuation_can_arrive_during_the_next_loan() {
    type Loan = g::Msg<1, Endpoint<'static, 3>>;
    type Keep = g::Msg<2, ()>;
    type Leave = g::Msg<3, Endpoint<'static, 3>>;
    type Use = g::Msg<4, ()>;
    type Ask = g::Msg<5, ()>;
    let body = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(g::send::<3, 1, Use>(), g::send::<1, 0, Loan>()),
    );
    let tail = g::route(
        g::send::<2, 0, Keep>(),
        g::seq(g::send::<2, 0, Ask>(), g::send::<0, 2, Leave>()),
    )
    .resolve::<17>();
    let global = g::seq(body.roll(), tail);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let mut slab = [0; 32768];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, common::TestTransport::new())
        .unwrap();
    kit.set_resolver(
        &p2,
        ResolverRef::<17>::decision_state(&(), |_| Ok(DecisionArm::Left)),
    )
    .unwrap();
    let sid = SessionId::new(100);
    let mut owner = kit.enter(sid, &p0).unwrap();
    let mut worker = kit.enter(sid, &p1).unwrap();
    let mut controller = kit.enter(sid, &p2).unwrap();
    let resource = kit.enter(sid, &p3).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let operation = pin!(async {
        owner.send::<Loan>(resource).await?;
        let mut resource = worker.recv::<Loan>().await?;
        resource.send::<Use>(&()).await?;
        worker.recv::<Use>().await?;
        worker.send::<Loan>(resource).await?;
        let resource = owner.recv::<Loan>().await?;

        owner.send::<Loan>(resource).await?;
        let mut resource = worker.recv::<Loan>().await?;

        controller.send::<Keep>(&()).await?;
        owner.recv::<Keep>().await?;

        resource.send::<Use>(&()).await?;
        worker.recv::<Use>().await?;
        worker.send::<Loan>(resource).await?;
        let resource = owner.recv::<Loan>().await?;
        drop(resource);
        Ok::<_, hibana::EndpointError>(())
    });
    match operation.poll(&mut cx) {
        Poll::Ready(Ok(())) => (),
        result => panic!("overlap did not complete: {result:?}"),
    }
}

#[test]
fn a_loan_can_resume_after_the_request_inside_a_continuation() {
    type Loan = g::Msg<1, Endpoint<'static, 3>>;
    type Keep = g::Msg<2, ()>;
    type Leave = g::Msg<3, Endpoint<'static, 3>>;
    type Use = g::Msg<4, ()>;
    type Ask = g::Msg<5, ()>;
    let body = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(g::send::<3, 1, Use>(), g::send::<1, 0, Loan>()),
    );
    let tail = g::route(
        g::send::<2, 0, Keep>(),
        g::seq(g::send::<2, 0, Ask>(), g::send::<0, 2, Leave>()),
    )
    .resolve::<17>();
    let global = g::seq(body.roll(), tail);
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let p3 = project::<3>(&global);
    let mut slab = [0; 32768];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage
        .init()
        .rendezvous(&mut slab, common::TestTransport::new())
        .unwrap();
    kit.set_resolver(
        &p2,
        ResolverRef::<17>::decision_state(&(), |_| Ok(DecisionArm::Right)),
    )
    .unwrap();
    let sid = SessionId::new(101);
    let mut owner = kit.enter(sid, &p0).unwrap();
    let mut worker = kit.enter(sid, &p1).unwrap();
    let mut controller = kit.enter(sid, &p2).unwrap();
    let resource = kit.enter(sid, &p3).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let operation = pin!(async {
        owner.send::<Loan>(resource).await?;
        let mut resource = worker.recv::<Loan>().await?;
        resource.send::<Use>(&()).await?;
        worker.recv::<Use>().await?;
        worker.send::<Loan>(resource).await?;
        let resource = owner.recv::<Loan>().await?;

        owner.send::<Loan>(resource).await?;
        let mut resource = worker.recv::<Loan>().await?;
        resource.send::<Use>(&()).await?;
        worker.recv::<Use>().await?;
        worker.send::<Loan>(resource).await?;
        let resource = owner.recv::<Loan>().await?;

        controller.send::<Ask>(&()).await?;
        owner.recv::<Ask>().await?;
        owner.send::<Loan>(resource).await?;
        let mut resource = worker.recv::<Loan>().await?;
        resource.send::<Use>(&()).await?;
        worker.recv::<Use>().await?;
        worker.send::<Loan>(resource).await?;
        let resource = owner.recv::<Loan>().await?;

        owner.send::<Leave>(resource).await?;
        drop(controller.recv::<Leave>().await?);
        Ok::<_, hibana::EndpointError>(())
    });
    assert!(matches!(operation.poll(&mut cx), Poll::Ready(Ok(()))));
}
