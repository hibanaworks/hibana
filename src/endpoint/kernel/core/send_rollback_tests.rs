use super::*;
use crate::{
    control::cap::mint::{CAP_NONCE_LEN, CAP_TOKEN_LEN},
    integration::ids::Lane,
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
fn dropping_pending_cap_release_releases_provisional_capability() {
    let lane = Lane::new(3);
    let nonce = [0xAB; CAP_NONCE_LEN];
    let (table, snapshots, revisions, snapshot_storage) = provisional_release_ctx(lane);
    let _ = snapshot_storage.len();

    table
        .insert_entry(CapEntry::new(lane, 1, nonce))
        .expect("insert succeeds");

    drop(PendingCapRelease::new(
        nonce,
        CapReleaseCtx::new(&table, &snapshots, &revisions, lane),
    ));

    assert!(
        !table.release_by_nonce(&nonce),
        "dropping the staged token must release provisional authority"
    );
}

#[test]
fn transferring_pending_cap_release_preserves_provisional_capability() {
    let lane = Lane::new(4);
    let nonce = [0xCD; CAP_NONCE_LEN];
    let (table, snapshots, revisions, snapshot_storage) = provisional_release_ctx(lane);
    let _ = snapshot_storage.len();

    table
        .insert_entry(CapEntry::new(lane, 1, nonce))
        .expect("insert succeeds");

    let registered = PendingCapRelease::new(
        nonce,
        CapReleaseCtx::new(&table, &snapshots, &revisions, lane),
    )
    .into_registered_token([0u8; CAP_TOKEN_LEN]);

    assert!(
        table.release_by_nonce(&nonce),
        "transferred rollback must keep authority live for the registered owner"
    );
    drop(registered);
}
