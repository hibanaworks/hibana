//! A different branch's possible holder cannot supply a retained operation.
#[path = "../examples/support/in_memory.rs"]
mod in_memory;

use futures::{FutureExt, executor::block_on};
use hibana::{
    Endpoint, g,
    runtime::{SessionKitStorage, ids::SessionId, program::project, transport::EndpointBinding},
};
use in_memory::InMemoryTransport;

#[test]
fn returning_a_conditionally_held_endpoint_preserves_its_selected_owner_guard() {
    type Notice = g::Msg<0, ()>;
    type Loan<'r> = g::Msg<1, Endpoint<'r, 3>>;
    type Proxy<'r> = g::Msg<2, Endpoint<'r, 2>>;
    type Work = g::Msg<3, ()>;
    type Returned<'r> = g::Msg<4, Endpoint<'r, 3>>;
    let global = g::seq(
        g::route(
            g::send::<0, 1, Notice>(),
            g::seq(g::send::<0, 1, Loan>(), g::send::<0, 1, Proxy>()),
        ),
        g::seq(g::send::<3, 0, Work>().roll(), g::send::<2, 0, Returned>()),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let proxy_program = project::<2>(&global);
    let resource_program = project::<3>(&global);

    for delegated in [false, true] {
        block_on(async {
            let sid = SessionId::new(9170 + u32::from(delegated));
            // SAFETY: each case owns two distinct physical test resources and
            // keeps their sole bindings until every Endpoint has been dropped.
            let network = InMemoryTransport::new(unsafe {
                [
                    ((sid, 2), EndpointBinding::new()),
                    ((sid, 3), EndpointBinding::new()),
                ]
            });
            let mut slab = [0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage.init().rendezvous(&mut slab, network).unwrap();
            let mut owner = rv.enter(sid, &owner_program).unwrap();
            let mut worker = rv.enter(sid, &worker_program).unwrap();
            let mut proxy = rv.enter(sid, &proxy_program).unwrap();
            let mut resource = rv.enter(sid, &resource_program).unwrap();

            if delegated {
                owner.send::<Loan>(resource).await.unwrap();
                owner.send::<Proxy>(proxy).await.unwrap();
                let mut resource = worker.recv::<Loan>().await.unwrap();
                let mut proxy = worker.recv::<Proxy>().await.unwrap();
                resource.send::<Work>(&()).await.unwrap();
                owner.recv::<Work>().await.unwrap();
                proxy.send::<Returned>(resource).await.unwrap();
                let mut resource = owner.recv::<Returned>().await.unwrap();
                let result = resource.send::<Work>(&()).now_or_never();
                let error = result
                    .expect("an inherited operation must be rejected rather than left pending")
                    .expect_err("the other branch cannot grant this holder the rolled operation");
                assert!(format!("{error:?}").contains("OutOfOrder"), "{error:?}");
            } else {
                owner.send::<Notice>(&()).await.unwrap();
                worker.recv::<Notice>().await.unwrap();
                resource.send::<Work>(&()).await.unwrap();
                owner.recv::<Work>().await.unwrap();
                proxy.send::<Returned>(resource).await.unwrap();
                let mut resource = owner.recv::<Returned>().await.unwrap();
                resource.send::<Work>(&()).await.unwrap();
                owner.recv::<Work>().await.unwrap();
            }
        });
    }
}

#[test]
fn moving_an_unrelated_resource_preserves_the_retained_roll() {
    type Loan<'r> = g::Msg<0, Endpoint<'r, 3>>;
    type Work = g::Msg<1, ()>;
    type Proxy<'r> = g::Msg<2, Endpoint<'r, 2>>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(g::send::<3, 0, Work>().roll(), g::send::<0, 1, Proxy>()),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let proxy_program = project::<2>(&global);
    let resource_program = project::<3>(&global);

    block_on(async {
        let sid = SessionId::new(9172);
        // SAFETY: two distinct resources with one binding each for this run.
        let network = InMemoryTransport::new(unsafe {
            [
                ((sid, 2), EndpointBinding::new()),
                ((sid, 3), EndpointBinding::new()),
            ]
        });
        let mut slab = [0; 64 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage.init().rendezvous(&mut slab, network).unwrap();
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let proxy = rv.enter(sid, &proxy_program).unwrap();
        let resource = rv.enter(sid, &resource_program).unwrap();
        owner.send::<Loan>(resource).await.unwrap();
        let mut resource = worker.recv::<Loan>().await.unwrap();
        resource.send::<Work>(&()).await.unwrap();
        owner.recv::<Work>().await.unwrap();
        owner.send::<Proxy>(proxy).await.unwrap();
        let _proxy = worker.recv::<Proxy>().await.unwrap();
        resource.send::<Work>(&()).await.unwrap();
        owner.recv::<Work>().await.unwrap();
    });
}
