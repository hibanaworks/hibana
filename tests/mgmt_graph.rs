#![cfg(feature = "std")]

mod common;
#[path = "common/mgmt_loop_support.rs"]
mod mgmt_loop_support;
mod support;

use common::TestTransport;
use hibana::ResourceKind;
use hibana::observe;
use hibana::{
    NoBinding,
    control::{
        cap::{
            CAP_FIXED_HEADER_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN,
            CapShot, GenericCapToken,
            resource_kinds::{LoadBeginKind, LoadCommitKind},
        },
        types::RendezvousId,
    },
    epf::{Slot, verifier::compute_hash},
    g::{self, StepNil},
    observe::{PolicyEventKind, normalise},
    rendezvous::{Lane, Rendezvous, SessionId as RendezvousSessionId},
    runtime::{
        SessionCluster,
        config::Config,
        consts::DefaultLabelUniverse,
        mgmt::{
            Command, LOAD_CHUNK_MAX, LoadBegin, LoadChunk, Reply,
            session::{self, ControllerPlan},
        },
    },
};
use mgmt_loop_support::{register_mgmt_loop_resolvers, reset_mgmt_loop_resolver};
use std::sync::atomic::{AtomicU64, Ordering};
use support::{leak_clock, leak_slab, leak_tap_storage};

type Cluster = SessionCluster<
    'static,
    TestTransport,
    DefaultLabelUniverse,
    hibana::runtime::config::CounterClock,
    4,
>;

const EMPTY_PROGRAM: g::Program<StepNil> = g::Program::empty();
static CHILD_PROGRAM: g::RoleProgram<'static, 0, StepNil> =
    g::project::<0, StepNil, _>(&EMPTY_PROGRAM);

fn slot_to_u8(slot: Slot) -> u8 {
    match slot {
        Slot::Forward => 0,
        Slot::EndpointRx => 1,
        Slot::EndpointTx => 2,
        Slot::Rendezvous => 3,
        Slot::Route => 4,
    }
}

fn next_nonce() -> [u8; CAP_NONCE_LEN] {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let value = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut nonce = [0u8; CAP_NONCE_LEN];
    nonce[..8].copy_from_slice(&value.to_be_bytes());
    nonce[8..16].copy_from_slice(&(!value).to_be_bytes());
    nonce
}

fn base_header(sid: RendezvousSessionId, lane: Lane, role: u8, tag: u8) -> [u8; CAP_HEADER_LEN] {
    let mut header = [0u8; CAP_HEADER_LEN];
    header[..4].copy_from_slice(&sid.raw().to_be_bytes());
    let lane_raw = lane.raw();
    assert!(
        lane_raw <= u32::from(u8::MAX),
        "lane id must fit into u8 for capability token header"
    );
    header[4] = lane_raw as u8;
    header[5] = role;
    header[6] = tag;
    header[7] = CapShot::One.as_u8();
    header
}

fn make_load_begin_token(
    slot: Slot,
    hash: u32,
    sid: RendezvousSessionId,
    lane: Lane,
) -> GenericCapToken<LoadBeginKind> {
    let nonce = next_nonce();
    let mut header = base_header(sid, lane, session::ROLE_CONTROLLER, LoadBeginKind::TAG);
    let handle = (slot_to_u8(slot), u64::from(hash));
    let mask_bits = LoadBeginKind::caps_mask(&handle).bits();
    header[8..10].copy_from_slice(&mask_bits.to_be_bytes());
    let handle_bytes = LoadBeginKind::encode_handle(&handle);
    header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
        .copy_from_slice(&handle_bytes);
    GenericCapToken::from_parts(nonce, header, [0u8; CAP_TAG_LEN])
}

fn make_load_commit_token(
    slot: Slot,
    sid: RendezvousSessionId,
    lane: Lane,
) -> GenericCapToken<LoadCommitKind> {
    let nonce = next_nonce();
    let mut header = base_header(sid, lane, session::ROLE_CONTROLLER, LoadCommitKind::TAG);
    let handle = slot_to_u8(slot);
    let mask_bits = LoadCommitKind::caps_mask(&handle).bits();
    header[8..10].copy_from_slice(&mask_bits.to_be_bytes());
    let handle_bytes = LoadCommitKind::encode_handle(&handle);
    header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
        .copy_from_slice(&handle_bytes);
    GenericCapToken::from_parts(nonce, header, [0u8; CAP_TAG_LEN])
}

