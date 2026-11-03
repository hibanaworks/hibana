mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use ::core::{
    future::Future,
    mem::{size_of, size_of_val},
    task::{Context, Poll},
};

use common::TestTransport;
use hibana::g::{self, Msg};
use hibana::runtime::program::project;
use hibana::runtime::{SessionKitStorage, ids::SessionId};

const ROUTE_BRANCH_BYTES_MAX: usize = 32;
const OFFER_FUTURE_BYTES_MAX: usize = 48;
const BRANCH_RECV_FUTURE_BYTES_MAX: usize = 48;

fn choreography() -> impl hibana::runtime::program::Projectable {
    let inner_route = g::route(
        g::send::<0, 1, Msg<7, u32>>(),
        g::send::<0, 1, Msg<8, u32>>(),
    );

    let outer_left = g::seq(g::send::<0, 1, Msg<5, u32>>(), inner_route);

    let outer_right = g::send::<0, 1, Msg<6, u32>>();

    g::route(outer_left, outer_right)
}

// Test nested first-visible routes.
#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_branch_commit_stack() {
    let mut slab = [0; 64 * 1024];

    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    let transport = TestTransport::new();
    let rv = cluster
        .rendezvous(&mut slab, transport.clone())
        .expect("register rv");

    let sid = SessionId::new(77);
    let global = choreography();
    let controller_program = project::<0>(&global);
    let worker_program = project::<1>(&global);

    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");

    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    // Outer route: Controller sends wire data to Worker
    controller
        .send::<Msg<5, u32>>(&1234)
        .await
        .expect("send outer left data");
    // Outer route: Worker offers route arm, then receives selected data
    let outer_branch = worker.offer().await.expect("offer outer route");
    assert_eq!(
        outer_branch.label(),
        5,
        "outer route should expose OuterLeftData"
    );
    let observed_outer = outer_branch
        .recv::<Msg<5, u32>>()
        .await
        .expect("decode outer left data");
    assert_eq!(observed_outer, 1234);
    // Inner route: Controller sends wire data to Worker
    controller
        .send::<Msg<7, u32>>(&5678)
        .await
        .expect("send inner left data");
    // Inner route: Worker offers route arm, then receives selected data
    let inner_branch = worker.offer().await.expect("offer inner route");
    assert_eq!(
        inner_branch.label(),
        7,
        "inner route should expose InnerLeftData"
    );
    let observed_inner = inner_branch
        .recv::<Msg<7, u32>>()
        .await
        .expect("decode inner left data");
    assert_eq!(observed_inner, 5678);
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn forgotten_started_offer_future_leaves_endpoint_fail_closed() {
    let mut slab = [0; 64 * 1024];

    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    let transport = TestTransport::new();
    let rv = cluster
        .rendezvous(&mut slab, transport)
        .expect("register rv");

    let sid = SessionId::new(79);
    let global = choreography();
    let controller_program = project::<0>(&global);
    let worker_program = project::<1>(&global);

    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");
    core::hint::black_box(&mut controller);

    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    let mut offer = core::mem::ManuallyDrop::new(worker.offer());
    let waker = core::task::Waker::noop().clone();
    let mut cx = Context::from_waker(&waker);
    if let Poll::Ready(result) = Future::poll(core::pin::Pin::new(&mut *offer), &mut cx) {
        match result {
            Ok(_) => {
                panic!("offer should wait before the route decision")
            }
            Err(error) => {
                panic!("offer failed before the route decision: {error:?}")
            }
        }
    }

    let error = match (worker.offer()).await {
        Ok(_) => panic!("forgotten pending offer must leave endpoint fail-closed"),
        Err(error) => error,
    };
    assert!(format!("{error:?}").contains("operation: \"offer\""));
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("PhaseInvariant")
            || rendered.contains("ProgressInvariantViolated")
            || rendered.contains("SessionFault"),
        "busy endpoint must preserve terminal progress evidence: {rendered}"
    );
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn localside_branch_recv_sizes_stay_compact() {
    let mut slab = [0; 64 * 1024];

    let mut storage = SessionKitStorage::uninit();
    let cluster = storage.init();
    let transport = TestTransport::new();
    let rv = cluster
        .rendezvous(&mut slab, transport)
        .expect("register rv");

    let sid = SessionId::new(78);
    let global = choreography();
    let controller_program = project::<0>(&global);
    let worker_program = project::<1>(&global);

    let mut controller = rv
        .enter(sid, &controller_program)
        .expect("attach controller");

    let mut worker = rv.enter(sid, &worker_program).expect("attach worker");
    let offer = worker.offer();
    let offer_bytes = size_of_val(&offer);
    drop(offer);

    (controller.send::<Msg<5, u32>>(&1234))
        .await
        .expect("send outer left data");

    let branch = (worker.offer()).await.expect("offer route");
    let branch_bytes = size_of::<hibana::RouteBranch<1>>();
    assert_eq!(branch.label(), 5, "route should expose outer left arm");

    let recv = branch.recv::<Msg<5, u32>>();
    let recv_bytes = size_of_val(&recv);
    drop(recv);

    assert!(
        branch_bytes <= ROUTE_BRANCH_BYTES_MAX,
        "route branch handle regressed: {branch_bytes} > {ROUTE_BRANCH_BYTES_MAX}"
    );
    assert!(
        offer_bytes <= OFFER_FUTURE_BYTES_MAX,
        "offer future regressed: {offer_bytes} > {OFFER_FUTURE_BYTES_MAX}"
    );
    assert!(
        recv_bytes <= BRANCH_RECV_FUTURE_BYTES_MAX,
        "branch recv future regressed: {recv_bytes} > {BRANCH_RECV_FUTURE_BYTES_MAX}"
    );
}
