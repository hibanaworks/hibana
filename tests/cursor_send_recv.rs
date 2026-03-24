mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::{
    g::advanced::steps::{ProjectRole, SendStep, StepCons, StepNil},
    g::advanced::{RoleProgram, project},
    g::{self, Msg, Role},
    substrate::{SessionCluster, SessionId, binding::NoBinding, runtime::Config},
};
use runtime_support::{leak_slab, leak_tap_storage};

const PROGRAM: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil>> =
    g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();

static ORIGIN_PROGRAM: RoleProgram<
    'static,
    0,
    <StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil> as ProjectRole<Role<0>>>::Output,
> = project(&PROGRAM);
static TARGET_PROGRAM: RoleProgram<
    'static,
    1,
    <StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil> as ProjectRole<Role<1>>>::Output,
> = project(&PROGRAM);

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport
        .state
        .lock()
        .expect("state lock")
        .queues
        .values()
        .all(|queue| queue.is_empty())
}

#[test]
fn cursor_send_and_recv_roundtrip() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(1024);
    let config = Config::new(tap_buf, slab);
    let transport = TestTransport::default();
    let cluster: &mut SessionCluster<
        'static,
        TestTransport,
        hibana::substrate::runtime::DefaultLabelUniverse,
        hibana::substrate::runtime::CounterClock,
        4,
    > = Box::leak(Box::new(SessionCluster::new(runtime_support::leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(1);

    // Attach both endpoints FIRST so they're both registered
    let origin_endpoint = cluster
        .enter::<0, _, _, _>(rv_id, sid, &ORIGIN_PROGRAM, NoBinding)
        .expect("origin endpoint");
    let target_endpoint = cluster
        .enter::<1, _, _, _>(rv_id, sid, &TARGET_PROGRAM, NoBinding)
        .expect("target endpoint");

    // Now run send/recv concurrently
    let (send_result, recv_result) = futures::executor::block_on(async {
        futures::join!(
            async {
                origin_endpoint
                    .flow::<Msg<1, u32>>()
                    .unwrap()
                    .send(&42)
                    .await
            },
            target_endpoint.recv::<Msg<1, u32>>()
        )
    });

    let (origin_endpoint, outcome) = send_result.expect("send succeeds");
    assert!(outcome.is_none());
    let (target_endpoint, payload) = recv_result.expect("recv succeeds");
    assert_eq!(payload, 42u32);
    assert!(transport_queue_is_empty(&transport));

    drop(origin_endpoint);
    drop(target_endpoint);
}
