use super::*;
use crate::control::types::Lane;

fn cap_table() -> CapTable {
    const CAP_TABLE_SLOTS: usize = 64;
    let mut table = CapTable::empty();
    let storage = std::vec![Option::<CapEntry>::None; CAP_TABLE_SLOTS].into_boxed_slice();
    let ptr = std::boxed::Box::leak(storage).as_mut_ptr().cast::<u8>();
    unsafe {
        // SAFETY: the leaked backing storage is aligned for `CapEntry` slots
        // and remains resident for the duration of the test process.
        table.bind_from_storage(ptr, CAP_TABLE_SLOTS, 0);
    }
    table
}

#[inline]
fn entry(lane: Lane, revision: u64, nonce: [u8; CAP_NONCE_LEN]) -> CapEntry {
    CapEntry::new(lane, revision, nonce)
}

fn find_entry(table: &CapTable, nonce: &[u8; CAP_NONCE_LEN]) -> Option<CapEntry> {
    let slots = table.slots_ptr();
    let mut idx = 0usize;
    while idx < table.capacity {
        let slot = unsafe {
            // SAFETY: test helper scans within the table's bound slot range.
            *slots.add(idx)
        };
        if slot.is_some_and(|entry| entry.nonce == *nonce) {
            return slot;
        }
        idx += 1;
    }
    None
}

#[test]
fn failed_insert_does_not_build_entry() {
    let table = CapTable::empty();
    let lane = Lane::new(1);
    let mut built = false;

    assert!(
        table
            .insert_entry_with(|| {
                built = true;
                entry(lane, 1, [0x01; CAP_NONCE_LEN])
            })
            .is_err()
    );
    assert!(!built, "failed insert must not allocate authority");
}

#[test]
fn release_by_nonce_removes_matching_entry() {
    let table = cap_table();
    let lane = Lane::new(3);
    let nonce = [0xAB; CAP_NONCE_LEN];

    table
        .insert_entry_with(|| entry(lane, 1, nonce))
        .expect("insert succeeds");

    assert!(table.release_by_nonce(&nonce));
    assert!(
        !table.release_by_nonce(&nonce),
        "released authority must not be reusable"
    );
}

#[test]
fn release_by_nonce_at_next_revision_removes_post_snapshot_entry() {
    let table = cap_table();
    let lane = Lane::new(4);
    let nonce = [0xBC; CAP_NONCE_LEN];
    let mut revision_requested = false;

    table
        .insert_entry_with(|| entry(lane, 5, nonce))
        .expect("insert succeeds");

    assert!(
        table.release_by_nonce_at_next_revision(&nonce, lane, 3, || {
            revision_requested = true;
            6
        })
    );
    assert!(revision_requested);
    assert!(
        !table.release_by_nonce(&nonce),
        "post-snapshot release must remove authority immediately"
    );
}

#[test]
fn release_by_nonce_at_next_revision_marks_pre_snapshot_tombstone() {
    let table = cap_table();
    let lane = Lane::new(5);
    let nonce = [0xCD; CAP_NONCE_LEN];
    let mut release_revision = 10;

    table
        .insert_entry_with(|| entry(lane, 2, nonce))
        .expect("insert succeeds");

    assert!(
        table.release_by_nonce_at_next_revision(&nonce, lane, 3, || {
            release_revision += 1;
            release_revision
        })
    );
    assert_eq!(
        find_entry(&table, &nonce).map(|entry| entry.released_revision),
        Some(11),
        "pre-snapshot release must record a tombstone instead of dropping the entry"
    );
}

#[test]
fn restore_lane_to_revision_removes_post_snapshot_entries() {
    let table = cap_table();
    let lane = Lane::new(6);
    let nonce_pre = [0x11; CAP_NONCE_LEN];
    let nonce_post = [0x22; CAP_NONCE_LEN];

    table
        .insert_entry_with(|| entry(lane, 1, nonce_pre))
        .expect("insert succeeds");
    table
        .insert_entry_with(|| entry(lane, 5, nonce_post))
        .expect("insert succeeds");

    table.restore_lane_to_revision(lane, 3);

    assert!(
        !table.release_by_nonce(&nonce_post),
        "restore must discard authority minted after the snapshot"
    );
    assert!(
        table.release_by_nonce(&nonce_pre),
        "restore must preserve authority minted before the snapshot"
    );
}

#[test]
fn restore_lane_to_revision_revives_post_snapshot_release_tombstone() {
    let table = cap_table();
    let lane = Lane::new(7);
    let nonce = [0x33; CAP_NONCE_LEN];

    table
        .insert_entry_with(|| entry(lane, 1, nonce))
        .expect("insert succeeds");
    assert!(table.release_by_nonce_at_next_revision(&nonce, lane, 3, || 5));
    assert_eq!(
        find_entry(&table, &nonce).map(|entry| entry.released_revision),
        Some(5),
        "release before restore must leave a snapshot tombstone"
    );

    table.restore_lane_to_revision(lane, 3);

    assert!(
        table.release_by_nonce(&nonce),
        "restore must revive authority released after the snapshot"
    );
}

#[test]
fn discard_released_lane_entries_purges_tombstones() {
    let table = cap_table();
    let lane = Lane::new(8);
    let nonce = [0x44; CAP_NONCE_LEN];

    table
        .insert_entry_with(|| entry(lane, 1, nonce))
        .expect("insert succeeds");
    assert!(table.release_by_nonce_at_next_revision(&nonce, lane, 3, || 4));

    table.discard_released_lane_entries(lane);

    table.restore_lane_to_revision(lane, 3);
    assert!(
        !table.release_by_nonce(&nonce),
        "discard must make release tombstones permanent"
    );
}
