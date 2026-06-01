use super::*;
#[test]
fn topology_ack_emits_registered_tap_event() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(19);
        let lane = Lane::new(1);
        bind_topology_test_scope(rendezvous, lane)
            .expect("topology ack test must bind topology storage");
        let operands = TopologyOperands {
            src_rv: RendezvousId::new(9),
            dst_rv: rendezvous.id,
            src_lane: lane,
            dst_lane: lane,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(2),
            seq_tx: 31,
            seq_rx: 37,
        };
        assert_eq!(
            rendezvous.preflight_destination_topology_commit(sid, lane),
            Err(TopologyError::NoPending { lane }),
            "destination topology starts unstaged before the cluster-owned ack helper runs",
        );

        rendezvous
            .acknowledge_topology_intent(&operands.intent(sid))
            .expect("cluster-owned topology ack helper must stage destination prepare");
        assert!(
            !rendezvous.is_session_registered(sid),
            "destination ack must stage the topology change without making the destination session live",
        );
        assert_eq!(
            rendezvous.preflight_destination_topology_commit(sid, lane),
            Ok(()),
            "ack must leave destination topology pending until the source commit finalizes it",
        );

        let mut cursor = 0usize;
        let events = rendezvous
            .tap()
            .events_since(&mut cursor, |event| {
                (event.id == crate::observe::ids::TOPOLOGY_ACK).then_some(event)
            })
            .collect::<std::vec::Vec<_>>();

        assert_eq!(
            events.len(),
            1,
            "ack path must emit exactly one topology ack tap"
        );
        let event = events[0];
        let expected = ((operands.src_lane.as_wire() as u32) & 0xFF)
            | (((operands.dst_lane.as_wire() as u32) & 0xFF) << 8)
            | ((operands.new_gen.0 as u32) << 16);
        assert_eq!(event.arg0, expected);
        assert_eq!(event.arg1, sid.raw());
    });
}

#[test]
fn abort_topology_state_clears_destination_prepare_explicitly() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(34);
        let lane = Lane::new(1);

        bind_topology_test_scope(rendezvous, lane)
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
            dst_lane: lane,
        };
        rendezvous
            .prepare_destination_topology_ack(&intent)
            .expect("destination prepare must succeed before explicit abort");
        assert_eq!(
            rendezvous.lane_generation(lane),
            Generation::ZERO,
            "destination prepare must not advance generation before commit",
        );
        assert_eq!(
            rendezvous.preflight_destination_topology_commit(sid, lane),
            Ok(()),
            "destination prepare must be pending before explicit abort",
        );

        assert_eq!(
            rendezvous.abort_topology_state(sid),
            true,
            "explicit abort must clear destination-only prepared topology",
        );
        assert_eq!(
            rendezvous.preflight_destination_topology_commit(sid, lane),
            Err(TopologyError::NoPending { lane }),
            "explicit abort must remove destination pending topology state",
        );
        assert_eq!(
            rendezvous.r#gen.last(lane),
            None,
            "explicit abort must keep a fresh destination lane at its pre-ack generation state",
        );
    });
}

#[test]
fn destination_topology_commit_rejects_stale_prepared_generation_base() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(37);
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
            .expect("generation must advance before destination prepare");

        let intent = TopologyIntent {
            src_rv: RendezvousId::new(7),
            dst_rv: rendezvous.id,
            sid: sid.raw(),
            old_gen: Generation::new(1),
            new_gen: Generation::new(3),
            seq_tx: 3,
            seq_rx: 7,
            src_lane: Lane::new(0),
            dst_lane: lane,
        };
        rendezvous
            .prepare_destination_topology_ack(&intent)
            .expect("destination prepare must record its generation base");
        let proof = rendezvous
            .reserve_destination_topology_commit(sid, lane)
            .expect("destination commit reservation must succeed before publish");

        rendezvous.publish_prepared_destination_topology_commit(proof, lane);
        assert_eq!(
            rendezvous.r#gen.last(lane),
            Some(intent.new_gen),
            "prepared destination commit must publish the reserved generation directly"
        );
        assert_eq!(
            rendezvous.topology.attach_ready_sid(lane),
            Some(sid),
            "prepared destination commit must leave the destination lane attach-ready"
        );
    });
}

#[test]
fn abort_destination_prepare_does_not_rewind_unowned_generation_change() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(38);
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
            .expect("generation must advance before destination prepare");

        let intent = TopologyIntent {
            src_rv: RendezvousId::new(7),
            dst_rv: rendezvous.id,
            sid: sid.raw(),
            old_gen: Generation::new(1),
            new_gen: Generation::new(3),
            seq_tx: 3,
            seq_rx: 7,
            src_lane: Lane::new(0),
            dst_lane: lane,
        };
        rendezvous
            .prepare_destination_topology_ack(&intent)
            .expect("destination prepare must record its generation base");
        rendezvous
            .r#gen
            .check_and_update(lane, Generation::new(2))
            .expect("test interleaving must advance generation outside the prepared lease");

        assert_eq!(
            rendezvous.abort_topology_state(sid),
            true,
            "explicit abort must clear destination-only prepared topology"
        );
        assert_eq!(
            rendezvous.preflight_destination_topology_commit(sid, lane),
            Err(TopologyError::NoPending { lane }),
            "abort must remove destination pending topology state"
        );
        assert_eq!(
            rendezvous.r#gen.last(lane),
            Some(Generation::new(2)),
            "destination prepare abort must not rewind a generation it never committed"
        );
    });
}

