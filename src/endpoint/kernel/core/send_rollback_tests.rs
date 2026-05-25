use super::*;

use super::{PendingCapRelease, RawEmittedCapToken, StagedDispatchToken};
use crate::{
    control::cap::{
        mint::{CAP_NONCE_LEN, CAP_TOKEN_LEN, CapShot, ResourceKind},
        resource_kinds::{LoopContinueKind, LoopDecisionHandle},
    },
    global::const_dsl::ScopeId,
    integration::ids::{Lane, SessionId},
    rendezvous::{
        capability::{CapEntry, CapReleaseCtx, CapTable},
        tables::StateSnapshotTable,
    },
};
use core::cell::Cell;
use std::vec;

fn cap_table() -> CapTable {
    const CAP_TABLE_SLOTS: usize = 64;
    let mut table = CapTable::empty();
    let storage = vec![Option::<CapEntry>::None; CAP_TABLE_SLOTS].into_boxed_slice();
    let ptr = std::boxed::Box::leak(storage).as_mut_ptr().cast::<u8>();
    unsafe {
        table.bind_from_storage(ptr, CAP_TABLE_SLOTS, 0);
    }
    table
}

fn provisional_release_ctx(
    lane: Lane,
) -> (CapTable, StateSnapshotTable, Cell<u64>, std::vec::Vec<u8>) {
    let table = cap_table();
    let mut snapshot_storage = vec![0u8; StateSnapshotTable::storage_bytes(1)];
    let mut snapshots = StateSnapshotTable::empty();
    unsafe {
        snapshots.bind_from_storage(snapshot_storage.as_mut_ptr(), lane.raw(), 1);
    }
    (table, snapshots, Cell::new(0), snapshot_storage)
}

#[test]
fn dropping_staged_dispatch_token_releases_provisional_capability() {
    let sid = SessionId::new(42);
    let lane = Lane::new(3);
    let role = 0u8;
    let nonce = [0xAB; CAP_NONCE_LEN];
    let handle = LoopDecisionHandle {
        sid: sid.raw(),
        lane: lane.as_wire(),
        scope: ScopeId::loop_scope(2),
    };
    let handle_bytes = LoopContinueKind::encode_handle(&handle);
    let (table, snapshots, revisions, _snapshot_storage) = provisional_release_ctx(lane);

    table
        .insert_entry(CapEntry {
            sid,
            lane_raw: lane.as_wire(),
            kind_tag: LoopContinueKind::TAG,
            shot_state: CapShot::Many.as_u8(),
            role,
            mint_revision: 1,
            consumed_revision: 0,
            released_revision: 0,
            nonce,
            handle: handle_bytes,
        })
        .expect("insert succeeds");

    drop(StagedDispatchToken {
        token: RawEmittedCapToken::new([0u8; CAP_TOKEN_LEN]),
        rollback: PendingCapRelease::new(
            nonce,
            CapReleaseCtx::new(&table, &snapshots, &revisions, lane),
        ),
    });

    assert!(
        table
            .claim_by_nonce(
                &nonce,
                sid,
                lane,
                LoopContinueKind::TAG,
                role,
                CapShot::Many,
                &handle_bytes,
                2,
            )
            .is_err(),
        "dropping the staged token must release provisional authority"
    );
}

#[test]
fn disarming_staged_dispatch_token_preserves_provisional_capability() {
    let sid = SessionId::new(43);
    let lane = Lane::new(4);
    let role = 1u8;
    let nonce = [0xCD; CAP_NONCE_LEN];
    let handle = LoopDecisionHandle {
        sid: sid.raw(),
        lane: lane.as_wire(),
        scope: ScopeId::loop_scope(3),
    };
    let handle_bytes = LoopContinueKind::encode_handle(&handle);
    let (table, snapshots, revisions, _snapshot_storage) = provisional_release_ctx(lane);

    table
        .insert_entry(CapEntry {
            sid,
            lane_raw: lane.as_wire(),
            kind_tag: LoopContinueKind::TAG,
            shot_state: CapShot::Many.as_u8(),
            role,
            mint_revision: 1,
            consumed_revision: 0,
            released_revision: 0,
            nonce,
            handle: handle_bytes,
        })
        .expect("insert succeeds");

    let mut token = StagedDispatchToken {
        token: RawEmittedCapToken::new([0u8; CAP_TOKEN_LEN]),
        rollback: PendingCapRelease::new(
            nonce,
            CapReleaseCtx::new(&table, &snapshots, &revisions, lane),
        ),
    };
    token.rollback.disarm();
    drop(token);

    assert!(
        table
            .claim_by_nonce(
                &nonce,
                sid,
                lane,
                LoopContinueKind::TAG,
                role,
                CapShot::Many,
                &handle_bytes,
                2,
            )
            .is_ok(),
        "disarming rollback must keep authority live for the registered owner"
    );
}

#[test]
fn inert_explicit_dispatch_token_does_not_release_live_capability() {
    let sid = SessionId::new(44);
    let lane = Lane::new(5);
    let role = 0u8;
    let nonce = [0xEF; CAP_NONCE_LEN];
    let handle = LoopDecisionHandle {
        sid: sid.raw(),
        lane: lane.as_wire(),
        scope: ScopeId::loop_scope(4),
    };
    let handle_bytes = LoopContinueKind::encode_handle(&handle);
    let (table, _snapshots, _revisions, _snapshot_storage) = provisional_release_ctx(lane);

    table
        .insert_entry(CapEntry {
            sid,
            lane_raw: lane.as_wire(),
            kind_tag: LoopContinueKind::TAG,
            shot_state: CapShot::Many.as_u8(),
            role,
            mint_revision: 1,
            consumed_revision: 0,
            released_revision: 0,
            nonce,
            handle: handle_bytes,
        })
        .expect("insert succeeds");

    let mut rollback = PendingCapRelease::inert();
    let token = rollback.take_registered_token([0u8; CAP_TOKEN_LEN]);
    assert!(
        token.is_none(),
        "explicit payload tokens must not fabricate registered capability ownership"
    );
    drop(rollback);

    assert!(
        table
            .claim_by_nonce(
                &nonce,
                sid,
                lane,
                LoopContinueKind::TAG,
                role,
                CapShot::Many,
                &handle_bytes,
                2,
            )
            .is_ok(),
        "inert explicit payload rollback must not release unrelated live authority"
    );
}
