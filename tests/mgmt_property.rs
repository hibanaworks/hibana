#![cfg(feature = "std")]

use hibana::{
    epf::{
        Slot,
        host::HostSlots,
        verifier::{Header, compute_hash},
    },
    observe::{self, PolicyEvent, PolicyEventKind, TapRing},
    rendezvous::{SLOT_COUNT as RV_SLOT_COUNT, SlotStorage},
    runtime::mgmt::{AwaitBegin, Cold, Manager, PolicySnapshot, TransitionReport},
};
use proptest::prelude::*;

mod support;

const SLOT: Slot = Slot::Rendezvous;
const FUEL_MAX: u16 = 64;
const MEM_LEN: u16 = 128;

fn collect_policy_since(cursor: &mut usize) -> PolicySnapshot {
    let mut snapshot = PolicySnapshot::default();
    observe::for_each_since(cursor, |event| {
        if let Some(policy) = PolicyEvent::from_tap(event) {
            match policy.kind {
                PolicyEventKind::Abort => snapshot.aborts = snapshot.aborts.saturating_add(1),
                PolicyEventKind::Trap => snapshot.traps = snapshot.traps.saturating_add(1),
                PolicyEventKind::Annotate => {
                    snapshot.annotations = snapshot.annotations.saturating_add(1)
                }
                PolicyEventKind::Effect => snapshot.effects = snapshot.effects.saturating_add(1),
                PolicyEventKind::EffectOk => {
                    snapshot.effects_ok = snapshot.effects_ok.saturating_add(1)
                }
                PolicyEventKind::Commit => {
                    snapshot.commits = snapshot.commits.saturating_add(1);
                    snapshot.last_commit = Some(policy.arg1);
                }
                PolicyEventKind::Rollback => {
                    snapshot.rollbacks = snapshot.rollbacks.saturating_add(1);
                    snapshot.last_rollback = Some(policy.arg1);
                }
            }
        }
    });
    snapshot
}

fn install_test_ring() -> (&'static TapRing<'static>, Option<&'static TapRing<'static>>) {
    let storage = support::leak_tap_storage();
    let ring = Box::leak(Box::new(TapRing::from_storage(storage)));
    let previous = unsafe { observe::install_ring(ring.assume_static()) };
    (ring, previous)
}

fn restore_ring(ring: &'static TapRing<'static>, previous: Option<&'static TapRing<'static>>) {
    unsafe {
        let _ = observe::uninstall_ring(ring.as_static_ptr());
    }
    if let Some(prev) = previous {
        let _ = observe::install_ring(prev);
    }
}

fn new_manager() -> Manager<AwaitBegin, { RV_SLOT_COUNT }> {
    Manager::<Cold, { RV_SLOT_COUNT }>::new().into_await_begin()
}

fn stage_and_commit(
    manager: &mut Manager<AwaitBegin, { RV_SLOT_COUNT }>,
    storage: &mut SlotStorage,
    code: &[u8],
) -> u32 {
    let header = Header {
        code_len: code.len() as u16,
        fuel_max: FUEL_MAX,
        mem_len: MEM_LEN,
        flags: 0,
        hash: compute_hash(code),
    };
    manager.load_begin(SLOT, header).expect("load begin");
    manager
        .load_chunk(SLOT, 0, code)
        .expect("load first chunk matches header");
    manager
        .load_commit(SLOT, storage)
        .expect("load commit assigns version")
}

fn install_code<'arena>(
    manager: &mut Manager<AwaitBegin, { RV_SLOT_COUNT }>,
    storage: &'arena mut SlotStorage,
    host_slots: &mut HostSlots<'arena>,
) -> TransitionReport {
    manager
        .activate(SLOT, storage, host_slots)
        .expect("activate installs code")
}

fn revert_code<'arena>(
    manager: &mut Manager<AwaitBegin, { RV_SLOT_COUNT }>,
    storage: &'arena mut SlotStorage,
    host_slots: &mut HostSlots<'arena>,
) -> TransitionReport {
    manager
        .revert(SLOT, storage, host_slots)
        .expect("revert restores previous code")
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 48,
        max_shrink_iters: 1024,
        .. ProptestConfig::default()
    })]

    #[test]
    fn management_policy_snapshots_match_tap_feed(
        plans in prop::collection::vec(
            (prop::collection::vec(any::<u8>(), 1..32), any::<bool>()),
            1..8,
        )
    ) {
        let (ring, previous) = install_test_ring();
        let mut manager = new_manager();
        let mut storage = SlotStorage::new();
        let mut cursor = observe::head().unwrap_or(0);

        let mut current_version: Option<u32> = None;

        for (code, issue_revert) in plans {
            let prior_current = current_version;
            let version = stage_and_commit(&mut manager, &mut storage, &code);
            let report = {
                let mut host_slots = HostSlots::new();

                install_code(&mut manager, &mut storage, &mut host_slots)
            };
            prop_assert_eq!(report.version, version);

            let snapshot = collect_policy_since(&mut cursor);
            prop_assert_eq!(report.policy, snapshot);
            prop_assert_eq!(manager.policy_snapshot(SLOT).unwrap(), snapshot);
            prop_assert_eq!(snapshot.commits, 1);
            prop_assert_eq!(snapshot.rollbacks, 0);
            prop_assert_eq!(snapshot.last_commit, Some(version));
            prop_assert_eq!(snapshot.last_rollback, None);

            current_version = Some(version);

            if issue_revert
                && let Some(expected_version) = prior_current {
                    let revert_report = {
                        let mut host_slots = HostSlots::new();
                        revert_code(&mut manager, &mut storage, &mut host_slots)
                    };
                    prop_assert_eq!(revert_report.version, expected_version);

                    let revert_snapshot = collect_policy_since(&mut cursor);
                    prop_assert_eq!(revert_report.policy, revert_snapshot);
                    prop_assert_eq!(manager.policy_snapshot(SLOT).unwrap(), revert_snapshot);
                    prop_assert_eq!(revert_snapshot.commits, 0);
                    prop_assert_eq!(revert_snapshot.rollbacks, 1);
                    prop_assert_eq!(revert_snapshot.last_commit, None);
                    prop_assert_eq!(revert_snapshot.last_rollback, Some(expected_version));

                    prop_assert!(current_version.is_some());
                    current_version = Some(expected_version);
                }
        }

        restore_ring(ring, previous);
    }
}