#[test]
fn route_table_capacity_stays_tied_to_lane_frame_depth() {
    with_image_test_rendezvous(|rendezvous| {
        rendezvous
            .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                2, 3, 0, 0, 0,
            ))
            .expect("route resident budget should bind route storage");
        assert_eq!(
            rendezvous.routes.route_slots(),
            2,
            "route ledger lane-frame storage must stay tied to route depth"
        );
        assert_eq!(
            rendezvous.routes.lane_slots(),
            3,
            "route ledger lane storage must stay tied to the live lane span"
        );
    });
}

#[test]
fn topology_table_binds_only_for_topology_control_scope() {
    with_image_test_rendezvous(|rendezvous| {
        assert!(!rendezvous.topology.is_bound());

        rendezvous.initialise_control_scope(Lane::new(0), ControlScopeKind::Loop);
        assert!(
            !rendezvous.topology.is_bound(),
            "non-topology control scopes must not bind topology storage"
        );

        bind_topology_test_scope(rendezvous, Lane::new(0))
            .expect("topology control scope should bind topology storage");
        assert!(rendezvous.topology.is_bound());
    });
}

#[test]
fn lane_lifecycle_clears_dynamic_policy_state() {
    with_epf_test_rendezvous(|rendezvous| {
        let lane = Lane::new(1);
        let sid = SessionId::new(29);
        let eff_index = EffIndex::from_dense_ordinal(11);
        let tag = 7;
        let policy = PolicyMode::dynamic(3);

        rendezvous
            .register_policy(lane, eff_index, tag, policy)
            .expect("dynamic policy registration must bind policy storage");
        assert_eq!(rendezvous.policy(lane, eff_index, tag), Some(policy));

        rendezvous
            .activate_lane_attachment(sid, lane)
            .expect("first attach must clear stale policy state before opening the lane");
        assert_eq!(
            rendezvous.policy(lane, eff_index, tag),
            None,
            "first attach must clear stale lane policy state",
        );

        rendezvous
            .register_policy(lane, eff_index, tag, policy)
            .expect("policy state should remain writable after attach");
        assert_eq!(rendezvous.release_lane(lane), Some(sid));
        assert_eq!(
            rendezvous.policy(lane, eff_index, tag),
            None,
            "lane release must own dynamic policy cleanup",
        );
    });
}

#[test]
fn state_restore_preserves_live_session_policy_image() {
    with_epf_test_rendezvous(|rendezvous| {
        let lane = Lane::new(1);
        let sid = SessionId::new(30);
        let eff_index = EffIndex::from_dense_ordinal(12);
        let tag = 9;
        let policy = PolicyMode::dynamic(7);

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
            .register_policy(lane, eff_index, tag, policy)
            .expect("policy image should be writable before snapshot");

        let snapshot = publish_state_snapshot(rendezvous, sid, lane);
        publish_state_restore(rendezvous, sid, lane, snapshot)
            .expect("restore should not clear the live session policy image");

        assert_eq!(
            rendezvous.policy(lane, eff_index, tag),
            Some(policy),
            "restore must preserve the session policy image for the live lane",
        );
    });
}

#[test]
#[should_panic(expected = "capability nonce counter exhausted")]
fn next_nonce_seed_panics_on_overflow() {
    with_epf_test_rendezvous(|rendezvous| {
        rendezvous.cap_nonce.set(u64::MAX);
        let _ = rendezvous.next_nonce_seed();
    });
}

#[test]
fn trim_resident_headers_reclaims_frontier_when_no_images_remain_above_sidecars() {
    with_image_test_rendezvous(|rendezvous| {
        let initial_frontier = rendezvous.image_frontier;
        rendezvous
            .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                2, 3, 3, 8, 0,
            ))
            .expect("resident sidecars should bind");
        assert!(
            rendezvous.image_frontier > initial_frontier,
            "resident sidecars must consume persistent bytes before trimming"
        );

        rendezvous.trim_resident_headers_to_live_budget();

        assert_eq!(
            rendezvous.image_frontier, initial_frontier,
            "trimming empty resident headers must return the frontier when nothing remains above them"
        );
        assert_eq!(rendezvous.routes.route_slots(), 0);
        assert_eq!(rendezvous.loops.loop_slots(), 0);
        assert_eq!(rendezvous.caps.capacity(), 0);
    });
}

#[test]
fn external_sidecar_free_reclaims_frontier_alignment_padding() {
    with_image_test_rendezvous(|rendezvous| {
        rendezvous.free_regions = [FreeRegion::EMPTY; FREE_REGION_CAPACITY];
        let initial_frontier = rendezvous.image_frontier;
        let align = core::mem::align_of::<u128>();
        let head_bytes = if (initial_frontier as usize + 1) % align == 0 {
            2
        } else {
            1
        };

        let (head_ptr, head_reclaim_delta) = rendezvous
            .allocate_external_persistent_sidecar_bytes(head_bytes, 1)
            .expect("unaligned external sidecar should bind");
        let frontier_after_head = rendezvous.image_frontier;

        let (aligned_ptr, aligned_reclaim_delta) = rendezvous
            .allocate_external_persistent_sidecar_bytes(8, align)
            .expect("aligned external sidecar should bind");
        assert!(
            aligned_reclaim_delta > 0,
            "aligned external sidecar must record reclaimed prefix padding when frontier is unaligned"
        );

        rendezvous.free_external_persistent_sidecar_bytes(aligned_ptr, 8, aligned_reclaim_delta);
        assert_eq!(
            rendezvous.image_frontier, frontier_after_head,
            "freeing the top external sidecar must reclaim its alignment padding back to the previous frontier"
        );

        rendezvous.free_external_persistent_sidecar_bytes(head_ptr, head_bytes, head_reclaim_delta);
        assert_eq!(
            rendezvous.image_frontier, initial_frontier,
            "freeing all external sidecars must return the frontier to its starting point"
        );
    });
}
