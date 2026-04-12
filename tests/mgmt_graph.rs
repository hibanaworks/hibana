#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{cell::UnsafeCell, mem::MaybeUninit};

use common::TestTransport;
use hibana::{
    g,
    g::advanced::project,
    g::advanced::steps::{SendStep, SeqSteps, StepCons, StepNil},
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        cap::advanced::MintConfig,
        mgmt::{self, SubscribeReq},
        runtime::{Config, CounterClock},
    },
};
use runtime_support::with_fixture;
use tls_ref_support::with_tls_ref;

type TestKit = SessionKit<
    'static,
    TestTransport,
    hibana::substrate::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;
type ObserveAppSteps = StepCons<
    SendStep<g::Role<{ mgmt::ROLE_CONTROLLER }>, g::Role<{ mgmt::ROLE_CLUSTER }>, g::Msg<121, ()>>,
    StepNil,
>;
type ObserveProgramSteps = SeqSteps<mgmt::observe_stream::PrefixSteps, ObserveAppSteps>;

const OBSERVE_APP: g::Program<ObserveAppSteps> = g::send::<
    g::Role<{ mgmt::ROLE_CONTROLLER }>,
    g::Role<{ mgmt::ROLE_CLUSTER }>,
    g::Msg<121, ()>,
    0,
>();
const OBSERVE_PROGRAM: g::Program<ObserveProgramSteps> =
    g::seq(mgmt::observe_stream::PREFIX, OBSERVE_APP);

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

#[test]
fn observe_stream_prefix_projects_and_enters_without_helper_surface() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::new(tap_buf, slab),
                        TestTransport::default(),
                    )
                    .expect("register rendezvous");

                let prefix = mgmt::observe_stream::PREFIX;
                let controller_program =
                    project::<{ mgmt::ROLE_CONTROLLER }, _, MintConfig>(&prefix);
                let cluster_program = project::<{ mgmt::ROLE_CLUSTER }, _, MintConfig>(&prefix);
                let _controller_endpoint = cluster
                    .enter(
                        rv_id,
                        SessionId::new(0xD00D),
                        &controller_program,
                        NoBinding,
                    )
                    .expect("enter observe controller");
                let _cluster_endpoint = cluster
                    .enter(rv_id, SessionId::new(0xD00D), &cluster_program, NoBinding)
                    .expect("enter observe cluster");

                let _subscribe = SubscribeReq::default();
                let _tap = mgmt::tap::TapEvent::default();
            },
        );
    });
}

#[test]
fn observe_stream_prefix_stays_composable_as_an_ordinary_choreography_prefix() {
    with_fixture(|clock, tap_buf, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::new(tap_buf, slab),
                        TestTransport::default(),
                    )
                    .expect("register rendezvous");

                let program = OBSERVE_PROGRAM;
                let controller_program =
                    project::<{ mgmt::ROLE_CONTROLLER }, _, MintConfig>(&program);
                let cluster_program = project::<{ mgmt::ROLE_CLUSTER }, _, MintConfig>(&program);
                let _controller_endpoint = cluster
                    .enter(
                        rv_id,
                        SessionId::new(0xD00E),
                        &controller_program,
                        NoBinding,
                    )
                    .expect("enter composed observe controller");
                let _cluster_endpoint = cluster
                    .enter(rv_id, SessionId::new(0xD00E), &cluster_program, NoBinding)
                    .expect("enter composed observe cluster");
            },
        );
    });
}
