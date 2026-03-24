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
        session::{self, LoadRequest, SlotRequest},
    },
    policy::epf::Slot,
    runtime::{Config, CounterClock, DefaultLabelUniverse},
};
use runtime_support::{leak_clock, leak_slab, leak_tap_storage};

const SLOT: Slot = Slot::Rendezvous;
const FUEL_MAX: u16 = 64;
const MEM_LEN: u16 = 128;

fn run_code(
    cluster: &'static SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>,
    rv_id: hibana::substrate::RendezvousId,
    sid: SessionId,
    request: session::Request<'_>,
) -> Reply {
    futures::executor::block_on(async {
        let controller = session::enter_controller(cluster, rv_id, sid, NoBinding)
            .expect("enter mgmt controller");
        let cluster_endpoint =
            session::enter_cluster(cluster, rv_id, sid, NoBinding).expect("enter mgmt cluster");
        let ((_, reply), _) = futures::future::try_join(
            request.drive_controller(controller),
            session::drive_cluster(cluster, rv_id, sid, cluster_endpoint),
        )
        .await
        .expect("management session succeeds");
        reply
    })
}

#[test]
fn management_session_tracks_activation_and_revert_versions() {
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
    let sid_v1 = SessionId::new(42);
    let sid_v2 = SessionId::new(43);
    let sid_revert = SessionId::new(44);

    let code_v1 = [0x00, 0x01];
    let code_v2 = [0x31, 0x34, 0x12, 0x01];

    let first = run_code(
        cluster,
        rv_id,
        sid_v1,
        session::Request::LoadAndActivate(LoadRequest {
            slot: SLOT,
            code: &code_v1,
            fuel_max: FUEL_MAX,
            mem_len: MEM_LEN,
        }),
    );
    let second = run_code(
        cluster,
        rv_id,
        sid_v2,
        session::Request::LoadAndActivate(LoadRequest {
            slot: SLOT,
            code: &code_v2,
            fuel_max: FUEL_MAX,
            mem_len: MEM_LEN,
        }),
    );
    let reverted = run_code(
        cluster,
        rv_id,
        sid_revert,
        session::Request::Revert(SlotRequest { slot: SLOT }),
    );

    let first_report = match first {
        Reply::ActivationScheduled(report) => report,
        other => panic!("expected first activation reply, got {other:?}"),
    };
    let second_report = match second {
        Reply::ActivationScheduled(report) => report,
        other => panic!("expected second activation reply, got {other:?}"),
    };
    let reverted_report = match reverted {
        Reply::Reverted(report) => report,
        other => panic!("expected revert reply, got {other:?}"),
    };

    assert_eq!(
        first_report.version, 1,
        "first activation should produce version 1"
    );
    assert_eq!(
        second_report.version, 2,
        "second activation should produce version 2"
    );
    assert_eq!(
        reverted_report.version, 1,
        "revert should restore the previous version"
    );

    assert_eq!(first_report.policy_stats.commits, 1);
    assert_eq!(first_report.policy_stats.rollbacks, 0);
    assert_eq!(second_report.policy_stats.commits, 1);
    assert_eq!(second_report.policy_stats.rollbacks, 0);
    assert_eq!(reverted_report.policy_stats.commits, 0);
    assert_eq!(reverted_report.policy_stats.rollbacks, 1);
    assert_eq!(reverted_report.policy_stats.last_rollback, Some(1));
}

#[test]
fn management_session_keeps_load_and_activate_as_distinct_requests() {
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

    let code = [0x30, 0x03, 0x00, 0x01];
    let loaded = run_code(
        cluster,
        rv_id,
        SessionId::new(60),
        session::Request::Load(LoadRequest {
            slot: SLOT,
            code: &code,
            fuel_max: FUEL_MAX,
            mem_len: MEM_LEN,
        }),
    );
    let activated = run_code(
        cluster,
        rv_id,
        SessionId::new(61),
        session::Request::Activate(SlotRequest { slot: SLOT }),
    );

    let loaded_report = match loaded {
        Reply::Loaded(report) => report,
        other => panic!("expected staged-load reply, got {other:?}"),
    };
    let activated_report = match activated {
        Reply::ActivationScheduled(report) => report,
        other => panic!("expected activation reply, got {other:?}"),
    };

    assert_eq!(loaded_report.staged_version, 1);
    assert_eq!(activated_report.version, loaded_report.staged_version);
    assert_eq!(activated_report.policy_stats.commits, 1);
}
