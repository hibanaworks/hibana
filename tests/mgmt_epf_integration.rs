#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::substrate::{
    SessionCluster, SessionId,
    binding::NoBinding,
    mgmt::{
        Reply,
        session::{self, LoadRequest},
    },
    policy::epf::Slot,
    runtime::{Config, CounterClock, DefaultLabelUniverse},
};
use runtime_support::{leak_clock, leak_slab, leak_tap_storage};

const SLOT: Slot = Slot::Rendezvous;
const FUEL_MAX: u16 = 64;
const MEM_LEN: u16 = 128;

fn sample_epf_code() -> [u8; 4] {
    [0x30, 0x03, 0x00, 0x01]
}

#[test]
fn management_session_loads_and_activates_epf_image() {
    let cluster: &'static SessionCluster<
        'static,
        TestTransport,
        DefaultLabelUniverse,
        CounterClock,
        4,
    > = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(
            Config::new(leak_tap_storage(), leak_slab(4096)),
            TestTransport::default(),
        )
        .expect("register rendezvous");

    let reply = futures::executor::block_on(async {
        let controller =
            session::enter_controller(cluster, rv_id, SessionId::new(0xCAFE), NoBinding)
                .expect("enter mgmt controller");
        let cluster_endpoint =
            session::enter_cluster(cluster, rv_id, SessionId::new(0xCAFE), NoBinding)
                .expect("enter mgmt cluster");
        let ((_, reply), _) = futures::future::try_join(
            session::Request::LoadAndActivate(LoadRequest {
                slot: SLOT,
                code: &sample_epf_code(),
                fuel_max: FUEL_MAX,
                mem_len: MEM_LEN,
            })
            .drive_controller(controller),
            session::drive_cluster(cluster, rv_id, SessionId::new(0xCAFE), cluster_endpoint),
        )
        .await
        .expect("management session succeeds");
        reply
    });

    let transition = match reply {
        Reply::ActivationScheduled(report) => report,
        other => panic!("expected activation reply, got {other:?}"),
    };

    assert_eq!(
        transition.version, 1,
        "first activation should produce version 1"
    );
    assert_eq!(
        transition.policy_stats.commits, 1,
        "activation policy stats should record one commit"
    );
    assert_eq!(
        transition.policy_stats.rollbacks, 0,
        "activation policy stats should not record rollbacks"
    );
    assert_eq!(
        transition.policy_stats.last_commit,
        Some(transition.version),
        "last_commit should match activated version"
    );
    assert_eq!(
        transition.policy_stats.last_rollback, None,
        "no rollback should be recorded"
    );
}
