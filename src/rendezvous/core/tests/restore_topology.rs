use super::*;

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
        let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);
        assert_eq!(snapshot, Generation::new(1));

        rendezvous
            .r#gen
            .check_and_update(lane, Generation::new(3))
            .expect("generation must advance beyond snapshot");
        assert_eq!(rendezvous.r#gen.last(lane), Some(Generation::new(3)));

        rendezvous
            .state_restore_at_lane(sid, lane, snapshot)
            .expect("recorded snapshot must restore lane generation");

        assert_eq!(rendezvous.r#gen.last(lane), Some(snapshot));
        assert_eq!(
            rendezvous.state_snapshots.finalization(lane),
            Some(SnapshotFinalization::Restored)
        );
    });
}

#[test]
fn state_restore_run_effect_respects_associated_lane() {
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
        let snapshot_a = rendezvous.state_snapshot_at_lane(sid, lane_a);

        rendezvous
            .r#gen
            .check_and_update(lane_b, Generation::ZERO)
            .expect("lane B zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(lane_b, Generation::new(3))
            .expect("lane B generation must advance before snapshot");
        let snapshot_b = rendezvous.state_snapshot_at_lane(sid, lane_b);
        rendezvous
            .r#gen
            .check_and_update(lane_b, Generation::new(5))
            .expect("lane B generation must advance beyond the snapshot");

        EffectRunner::run_effect(
            rendezvous,
            CpCommand::state_restore(sid, lane_b, snapshot_b),
        )
        .expect("state restore must target the lane associated with the token");

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

        rendezvous
            .prepare_topology_control_scope(lane)
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

        let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);
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

        rendezvous
            .state_restore_at_lane(sid, lane, snapshot)
            .expect("restore must clear transient topology state recorded after snapshot");

        rendezvous
            .topology_begin(sid, lane, fences, pending_generation, Some(expected_ack))
            .expect("restored lane must accept a fresh topology begin");
    });
}

#[test]
fn process_topology_intent_leaves_no_pending_state_on_generation_failure() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(32);
        let lane = Lane::new(1);

        rendezvous
            .prepare_topology_control_scope(lane)
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
            rendezvous.process_topology_intent(&stale),
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
            .process_topology_intent(&valid)
            .expect("stale intent must not leave pending topology wedged on the lane");
    });
}

#[test]
fn process_topology_intent_accepts_established_source_generation_on_fresh_destination_lane() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(33);
        let dst_lane = Lane::new(1);

        rendezvous
            .prepare_topology_control_scope(dst_lane)
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
        let ack = rendezvous
            .process_topology_intent(&intent)
            .expect("fresh destination lane must not reject an established source generation");

        assert_eq!(ack, TopologyAck::from_intent(&intent));
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
fn process_topology_intent_reports_occupied_destination_lane() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(35);
        let occupying_sid = SessionId::new(36);
        let dst_lane = Lane::new(1);

        rendezvous
            .prepare_topology_control_scope(dst_lane)
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
            rendezvous.process_topology_intent(&intent),
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
        let role = 5;
        let nonce = [0xA5; crate::control::cap::mint::CAP_NONCE_LEN];
        let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);
        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];

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

        let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);
        rendezvous
            .mint_cap::<crate::control::cap::mint::EndpointResource>(
                sid,
                lane,
                crate::control::cap::mint::CapShot::One,
                role,
                nonce,
                handle,
            )
            .expect("capability mint before snapshot must succeed");

        crate::control::cap::mint::CapHeader::new(
            sid,
            lane,
            role,
            crate::control::cap::mint::EndpointResource::TAG,
            ControlOp::Fence,
            crate::control::cap::mint::ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            crate::control::cap::mint::EndpointResource::encode_handle(&handle),
        )
        .encode(&mut header);
        let token = endpoint_cap_token_from_wire(nonce, header);

        rendezvous
            .state_restore_at_lane(sid, lane, snapshot)
            .expect("restore must invalidate post-snapshot capability authority");

        assert!(
            matches!(rendezvous.claim_cap(&token), Err(CapError::UnknownToken)),
            "restore must not leave post-snapshot capability authority claimable",
        );
    });
}