fn make_load_begin(slot: Slot, code: &[u8]) -> LoadBegin {
    LoadBegin {
        slot: slot_to_u8(slot),
        code_len: code.len() as u32,
        fuel_max: 64,
        mem_len: 128,
        hash: compute_hash(code),
    }
}

fn make_chunk(offset: u32, code: &[u8], is_last: bool) -> LoadChunk {
    let mut bytes = [0u8; LOAD_CHUNK_MAX];
    if !code.is_empty() {
        bytes[..code.len()].copy_from_slice(code);
    }
    LoadChunk {
        offset,
        len: code.len() as u16,
        is_last,
        bytes,
    }
}

async fn run_mgmt_transaction<'chunks>(
    cluster: &'static Cluster,
    rv_id: RendezvousId,
    sid: RendezvousSessionId,
    lane: Lane,
    plan: ControllerPlan<'chunks>,
) -> Reply {
    let controller_endpoint = cluster
        .attach_cursor::<{ session::ROLE_CONTROLLER }, _, _, _>(
            rv_id,
            sid,
            &session::CONTROLLER_PROGRAM,
            NoBinding,
        )
        .expect("attach management controller cursor");
    let cluster_endpoint = cluster
        .attach_cursor::<{ session::ROLE_CLUSTER }, _, _, _>(rv_id, sid, &session::CLUSTER_PROGRAM, NoBinding)
        .expect("attach management cluster cursor");
    let (controller_result, cluster_result) = tokio::join!(
        async { session::drive_controller(controller_endpoint, plan).await },
        async { cluster.init_mgmt(rv_id, sid, lane, cluster_endpoint).await }
    );

    let controller_cursor = controller_result.expect("management controller drive succeeded");
    drop(controller_cursor);

    let (cluster_cursor, seed) = cluster_result.expect("management cluster init succeeded");
    drop(cluster_cursor);

    cluster
        .drive_mgmt(rv_id, sid, seed)
        .expect("management cluster drive succeeded")
}

