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
    g::advanced::steps::{SendStep, SeqSteps, StepCons, StepNil},
    g::advanced::{RoleProgram, project},
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        cap::advanced::MintConfig,
        mgmt::{self, LoadRequest},
        policy::epf::Slot,
        runtime::{Config, CounterClock},
    },
};
use runtime_support::with_fixture;
use tls_ref_support::with_tls_ref;

const SLOT: Slot = Slot::Rendezvous;
type TestKit = SessionKit<
    'static,
    TestTransport,
    hibana::substrate::runtime::DefaultLabelUniverse,
    CounterClock,
    2,
>;
type MgmtAppSteps = StepCons<
    SendStep<g::Role<{ mgmt::ROLE_CONTROLLER }>, g::Role<{ mgmt::ROLE_CLUSTER }>, g::Msg<120, u32>>,
    StepNil,
>;
type MgmtProgramSteps = SeqSteps<mgmt::request_reply::PrefixSteps, MgmtAppSteps>;

const MGMT_APP: g::Program<MgmtAppSteps> = g::send::<
    g::Role<{ mgmt::ROLE_CONTROLLER }>,
    g::Role<{ mgmt::ROLE_CLUSTER }>,
    g::Msg<120, u32>,
    0,
>();
const MGMT_PROGRAM: g::Program<MgmtProgramSteps> = g::seq(mgmt::request_reply::PREFIX, MGMT_APP);

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

#[test]
fn request_reply_prefix_projects_and_enters_without_helper_surface() {
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

                let prefix = mgmt::request_reply::PREFIX;
                let controller_program: RoleProgram<'_, { mgmt::ROLE_CONTROLLER }, _, MintConfig> =
                    project(&prefix);
                let cluster_program: RoleProgram<'_, { mgmt::ROLE_CLUSTER }, _, MintConfig> =
                    project(&prefix);
                let _controller_endpoint = cluster
                    .enter(
                        rv_id,
                        SessionId::new(0xCAFE),
                        &controller_program,
                        NoBinding,
                    )
                    .expect("enter mgmt controller");
                let _cluster_endpoint = cluster
                    .enter(rv_id, SessionId::new(0xCAFE), &cluster_program, NoBinding)
                    .expect("enter mgmt cluster");

                let _request = mgmt::Request::LoadAndActivate(LoadRequest {
                    slot: SLOT,
                    code: &[0x30, 0x03, 0x00, 0x01],
                    fuel_max: 64,
                    mem_len: 128,
                });
            },
        );
    });
}

#[test]
fn request_reply_prefix_stays_composable_as_an_ordinary_choreography_prefix() {
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

                let program = MGMT_PROGRAM;
                let controller_program: RoleProgram<'_, { mgmt::ROLE_CONTROLLER }, _, MintConfig> =
                    project(&program);
                let cluster_program: RoleProgram<'_, { mgmt::ROLE_CLUSTER }, _, MintConfig> =
                    project(&program);
                let _controller_endpoint = cluster
                    .enter(
                        rv_id,
                        SessionId::new(0xCB00),
                        &controller_program,
                        NoBinding,
                    )
                    .expect("enter composed controller");
                let _cluster_endpoint = cluster
                    .enter(rv_id, SessionId::new(0xCB00), &cluster_program, NoBinding)
                    .expect("enter composed cluster");
            },
        );
    });
}
