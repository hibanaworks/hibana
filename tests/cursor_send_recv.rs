mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{cell::UnsafeCell, mem::MaybeUninit};

use common::TestTransport;
use hibana::{
    g::advanced::steps::{SendStep, StepCons, StepNil},
    g::advanced::{ProgramWitness, RoleProgram, project},
    g::{self, Msg, Role},
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        runtime::{Config, CounterClock},
    },
};
use runtime_support::with_fixture;
use tls_ref_support::with_tls_ref;

const PROGRAM: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil>> =
    g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();

static ORIGIN_PROGRAM: RoleProgram<
    'static,
    0,
    ProgramWitness<StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil>>,
> = project(&PROGRAM);
static TARGET_PROGRAM: RoleProgram<
    'static,
    1,
    ProgramWitness<StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil>>,
> = project(&PROGRAM);
type TestKit = SessionKit<
    'static,
    TestTransport,
    hibana::substrate::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

#[test]
fn cursor_send_and_recv_roundtrip() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                    .expect("register rendezvous");

                let sid = SessionId::new(1);
                let mut origin_endpoint = cluster
                    .enter(rv_id, sid, &ORIGIN_PROGRAM, NoBinding)
                    .expect("origin endpoint");
                let mut target_endpoint = cluster
                    .enter(rv_id, sid, &TARGET_PROGRAM, NoBinding)
                    .expect("target endpoint");

                let outcome = futures::executor::block_on(
                    origin_endpoint
                        .flow::<Msg<1, u32>>()
                        .expect("send flow")
                        .send(&42),
                )
                .expect("send succeeds");
                assert!(outcome.is_none());
                let payload = futures::executor::block_on(target_endpoint.recv::<Msg<1, u32>>())
                    .expect("recv succeeds");
                assert_eq!(payload, 42u32);
                assert!(transport_queue_is_empty(&transport));
            },
        );
    });
}
