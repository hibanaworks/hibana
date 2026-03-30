#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::{
    g,
    g::advanced::{compose, project},
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        cap::advanced::MintConfig,
        mgmt::{self, SubscribeReq},
        runtime::{Config, CounterClock, DefaultLabelUniverse},
    },
};
use runtime_support::{leak_clock, leak_slab, leak_tap_storage};

#[test]
fn observe_stream_prefix_projects_and_enters_without_helper_surface() {
    let cluster: &'static SessionKit<
        'static,
        TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        4,
    > = Box::leak(Box::new(SessionKit::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(
            Config::new(leak_tap_storage(), leak_slab(4096)),
            TestTransport::default(),
        )
        .expect("register rendezvous");

    let controller_program =
        project::<{ mgmt::ROLE_CONTROLLER }, _, MintConfig>(&mgmt::observe_stream::PREFIX);
    let cluster_program =
        project::<{ mgmt::ROLE_CLUSTER }, _, MintConfig>(&mgmt::observe_stream::PREFIX);

    let _controller = cluster
        .enter(
            rv_id,
            SessionId::new(0xD00D),
            &controller_program,
            NoBinding,
        )
        .expect("enter observe controller");
    let _cluster = cluster
        .enter(rv_id, SessionId::new(0xD00D), &cluster_program, NoBinding)
        .expect("enter observe cluster");

    let _subscribe = SubscribeReq::default();
    let _tap = mgmt::tap::TapEvent::default();
}

#[test]
fn observe_stream_prefix_stays_composable_as_an_ordinary_choreography_prefix() {
    let cluster: &'static SessionKit<
        'static,
        TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        4,
    > = Box::leak(Box::new(SessionKit::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(
            Config::new(leak_tap_storage(), leak_slab(4096)),
            TestTransport::default(),
        )
        .expect("register rendezvous");

    let app = g::send::<
        g::Role<{ mgmt::ROLE_CONTROLLER }>,
        g::Role<{ mgmt::ROLE_CLUSTER }>,
        g::Msg<121, ()>,
        2,
    >();
    let program = compose::seq(mgmt::observe_stream::PREFIX, app);
    let controller_program = project::<{ mgmt::ROLE_CONTROLLER }, _, MintConfig>(&program);
    let cluster_program = project::<{ mgmt::ROLE_CLUSTER }, _, MintConfig>(&program);

    let _controller = cluster
        .enter(
            rv_id,
            SessionId::new(0xD00E),
            &controller_program,
            NoBinding,
        )
        .expect("enter composed observe controller");
    let _cluster = cluster
        .enter(rv_id, SessionId::new(0xD00E), &cluster_program, NoBinding)
        .expect("enter composed observe cluster");
}
