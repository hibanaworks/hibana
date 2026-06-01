use super::*;
use crate::rendezvous::capability::CapEntry;
#[test]
fn state_restore_rewinds_generation_to_recorded_snapshot() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(7);
        let lane = Lane::new(1);

        rendezvous.assoc.register(lane, sid);
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::ZERO)
            .expect("lane zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::new(1))
            .expect("generation must advance before snapshot");
        let snapshot = publish_state_snapshot(rendezvous, sid, lane);
        assert_eq!(snapshot, Generation::new(1));

        rendezvous
            .r#gen
            .check_and_update(lane, Generation::new(3))
            .expect("generation must advance beyond snapshot");
        assert_eq!(rendezvous.r#gen.last(lane), Some(Generation::new(3)));

        publish_state_restore(rendezvous, sid, lane, snapshot)
            .expect("recorded snapshot must restore lane generation");

        assert_eq!(rendezvous.r#gen.last(lane), Some(snapshot));
        assert_eq!(
            rendezvous.state_snapshots.finalization(lane),
            Some(SnapshotFinalization::Restored)
        );
    });
}

#[test]
fn state_restore_at_lane_targets_the_requested_lane() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(43);
        let lane_a = Lane::new(0);
        let lane_b = Lane::new(1);

        rendezvous.assoc.register(lane_a, sid);
        rendezvous.assoc.register(lane_b, sid);

        rendezvous
            .r#gen
            .check_and_update(lane_a, Generation::ZERO)
            .expect("lane A zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(lane_a, Generation::new(1))
            .expect("lane A generation must advance");
        let snapshot_a = publish_state_snapshot(rendezvous, sid, lane_a);

        rendezvous
            .r#gen
            .check_and_update(lane_b, Generation::ZERO)
            .expect("lane B zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(lane_b, Generation::new(3))
            .expect("lane B generation must advance before snapshot");
        let snapshot_b = publish_state_snapshot(rendezvous, sid, lane_b);
        rendezvous
            .r#gen
            .check_and_update(lane_b, Generation::new(5))
            .expect("lane B generation must advance beyond the snapshot");

        publish_state_restore(rendezvous, sid, lane_b, snapshot_b)
            .expect("state restore must target the requested lane");

        assert_eq!(rendezvous.r#gen.last(lane_a), Some(snapshot_a));
        assert_eq!(rendezvous.r#gen.last(lane_b), Some(snapshot_b));
        assert_eq!(
            rendezvous.state_snapshots.finalization(lane_b),
            Some(SnapshotFinalization::Restored)
        );
    });
}

#[test]
fn state_restore_clears_pending_topology_from_newer_epoch() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(13);
        let lane = Lane::new(1);
        let fences = Some((17, 23));
        let pending_generation = Generation::new(2);

        bind_topology_test_scope(rendezvous, lane)
            .expect("topology tests must bind topology storage");
        rendezvous.assoc.register(lane, sid);
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::ZERO)
            .expect("lane zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::new(1))
            .expect("generation must advance before snapshot");

        let snapshot = publish_state_snapshot(rendezvous, sid, lane);
        let expected_ack = TopologyAck {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(99),
            sid: sid.raw(),
            new_gen: pending_generation,
            src_lane: lane,
            new_lane: Lane::new(2),
            seq_tx: 17,
            seq_rx: 23,
        };
        rendezvous
            .topology_begin(sid, lane, fences, pending_generation, Some(expected_ack))
            .expect("topology begin must stage pending topology state");

        publish_state_restore(rendezvous, sid, lane, snapshot)
            .expect("restore must clear transient topology state recorded after snapshot");

        rendezvous
            .topology_begin(sid, lane, fences, pending_generation, Some(expected_ack))
            .expect("restored lane must accept a fresh topology begin");
    });
}