#[tokio::test(flavor = "current_thread")]
async fn management_session_emits_commit_and_rollback_taps()
-> Result<(), Box<dyn std::error::Error>> {
    let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));

    // Both rendezvous share the same transport so messages can be delivered between them
    let shared_transport = TestTransport::default();

    let root_rendezvous: Rendezvous<
        '_,
        '_,
        TestTransport,
        DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
    > = Rendezvous::from_config(
        Config::new(leak_tap_storage(), leak_slab(4096)),
        shared_transport.clone(),
    );
    let child_rendezvous: Rendezvous<
        '_,
        '_,
        TestTransport,
        DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
    > = Rendezvous::from_config(
        Config::new(leak_tap_storage(), leak_slab(4096)),
        shared_transport.clone(),
    );

    let root_rv = cluster
        .add_rendezvous(root_rendezvous)
        .expect("register root rendezvous");
    let child_rv = cluster
        .add_rendezvous(child_rendezvous)
        .expect("register child rendezvous");
    register_mgmt_loop_resolvers(cluster, root_rv);

    let sid = RendezvousSessionId::new(42);
    let mgmt_lane = Lane::new(0);

    let _child_endpoint = cluster
        .attach_cursor::<0, _, _, _>(child_rv, sid, &CHILD_PROGRAM, NoBinding)
        .expect("seed management link via child rendezvous");

    let tap = cluster
        .get_local(&root_rv)
        .expect("root rendezvous registered")
        .tap();
    unsafe {
        let _ = observe::install_ring(tap.assume_static());
    }
    let start_head = tap.head();

    const SLOT: Slot = Slot::Rendezvous;

    let code_v1: [u8; 3] = [0x01, 0x02, 0x03];
    let chunk_v1 = make_chunk(0, &code_v1, true);
    let chunks_v1 = [chunk_v1];
    let load_begin_v1 = make_load_begin(SLOT, &code_v1);
    let load_token_v1 = make_load_begin_token(SLOT, load_begin_v1.hash, sid, mgmt_lane);
    let commit_token_v1 = make_load_commit_token(SLOT, sid, mgmt_lane);
    let plan_v1 = ControllerPlan {
        load_token: load_token_v1,
        load_begin: load_begin_v1,
        chunks: &chunks_v1,
        commit_token: commit_token_v1,
        command: Command::Activate { slot: SLOT },
    };

    // Use root_rv for controller (traditional management plane) and child_rv for cluster being managed
    reset_mgmt_loop_resolver(root_rv, sid, mgmt_lane, plan_v1.chunks.len());
    let reply_v1 = run_mgmt_transaction(cluster, root_rv, sid, mgmt_lane, plan_v1).await;
    match reply_v1 {
        Reply::Activated(report) => assert_eq!(report.version, 1),
        other => panic!("expected activation reply, got {:?}", other),
    }

    let code_v2: [u8; 4] = [0xAA, 0xBB, 0xCC, 0xDD];
    let chunk_v2 = make_chunk(0, &code_v2, true);
    let chunks_v2 = [chunk_v2];
    let load_begin_v2 = make_load_begin(SLOT, &code_v2);
    let load_token_v2 = make_load_begin_token(SLOT, load_begin_v2.hash, sid, mgmt_lane);
    let commit_token_v2 = make_load_commit_token(SLOT, sid, mgmt_lane);
    let plan_v2 = ControllerPlan {
        load_token: load_token_v2,
        load_begin: load_begin_v2,
        chunks: &chunks_v2,
        commit_token: commit_token_v2,
        command: Command::Activate { slot: SLOT },
    };

    reset_mgmt_loop_resolver(root_rv, sid, mgmt_lane, plan_v2.chunks.len());
    let reply_v2 = run_mgmt_transaction(cluster, root_rv, sid, mgmt_lane, plan_v2).await;
    match reply_v2 {
        Reply::Activated(report) => assert_eq!(report.version, 2),
        other => panic!("expected second activation reply, got {:?}", other),
    }

    let chunk_revert = make_chunk(0, &[], true);
    let chunks_revert = [chunk_revert];
    let load_begin_revert = make_load_begin(SLOT, &[]);
    let load_token_revert = make_load_begin_token(SLOT, load_begin_revert.hash, sid, mgmt_lane);
    let commit_token_revert = make_load_commit_token(SLOT, sid, mgmt_lane);
    let plan_revert = ControllerPlan {
        load_token: load_token_revert,
        load_begin: load_begin_revert,
        chunks: &chunks_revert,
        commit_token: commit_token_revert,
        command: Command::Revert { slot: SLOT },
    };

    reset_mgmt_loop_resolver(root_rv, sid, mgmt_lane, plan_revert.chunks.len());
    let reply_revert = run_mgmt_transaction(cluster, root_rv, sid, mgmt_lane, plan_revert).await;
    match reply_revert {
        Reply::Reverted(report) => assert_eq!(report.version, 1),
        other => panic!("expected revert reply, got {:?}", other),
    }

    let end_head = tap.head();
    let storage = tap.as_slice();
    let policy_events = normalise::policy_trace(storage, start_head, end_head);
    let (policy_lane, failures) = normalise::policy_lane_trace(storage, start_head, end_head);
    assert!(
        failures.is_empty(),
        "unexpected local action failures during management trace"
    );

    let mut commit_versions = Vec::new();
    let mut rollback_versions = Vec::new();

    for event in policy_events {
        match event.kind {
            PolicyEventKind::Commit => commit_versions.push(event.arg1),
            PolicyEventKind::Rollback => rollback_versions.push(event.arg1),
            _ => {}
        }
    }

    assert!(
        commit_versions.contains(&1),
        "expected commit event for version 1, saw {:?}",
        commit_versions
    );
    assert!(
        commit_versions.contains(&2),
        "expected commit event for version 2, saw {:?}",
        commit_versions
    );
    assert!(
        rollback_versions.contains(&1),
        "expected rollback event for version 1, saw {:?}",
        rollback_versions
    );

    for record in policy_lane.iter().filter(|rec| {
        matches!(
            rec.event.kind,
            PolicyEventKind::Commit | PolicyEventKind::Rollback
        )
    }) {
        if let Some(lane) = record.lane {
            assert_eq!(
                lane,
                mgmt_lane.raw() as u16,
                "unexpected lane marker for management policy event"
            );
        }
        assert!(
            record.lane_matches(),
            "expected lane association (or absence requirement) for {:?}, record: {:?}",
            record.event.kind,
            record
        );
    }

    Ok(())
}
