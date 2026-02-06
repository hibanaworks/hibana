mod common;
mod support;

use common::TestTransport;
use hibana::{
    NoBinding,
    endpoint::ControlOutcome,
    g::{
        self, Msg, Role,
        steps::{ProjectRole, SendStep, StepCons, StepNil},
    },
    rendezvous::{Rendezvous, SessionId},
    runtime::{SessionCluster, config::Config},
};
use support::{leak_slab, leak_tap_storage};

type Origin = Role<0>;
type Target = Role<1>;
type PayloadMsg = Msg<1, u32>;
type GlobalSteps = StepCons<SendStep<Origin, Target, PayloadMsg, 0>, StepNil>;

const PROGRAM: g::Program<GlobalSteps> = g::send::<Origin, Target, PayloadMsg, 0>();

type OriginLocal = <GlobalSteps as ProjectRole<Origin>>::Output;
type TargetLocal = <GlobalSteps as ProjectRole<Target>>::Output;

static ORIGIN_PROGRAM: g::RoleProgram<'static, 0, OriginLocal> =
    g::project::<0, GlobalSteps, _>(&PROGRAM);
static TARGET_PROGRAM: g::RoleProgram<'static, 1, TargetLocal> =
    g::project::<1, GlobalSteps, _>(&PROGRAM);

#[test]
fn cursor_send_and_recv_roundtrip() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(1024);
    let config = Config::new(tap_buf, slab);
    let transport = TestTransport::default();
    let rendezvous: Rendezvous<
        '_,
        '_,
        TestTransport,
        hibana::runtime::consts::DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
    > = Rendezvous::from_config(config, transport.clone());
    let cluster: &mut SessionCluster<
        'static,
        TestTransport,
        hibana::runtime::consts::DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
        4,
    > = Box::leak(Box::new(SessionCluster::new(support::leak_clock())));
    let rv_id = cluster
        .add_rendezvous(rendezvous)
        .expect("register rendezvous");

    let sid = SessionId::new(1);

    // Attach both endpoints FIRST so they're both registered
    let origin_endpoint = cluster
        .attach_cursor::<0, _, _, _>(rv_id, sid, &ORIGIN_PROGRAM, NoBinding)
        .expect("origin endpoint");
    let target_endpoint = cluster
        .attach_cursor::<1, _, _, _>(rv_id, sid, &TARGET_PROGRAM, NoBinding)
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
    assert!(matches!(outcome, ControlOutcome::None));
    let (target_endpoint, payload) = recv_result.expect("recv succeeds");
    assert_eq!(payload, 42u32);
    assert!(transport.queue_is_empty());

    // Ensure typestate advanced to terminal state
    #[cfg(feature = "test-utils")]
    target_endpoint.phase_cursor().assert_terminal();

    drop(origin_endpoint);
    drop(target_endpoint);
}