#[test]
fn prepare_destination_topology_ack_leaves_no_pending_state_on_generation_failure() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(32);
        let lane = Lane::new(1);

        bind_topology_test_scope(rendezvous, lane)
            .expect("topology tests must bind topology storage");
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::ZERO)
            .expect("lane zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::new(1))
            .expect("generation must advance before validating stale intent");

        let stale = TopologyIntent {
            src_rv: RendezvousId::new(7),
            dst_rv: rendezvous.id,
            sid: sid.raw(),
            old_gen: Generation::new(1),
            new_gen: Generation::new(1),
            seq_tx: 11,
            seq_rx: 13,
            src_lane: Lane::new(0),
            dst_lane: lane,
        };
        assert!(matches!(
            rendezvous.prepare_destination_topology_ack(&stale),
            Err(TopologyError::StaleGeneration { lane: err_lane, .. }) if err_lane == lane
        ));

        let valid = TopologyIntent {
            src_rv: RendezvousId::new(7),
            dst_rv: rendezvous.id,
            sid: sid.raw(),
            old_gen: Generation::new(1),
            new_gen: Generation::new(2),
            seq_tx: 11,
            seq_rx: 13,
            src_lane: Lane::new(0),
            dst_lane: lane,
        };
        rendezvous
            .prepare_destination_topology_ack(&valid)
            .expect("stale intent must not leave pending topology wedged on the lane");
    });
}

#[test]
fn prepare_destination_topology_ack_accepts_established_source_generation_on_fresh_destination_lane()
 {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(33);
        let dst_lane = Lane::new(1);

        bind_topology_test_scope(rendezvous, dst_lane)
            .expect("topology tests must bind topology storage");

        let intent = TopologyIntent {
            src_rv: RendezvousId::new(7),
            dst_rv: rendezvous.id,
            sid: sid.raw(),
            old_gen: Generation::new(5),
            new_gen: Generation::new(6),
            seq_tx: 3,
            seq_rx: 7,
            src_lane: Lane::new(0),
            dst_lane: dst_lane,
        };
        let proof = rendezvous
            .prepare_destination_topology_ack(&intent)
            .expect("fresh destination lane must not reject an established source generation");

        assert_eq!(proof.ack(), TopologyAck::from_intent(&intent));
        assert_eq!(
            rendezvous.lane_generation(dst_lane),
            Generation::ZERO,
            "destination prepare must reserve topology state without committing generation",
        );
        assert_eq!(
            rendezvous.preflight_destination_topology_commit(sid, dst_lane),
            Ok(()),
            "destination prepare must stay pending until source commit closes it",
        );
    });
}

#[test]
fn prepare_destination_topology_ack_reports_occupied_destination_lane() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(35);
        let occupying_sid = SessionId::new(36);
        let dst_lane = Lane::new(1);

        bind_topology_test_scope(rendezvous, dst_lane)
            .expect("topology tests must bind topology storage");
        rendezvous.assoc.register(dst_lane, occupying_sid);

        let intent = TopologyIntent {
            src_rv: RendezvousId::new(7),
            dst_rv: rendezvous.id,
            sid: sid.raw(),
            old_gen: Generation::new(5),
            new_gen: Generation::new(6),
            seq_tx: 3,
            seq_rx: 7,
            src_lane: Lane::new(0),
            dst_lane: dst_lane,
        };

        assert!(matches!(
            rendezvous.prepare_destination_topology_ack(&intent),
            Err(TopologyError::LaneMismatch { expected, provided })
                if expected == dst_lane && provided == dst_lane
        ));
    });
}

#[test]
fn topology_ack_mismatch_reports_destination_fields_when_destination_mismatches() {
    let expected = TopologyAck {
        src_rv: RendezvousId::new(1),
        dst_rv: RendezvousId::new(2),
        sid: 7,
        new_gen: Generation::new(3),
        src_lane: Lane::new(4),
        new_lane: Lane::new(5),
        seq_tx: 11,
        seq_rx: 13,
    };

    let mut got = expected;
    got.dst_rv = RendezvousId::new(9);
    assert!(matches!(
        classify_topology_ack_mismatch(expected, got),
        TopologyError::RendezvousIdMismatch {
            expected,
            got
        } if expected == RendezvousId::new(2) && got == RendezvousId::new(9)
    ));

    let mut got = expected;
    got.new_lane = Lane::new(8);
    assert!(matches!(
        classify_topology_ack_mismatch(expected, got),
        TopologyError::LaneMismatch {
            expected,
            provided
        } if expected == Lane::new(5) && provided == Lane::new(8)
    ));

    let mut got = expected;
    got.new_gen = Generation::new(4);
    assert!(matches!(
        classify_topology_ack_mismatch(expected, got),
        TopologyError::StaleGeneration {
            lane,
            last,
            new
        } if lane == Lane::new(5)
            && last == Generation::new(3)
            && new == Generation::new(4)
    ));
}

