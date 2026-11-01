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
fn a_retained_loan_resumes_after_a_choice_that_keeps_its_owner() {
    type Loan = g::Msg<1, Endpoint<'static, 3>>;
    type Keep = g::Msg<2, ()>;
    type Leave = g::Msg<3, Endpoint<'static, 3>>;
    type Use = g::Msg<4, ()>;
    let body = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(g::send::<3, 1, Use>(), g::send::<1, 0, Loan>()),
    );
    let global = g::seq(
        body.roll(),
        g::route(g::send::<0, 2, Keep>(), g::send::<0, 2, Leave>()).resolve::<17>(),
    );
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
        &p0,
        ResolverRef::<17>::decision_state(&(), |_| Ok(DecisionArm::Left)),
    )
    .unwrap();
    let sid = SessionId::new(99);
    let mut owner = kit.enter(sid, &p0).unwrap();
    let mut worker = kit.enter(sid, &p1).unwrap();
    let mut observer = kit.enter(sid, &p2).unwrap();
    let initial = kit.enter(sid, &p3).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let operation = pin!(async {
        let mut resource = initial;
        for visit in 0..2 {
            owner.send::<Loan>(resource).await?;
            let mut borrowed = worker.recv::<Loan>().await?;
            borrowed.send::<Use>(&()).await?;
            worker.recv::<Use>().await?;
            worker.send::<Loan>(borrowed).await?;
            resource = owner.recv::<Loan>().await?;
            if visit == 0 {
                let branch = owner.offer().await?;
                assert_eq!(branch.label(), 2);
                branch.send::<Keep>(&()).await?;
                let branch = observer.offer().await?;
                assert_eq!(branch.label(), 2);
                branch.recv::<Keep>().await?;
            }
        }
        drop(resource);
        Ok::<_, hibana::EndpointError>(())
    });
    match operation.poll(&mut cx) {
        Poll::Ready(Ok(())) => {}
        result => panic!("retained-owner reentry did not complete: {result:?}"),
    }
}
