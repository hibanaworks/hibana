#![cfg(feature = "std")]

mod common;
#[path = "common/mgmt_loop_support.rs"]
mod mgmt_loop_support;
mod support;

use common::TestTransport;
use hibana::{
    NoBinding,
    control::{
        cap::{
            CapShot, GenericCapToken, ResourceKind,
            resource_kinds::{LoadBeginKind, LoadCommitKind},
        },
        types::RendezvousId,
    },
    epf::{Slot, ops, verifier::compute_hash},
    g::{self, StepNil},
    observe::{
        self,
        normalise::{mgmt_policy_summary, mgmt_policy_trace},
    },
    rendezvous::{Lane, Rendezvous, SessionId as RendezvousSessionId},
    runtime::{
        SessionCluster,
        config::Config,
        consts::DefaultLabelUniverse,
        mgmt::{
            Command, LOAD_CHUNK_MAX, LoadBegin, LoadChunk, MgmtFacetProfile, Reply,
            session::{self, ControllerPlan},
        },
    },
};
use mgmt_loop_support::{register_mgmt_loop_resolvers, reset_mgmt_loop_resolver};
use std::{
    error::Error,
    sync::atomic::{AtomicU64, Ordering},
};
use support::{leak_clock, leak_slab, leak_tap_storage};

type Cluster = SessionCluster<
    'static,
    TestTransport,
    DefaultLabelUniverse,
    hibana::runtime::config::CounterClock,
    4,
>;

const SLOT: Slot = Slot::Rendezvous;

const EMPTY_PROGRAM: g::Program<StepNil> = g::Program::empty();
static CHILD_PROGRAM: g::RoleProgram<'static, 0, StepNil> =
    g::project::<0, StepNil, _>(&EMPTY_PROGRAM);

#[test]
fn management_session_loads_and_activates_epf_image() {
    // The management automaton exercises deep async call stacks; bump the native stack to avoid
    // overflow when the test is driven with Tokio on a dedicated thread.
    std::thread::Builder::new()
        .name("mgmt-epf-integration".into())
        .stack_size(128 * 1024 * 1024)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            runtime.block_on(async {
                run_mgmt_epf_flow()
                    .await
                    .expect("management session drive completes")
            });
        })
        .expect("spawn mgmt epf thread")
        .join()
        .expect("join mgmt epf thread");
}

async fn run_mgmt_epf_flow() -> Result<(), Box<dyn Error>> {
    let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
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

    let sid = RendezvousSessionId::new(0xCAFE);
    let mgmt_lane = Lane::new(0);

    // Register the session on the child rendezvous so management links detect it.
    let _child_endpoint = cluster
        .attach_cursor::<0, _, _, _>(child_rv, sid, &CHILD_PROGRAM, NoBinding)
        .expect("seed child rendezvous lane");

    let tap = cluster
        .get_local(&root_rv)
        .expect("root rendezvous registered")
        .tap();
    let tap_static = unsafe { tap.assume_static() };
    let previous_ring = observe::install_ring(tap_static);
    let start_head = tap.head();

    register_mgmt_loop_resolvers(cluster, root_rv);

    let controller_plan = build_controller_plan(sid, mgmt_lane);
    reset_mgmt_loop_resolver(root_rv, sid, mgmt_lane, controller_plan.chunks.len());

    let reply = run_mgmt_transaction(cluster, root_rv, sid, mgmt_lane, controller_plan).await;
    let transition = match reply {
        Reply::Activated(report) => report,
        other => panic!("expected activation reply, got {other:?}"),
    };

    let end_head = tap.head();
    let records = mgmt_policy_trace(tap.as_slice(), start_head, end_head);
    assert!(
        !records.is_empty(),
        "management drive should emit commit events"
    );
    let summary = mgmt_policy_summary(&records);
    assert_eq!(
        summary.commits, 1,
        "single activation should commit exactly once"
    );
    assert_eq!(summary.rollbacks, 0, "activation path must not rollback");
    assert_eq!(summary.unmatched_lanes, 0, "lane associations should match");
    assert_eq!(summary.sid_mismatches, 0, "sid associations should match");
    assert!(
        !summary.has_alerts(),
        "clean summary should not raise alerts"
    );

    for record in &records {
        assert!(
            MgmtFacetProfile::supports_policy_kind(record.spec.kind),
            "unexpected management policy kind {:?}",
            record.spec.kind
        );
        assert!(
            MgmtFacetProfile::supports_policy_id(record.spec.id()),
            "unexpected management policy id 0x{:04X}",
            record.spec.id()
        );
    }

    assert_eq!(
        MgmtFacetProfile::facet_needs(),
        MgmtFacetProfile::resource_facets(),
        "facet metadata should align with resource tags"
    );
    assert!(
        MgmtFacetProfile::policy_event_ids()
            .iter()
            .all(|id| MgmtFacetProfile::supports_policy_id(*id)),
        "policy id whitelist should be self-consistent"
    );

    assert_eq!(
        transition.version, 1,
        "first activation should produce version 1"
    );
    assert_eq!(
        transition.policy.commits, 1,
        "activation policy snapshot should record one commit"
    );
    assert_eq!(
        transition.policy.rollbacks, 0,
        "activation policy snapshot should not record rollbacks"
    );
    assert_eq!(
        transition.policy.last_commit,
        Some(transition.version),
        "last_commit should match activated version"
    );
    assert_eq!(
        transition.policy.last_rollback, None,
        "no rollback should be recorded"
    );

    unsafe {
        let _ = observe::uninstall_ring(tap.as_static_ptr());
    }
    if let Some(previous) = previous_ring {
        let _ = observe::install_ring(previous);
    }

    Ok(())
}