#[test]
fn state_restore_invalidates_post_snapshot_capability_authority() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(23);
        let lane = Lane::new(1);
        let nonce = [0xA5; crate::control::cap::mint::CAP_NONCE_LEN];

        rendezvous.assoc.register(lane, sid);
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::ZERO)
            .expect("lane zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::new(1))
            .expect("generation must advance before snapshot");
        rendezvous
            .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                0, 0, 0, 1, 0,
            ))
            .expect("capability restore test must bind cap storage");

        let snapshot = publish_state_snapshot(rendezvous, sid, lane);
        rendezvous
            .caps
            .insert_entry(CapEntry::new(lane, rendezvous.next_cap_revision(), nonce))
            .expect("post-snapshot capability entry must be staged");

        publish_state_restore(rendezvous, sid, lane, snapshot)
            .expect("restore must invalidate post-snapshot capability authority");

        assert!(
            !rendezvous.caps.release_by_nonce(&nonce),
            "restore must remove capability entries minted after the snapshot",
        );
    });
}

#[test]
fn state_restore_preserves_pre_snapshot_capability_authority() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(24);
        let lane = Lane::new(1);
        let nonce = [0xB6; crate::control::cap::mint::CAP_NONCE_LEN];

        rendezvous.assoc.register(lane, sid);
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::ZERO)
            .expect("lane zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::new(1))
            .expect("generation must advance before capability mint");
        rendezvous
            .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                0, 0, 0, 1, 0,
            ))
            .expect("capability restore test must bind cap storage");

        rendezvous
            .caps
            .insert_entry(CapEntry::new(lane, rendezvous.next_cap_revision(), nonce))
            .expect("pre-snapshot capability entry must be staged");
        let snapshot = publish_state_snapshot(rendezvous, sid, lane);

        publish_state_restore(rendezvous, sid, lane, snapshot)
            .expect("restore must preserve snapshot-era capability authority");

        assert!(
            rendezvous.caps.release_by_nonce(&nonce),
            "restore must preserve capability entries minted before the snapshot",
        );
    });
}

#[test]
fn state_restore_revives_pre_snapshot_release_authority() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(31);
        let lane = Lane::new(1);
        let nonce = [0xC7; crate::control::cap::mint::CAP_NONCE_LEN];

        rendezvous.assoc.register(lane, sid);
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::ZERO)
            .expect("lane zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::new(1))
            .expect("generation must advance before capability mint");
        rendezvous
            .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                0, 0, 0, 1, 0,
            ))
            .expect("release restore test must bind cap storage");

        rendezvous
            .caps
            .insert_entry(CapEntry::new(lane, rendezvous.next_cap_revision(), nonce))
            .expect("pre-snapshot capability entry must be staged");
        let snapshot = publish_state_snapshot(rendezvous, sid, lane);

        assert_eq!(
            rendezvous.state_snapshots.available_cap_revision(lane),
            Some(1)
        );
        rendezvous.cap_release_ctx(lane).release(&nonce);

        publish_state_restore(rendezvous, sid, lane, snapshot)
            .expect("restore must revive pre-snapshot released authority");

        assert!(
            rendezvous.caps.release_by_nonce(&nonce),
            "restore must clear post-snapshot release tombstones for pre-snapshot entries",
        );
    });
}

#[test]
fn state_snapshot_finalization_rejects_restore_and_commit_replay() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(11);
        let lane = Lane::new(1);

        rendezvous.assoc.register(lane, sid);
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::ZERO)
            .expect("lane zero generation must initialize");

        let snapshot = publish_state_snapshot(rendezvous, sid, lane);
        publish_state_restore(rendezvous, sid, lane, snapshot)
            .expect("first restore must finalize the snapshot");

        assert!(matches!(
            publish_state_restore(rendezvous, sid, lane, snapshot),
            Err(StateRestoreError::AlreadyFinalized { sid: err_sid }) if err_sid == sid
        ));
        assert!(matches!(
            publish_tx_commit(rendezvous, sid, lane, snapshot),
            Err(TxCommitError::AlreadyFinalized { sid: err_sid }) if err_sid == sid
        ));
    });
}
