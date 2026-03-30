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
        mgmt::{self, LoadRequest},
        policy::epf::Slot,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
    },
};
use runtime_support::{leak_clock, leak_slab, leak_tap_storage};

const SLOT: Slot = Slot::Rendezvous;

#[test]
fn request_reply_prefix_projects_and_enters_without_helper_surface() {
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
        project::<{ mgmt::ROLE_CONTROLLER }, _, MintConfig>(&mgmt::request_reply::PREFIX);
    let cluster_program =
        project::<{ mgmt::ROLE_CLUSTER }, _, MintConfig>(&mgmt::request_reply::PREFIX);

    let _controller = cluster
        .enter(
            rv_id,
            SessionId::new(0xCAFE),
            &controller_program,
            NoBinding,
        )
        .expect("enter mgmt controller");
    let _cluster = cluster
        .enter(rv_id, SessionId::new(0xCAFE), &cluster_program, NoBinding)
        .expect("enter mgmt cluster");

    let _request = mgmt::Request::LoadAndActivate(LoadRequest {
        slot: SLOT,
        code: &[0x30, 0x03, 0x00, 0x01],
        fuel_max: 64,
        mem_len: 128,
    });
}

#[test]
fn request_reply_prefix_stays_composable_as_an_ordinary_choreography_prefix() {
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
        g::Msg<120, u32>,
        2,
    >();
    let program = compose::seq(mgmt::request_reply::PREFIX, app);
    let controller_program = project::<{ mgmt::ROLE_CONTROLLER }, _, MintConfig>(&program);
    let cluster_program = project::<{ mgmt::ROLE_CLUSTER }, _, MintConfig>(&program);

    let _controller = cluster
        .enter(
            rv_id,
            SessionId::new(0xCB00),
            &controller_program,
            NoBinding,
        )
        .expect("enter composed controller");
    let _cluster = cluster
        .enter(rv_id, SessionId::new(0xCB00), &cluster_program, NoBinding)
        .expect("enter composed cluster");
}