fn build_controller_plan(sid: RendezvousSessionId, lane: Lane) -> ControllerPlan<'static> {
    let code = sample_epf_code();
    // Single chunk, so is_last = true
    let chunk = make_chunk(0, &code, true);
    let chunks = Box::leak(Box::new([chunk]));
    let load_begin = make_load_begin(SLOT, &code);
    let load_token = make_load_begin_token(SLOT, load_begin.hash, sid, lane);
    let commit_token = make_load_commit_token(SLOT, sid, lane);

    ControllerPlan {
        load_token,
        load_begin,
        chunks,
        commit_token,
        command: Command::Activate { slot: SLOT },
    }
}

fn sample_epf_code() -> Vec<u8> {
    vec![
        ops::instr::ACT_EFFECT,
        ops::effect::CHECKPOINT,
        0x00,
        ops::instr::HALT,
    ]
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
        session::drive_controller(controller_endpoint, plan),
        cluster.init_mgmt(rv_id, sid, lane, cluster_endpoint)
    );

    let controller_cursor = controller_result.expect("management controller drive succeeded");
    drop(controller_cursor);

    let (cluster_cursor, seed) = cluster_result.expect("management cluster init succeeded");
    drop(cluster_cursor);

    cluster
        .drive_mgmt(rv_id, sid, seed)
        .expect("management cluster drive succeeded")
}

fn slot_to_u8(slot: Slot) -> u8 {
    match slot {
        Slot::Forward => 0,
        Slot::EndpointRx => 1,
        Slot::EndpointTx => 2,
        Slot::Rendezvous => 3,
        Slot::Route => 4,
    }
}

fn next_nonce() -> [u8; hibana::control::cap::CAP_NONCE_LEN] {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let value = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut nonce = [0u8; hibana::control::cap::CAP_NONCE_LEN];
    nonce[..8].copy_from_slice(&value.to_be_bytes());
    nonce[8..16].copy_from_slice(&(!value).to_be_bytes());
    nonce
}

fn base_header(
    sid: RendezvousSessionId,
    lane: Lane,
    role: u8,
    tag: u8,
) -> [u8; hibana::control::cap::CAP_HEADER_LEN] {
    let mut header = [0u8; hibana::control::cap::CAP_HEADER_LEN];
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
    header[hibana::control::cap::CAP_FIXED_HEADER_LEN
        ..hibana::control::cap::CAP_FIXED_HEADER_LEN + hibana::control::cap::CAP_HANDLE_LEN]
        .copy_from_slice(&handle_bytes);
    GenericCapToken::from_parts(nonce, header, [0u8; hibana::control::cap::CAP_TAG_LEN])
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
    header[hibana::control::cap::CAP_FIXED_HEADER_LEN
        ..hibana::control::cap::CAP_FIXED_HEADER_LEN + hibana::control::cap::CAP_HANDLE_LEN]
        .copy_from_slice(&handle_bytes);
    GenericCapToken::from_parts(nonce, header, [0u8; hibana::control::cap::CAP_TAG_LEN])
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
    assert!(
        code.len() <= LOAD_CHUNK_MAX,
        "chunk payload must fit into LOAD_CHUNK_MAX"
    );
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