#[test]
fn state_restore_preserves_pre_snapshot_capability_authority() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(24);
        let lane = Lane::new(1);
        let role = 6;
        let nonce = [0xB6; crate::control::cap::mint::CAP_NONCE_LEN];
        let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);
        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];

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
            .mint_cap::<crate::control::cap::mint::EndpointResource>(
                sid,
                lane,
                crate::control::cap::mint::CapShot::One,
                role,
                nonce,
                handle,
            )
            .expect("capability mint before snapshot must succeed");
        let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);
        let handle_bytes = crate::control::cap::mint::EndpointResource::encode_handle(&handle);

        crate::control::cap::mint::CapHeader::new(
            sid,
            lane,
            role,
            crate::control::cap::mint::EndpointResource::TAG,
            ControlOp::Fence,
            crate::control::cap::mint::ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            handle_bytes,
        )
        .encode(&mut header);
        let token = endpoint_cap_token_from_wire(nonce, header);

        rendezvous
            .claim_cap(&token)
            .expect("pre-snapshot one-shot token must be claimable before restore");

        rendezvous
            .state_restore_at_lane(sid, lane, snapshot)
            .expect("restore must preserve snapshot-era capability authority");

        rendezvous
            .claim_cap(&token)
            .expect("restore must revive the snapshot-era one-shot capability state");
    });
}

#[test]
fn state_restore_revives_pre_snapshot_release_authority() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(31);
        let lane = Lane::new(1);
        let role = 7;
        let nonce = [0xC7; crate::control::cap::mint::CAP_NONCE_LEN];
        let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);
        let handle_bytes = crate::control::cap::mint::EndpointResource::encode_handle(&handle);
        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];

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
            .mint_cap::<crate::control::cap::mint::EndpointResource>(
                sid,
                lane,
                crate::control::cap::mint::CapShot::One,
                role,
                nonce,
                handle,
            )
            .expect("capability mint before snapshot must succeed");
        let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);

        crate::control::cap::mint::CapHeader::new(
            sid,
            lane,
            role,
            crate::control::cap::mint::EndpointResource::TAG,
            ControlOp::Fence,
            crate::control::cap::mint::ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            handle_bytes,
        )
        .encode(&mut header);
        let token = endpoint_cap_token_from_wire(nonce, header);

        assert_eq!(
            rendezvous.state_snapshots.available_cap_revision(lane),
            Some(1)
        );
        rendezvous.cap_release_ctx(lane).release(&nonce);

        assert!(
            matches!(
                rendezvous.caps.claim_by_nonce(
                    &nonce,
                    sid,
                    lane,
                    crate::control::cap::mint::EndpointResource::TAG,
                    role,
                    crate::control::cap::mint::CapShot::One,
                    &handle_bytes,
                    0,
                ),
                Err(CapError::UnknownToken)
            ),
            "release must hide authority in the capability table before restore",
        );
        assert!(
            matches!(rendezvous.claim_cap(&token), Err(CapError::UnknownToken)),
            "snapshot-aware release after snapshot must hide authority until restore",
        );

        rendezvous
            .state_restore_at_lane(sid, lane, snapshot)
            .expect("restore must revive pre-snapshot released authority");

        rendezvous
            .claim_cap(&token)
            .expect("restore must recreate pre-snapshot authority removed after snapshot");
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

        let snapshot = rendezvous.state_snapshot_at_lane(sid, lane);
        rendezvous
            .state_restore_at_lane(sid, lane, snapshot)
            .expect("first restore must finalize the snapshot");

        assert!(matches!(
            rendezvous.state_restore_at_lane(sid, lane, snapshot),
            Err(StateRestoreError::AlreadyFinalized { sid: err_sid }) if err_sid == sid
        ));
        assert!(matches!(
            rendezvous.tx_commit_at_lane(sid, lane, snapshot),
            Err(TxCommitError::AlreadyFinalized { sid: err_sid }) if err_sid == sid
        ));
    });
}
