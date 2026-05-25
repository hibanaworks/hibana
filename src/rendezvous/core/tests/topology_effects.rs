use super::*;

#[test]
fn init_in_slab_failure_drops_transport_and_clock() {
    reset_drop_counts();
    DROP_TEST_TAP.with(|tap| {
        DROP_TEST_TINY_SLAB.with(|slab| unsafe {
            let tap = &mut *tap.get();
            tap.fill(TapEvent::zero());
            let slab = &mut *slab.get();
            slab.fill(0);
            let config = Config::from_resources((tap, slab), DropClock);
            let rv = DropTestRendezvous::init_in_slab(RendezvousId::new(91), config, DropTransport);
            assert!(
                rv.is_none(),
                "undersized slab must fail public-path rendezvous init"
            );
        });
    });
    assert_eq!(
        drop_counts(),
        (1, 1),
        "failed init_in_slab must drop moved transport and clock exactly once"
    );
}

#[test]
fn init_in_slab_auto_failure_drops_transport_and_clock() {
    reset_drop_counts();
    DROP_TEST_TAP.with(|tap| {
        DROP_TEST_TINY_SLAB.with(|slab| unsafe {
            let tap = &mut *tap.get();
            tap.fill(TapEvent::zero());
            let slab = &mut *slab.get();
            slab.fill(0);
            let config = Config::from_resources((tap, slab), DropClock);
            let rv =
                DropTestRendezvous::init_in_slab_auto(RendezvousId::new(92), config, DropTransport);
            assert!(
                rv.is_none(),
                "undersized slab must fail public-path auto rendezvous init"
            );
        });
    });
    assert_eq!(
        drop_counts(),
        (1, 1),
        "failed init_in_slab_auto must drop moved transport and clock exactly once"
    );
}

#[test]
fn run_effect_allows_when_caps_present() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(2);
        let lane = Lane::new(1);

        let envelope = CpCommand::state_snapshot(SessionId::new(sid.raw()), Lane::new(lane.raw()));

        let result = EffectRunner::run_effect(rendezvous, envelope);

        assert!(matches!(result, Err(CpError::StateSnapshot(_))));
    });
}

#[test]
fn abort_begin_run_effect_respects_associated_lane() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(41);
        let lane_a = Lane::new(0);
        let lane_b = Lane::new(1);

        rendezvous.assoc.register(lane_a, sid);
        rendezvous.assoc.register(lane_b, sid);

        EffectRunner::run_effect(rendezvous, CpCommand::abort_begin(sid, lane_b))
            .expect("abort begin must use the associated lane from the control token");

        let mut cursor = 0usize;
        let events = rendezvous
            .tap()
            .events_since(&mut cursor, |event| {
                (event.id == crate::observe::ids::ABORT_BEGIN).then_some(event)
            })
            .collect::<std::vec::Vec<_>>();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].arg0, sid.raw());
        assert_eq!(events[0].arg1, lane_b.as_wire() as u32);
    });
}

#[test]
fn effect_taps_for_commit_and_tx_abort_carry_lane_causal_keys() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(71);
        let commit_lane = Lane::new(0);
        let abort_lane = Lane::new(1);

        rendezvous.assoc.register(commit_lane, sid);
        rendezvous.assoc.register(abort_lane, sid);
        rendezvous
            .r#gen
            .check_and_update(commit_lane, Generation::ZERO)
            .expect("commit lane zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(commit_lane, Generation::new(1))
            .expect("commit lane generation must advance before snapshot");
        let commit_generation = rendezvous.state_snapshot_at_lane(sid, commit_lane);
        rendezvous
            .tx_commit_at_lane(sid, commit_lane, commit_generation)
            .expect("commit lane should finalize the snapshot");

        rendezvous
            .r#gen
            .check_and_update(abort_lane, Generation::ZERO)
            .expect("abort lane zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(abort_lane, Generation::new(2))
            .expect("abort lane generation must advance before snapshot");
        let abort_generation = rendezvous.state_snapshot_at_lane(sid, abort_lane);
        rendezvous
            .r#gen
            .check_and_update(abort_lane, Generation::new(4))
            .expect("abort lane generation must advance beyond the snapshot");
        rendezvous
            .tx_abort_at_lane(sid, abort_lane, abort_generation)
            .expect("abort lane should restore the snapshot generation");

        let mut cursor = 0usize;
        let events = rendezvous
            .tap()
            .events_since(&mut cursor, |event| match event.id {
                crate::observe::ids::POLICY_COMMIT | crate::observe::ids::POLICY_TX_ABORT => {
                    Some(event)
                }
                _ => None,
            })
            .collect::<std::vec::Vec<_>>();

        assert_eq!(
            events.len(),
            2,
            "expected one commit tap and one tx-abort tap"
        );

        let commit = events
            .iter()
            .find(|event| event.id == crate::observe::ids::POLICY_COMMIT)
            .copied()
            .expect("commit tap");
        assert_eq!(commit.arg0, sid.raw());
        assert_eq!(commit.arg1, commit_generation.0 as u32);
        assert_eq!(
            commit.causal_key,
            TapEvent::make_causal_key(commit_lane.as_wire(), 1),
            "commit tap must encode the originating lane in its causal key"
        );

        let tx_abort = events
            .iter()
            .find(|event| event.id == crate::observe::ids::POLICY_TX_ABORT)
            .copied()
            .expect("tx abort tap");
        assert_eq!(tx_abort.arg0, sid.raw());
        assert_eq!(tx_abort.arg1, abort_generation.0 as u32);
        assert_eq!(
            tx_abort.causal_key,
            TapEvent::make_causal_key(abort_lane.as_wire(), 1),
            "tx-abort tap must encode the originating lane in its causal key"
        );
    });
}

#[test]
fn topology_begin_run_effect_rejects_direct_begin_before_mutation() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(42);
        let src_lane = Lane::new(0);
        let dst_lane = Lane::new(1);

        rendezvous
            .prepare_topology_control_scope(src_lane)
            .expect("topology tests must bind topology storage");
        rendezvous.assoc.register(src_lane, sid);

        let operands = TopologyOperands {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(9),
            src_lane: src_lane,
            dst_lane: dst_lane,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 11,
            seq_rx: 13,
        };
        assert!(matches!(
            EffectRunner::run_effect(rendezvous, CpCommand::topology_begin(sid, operands)),
            Err(CpError::Topology(
                crate::control::cluster::error::TopologyError::InvalidState
            ))
        ));

        rendezvous
            .topology_begin_from_intent(operands.intent(sid))
            .expect(
                "direct topology begin rejection must not wedge the cluster-owned topology path",
            );
    });
}

#[test]
fn topology_begin_run_effect_rejects_internal_lane_split_before_mutation() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(420);
        let associated_lane = Lane::new(0);
        let wrong_lane = Lane::new(1);
        let dst_lane = Lane::new(2);

        rendezvous
            .prepare_topology_control_scope(associated_lane)
            .expect("topology tests must bind topology storage");
        rendezvous
            .prepare_topology_control_scope(wrong_lane)
            .expect("topology tests must bind topology storage");
        rendezvous.assoc.register(associated_lane, sid);

        let operands = TopologyOperands {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(9),
            src_lane: associated_lane,
            dst_lane: dst_lane,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 5,
            seq_rx: 7,
        };
        let malformed = CpCommand::new(ControlOp::TopologyBegin)
            .with_sid(sid)
            .with_lane(wrong_lane)
            .with_topology(operands);

        assert!(matches!(
            EffectRunner::run_effect(rendezvous, malformed),
            Err(CpError::Topology(
                crate::control::cluster::error::TopologyError::LaneMismatch
            ))
        ));

        assert!(matches!(
            EffectRunner::run_effect(rendezvous, CpCommand::topology_begin(sid, operands)),
            Err(CpError::Topology(
                crate::control::cluster::error::TopologyError::InvalidState
            ))
        ));

        rendezvous
            .topology_begin_from_intent(operands.intent(sid))
            .expect("rejected direct begin must not wedge the cluster-owned topology path");
    });
}

#[test]
fn topology_begin_from_intent_rejects_foreign_source_rendezvous_before_mutation() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(421);
        let src_lane = Lane::new(0);
        let dst_lane = Lane::new(1);
        let foreign_src = RendezvousId::new(rendezvous.id.raw().saturating_add(1));

        rendezvous
            .prepare_topology_control_scope(src_lane)
            .expect("topology tests must bind topology storage");
        rendezvous.assoc.register(src_lane, sid);

        let invalid = TopologyOperands {
            src_rv: foreign_src,
            dst_rv: RendezvousId::new(9),
            src_lane: src_lane,
            dst_lane: dst_lane,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 23,
            seq_rx: 29,
        };
        assert!(matches!(
            rendezvous.topology_begin_from_intent(invalid.intent(sid)),
            Err(TopologyError::RendezvousIdMismatch { expected, got })
                if expected == foreign_src && got == rendezvous.id
        ));

        let valid = TopologyOperands {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(9),
            src_lane: src_lane,
            dst_lane: dst_lane,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 23,
            seq_rx: 29,
        };
        rendezvous
            .topology_begin_from_intent(valid.intent(sid))
            .expect("failed begin preflight must not wedge the topology intent path");
    });
}

#[test]
fn topology_begin_from_intent_rejects_stale_source_generation_before_mutation() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(422);
        let src_lane = Lane::new(0);
        let dst_lane = Lane::new(1);

        rendezvous
            .prepare_topology_control_scope(src_lane)
            .expect("topology tests must bind topology storage");
        rendezvous.assoc.register(src_lane, sid);
        rendezvous.advance_lane_generation_to(src_lane, Generation::new(1));

        let stale = TopologyOperands {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(10),
            src_lane: src_lane,
            dst_lane: dst_lane,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(2),
            seq_tx: 31,
            seq_rx: 37,
        };
        assert!(matches!(
            rendezvous.topology_begin_from_intent(stale.intent(sid)),
            Err(TopologyError::StaleGeneration { lane, last, new })
                if lane == src_lane
                    && last == Generation::new(1)
                    && new == Generation::new(2)
        ));

        let valid = TopologyOperands {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(10),
            src_lane: src_lane,
            dst_lane: dst_lane,
            old_gen: Generation::new(1),
            new_gen: Generation::new(2),
            seq_tx: 31,
            seq_rx: 37,
        };
        rendezvous
            .topology_begin_from_intent(valid.intent(sid))
            .expect("stale rejection must leave the topology intent path reusable");
    });
}

#[test]
fn topology_begin_run_effect_rejects_operandless_command() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(423);
        let lane = Lane::new(0);

        assert_eq!(
            EffectRunner::run_effect(
                rendezvous,
                CpCommand::new(ControlOp::TopologyBegin)
                    .with_sid(sid)
                    .with_lane(lane)
                    .with_generation(Generation::new(1)),
            ),
            Err(CpError::Topology(
                crate::control::cluster::error::TopologyError::InvalidState,
            ))
        );
    });
}

#[test]
fn topology_begin_from_intent_rejects_unassociated_source_lane() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(43);
        let associated_lane = Lane::new(0);
        let wrong_lane = Lane::new(1);
        let dst_lane = Lane::new(2);

        rendezvous
            .prepare_topology_control_scope(associated_lane)
            .expect("topology tests must bind topology storage");
        rendezvous
            .prepare_topology_control_scope(wrong_lane)
            .expect("topology tests must bind topology storage");
        rendezvous.assoc.register(associated_lane, sid);

        let invalid = TopologyIntent {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(7),
            sid: sid.raw(),
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 17,
            seq_rx: 19,
            src_lane: wrong_lane,
            dst_lane: dst_lane,
        };
        assert!(matches!(
            rendezvous.topology_begin_from_intent(invalid),
            Err(TopologyError::UnknownSession { sid: err_sid }) if err_sid == sid
        ));

        let valid = TopologyIntent {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(7),
            sid: sid.raw(),
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 17,
            seq_rx: 19,
            src_lane: associated_lane,
            dst_lane: dst_lane,
        };
        rendezvous
            .topology_begin_from_intent(valid)
            .expect("associated lane must remain usable after rejected begin intent");
    });
}

#[test]
fn topology_begin_rejects_duplicate_pending_session_across_lanes() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(45);
        let lane_a = Lane::new(0);
        let lane_b = Lane::new(1);
        let dst_a = Lane::new(2);
        let dst_b = Lane::new(3);
        let first = TopologyOperands {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(9),
            src_lane: lane_a,
            dst_lane: dst_a,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 11,
            seq_rx: 13,
        };
        let second = TopologyOperands {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(10),
            src_lane: lane_b,
            dst_lane: dst_b,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 17,
            seq_rx: 19,
        };

        rendezvous
            .prepare_topology_control_scope(lane_a)
            .expect("topology tests must bind topology storage");
        rendezvous
            .prepare_topology_control_scope(lane_b)
            .expect("topology tests must bind topology storage");
        rendezvous.assoc.register(lane_a, sid);
        rendezvous.assoc.register(lane_b, sid);

        rendezvous
            .topology_begin(
                sid,
                lane_a,
                Some((first.seq_tx, first.seq_rx)),
                first.new_gen,
                Some(first.ack(sid)),
            )
            .expect("first begin must succeed");

        assert_eq!(
            rendezvous.topology_begin(
                sid,
                lane_b,
                Some((second.seq_tx, second.seq_rx)),
                second.new_gen,
                Some(second.ack(sid)),
            ),
            Err(TopologyError::InProgress { lane: lane_a })
        );
        assert_eq!(
            rendezvous.expected_topology_ack(sid),
            Ok(first.ack(sid)),
            "duplicate begin rejection must keep the canonical expected ACK bound to the first lane"
        );
        assert_eq!(
            rendezvous.validate_topology_commit_operands(sid, second),
            Err(TopologyError::RendezvousIdMismatch {
                expected: first.dst_rv,
                got: second.dst_rv,
            }),
            "duplicate begin rejection must keep commit validation bound to the first pending topology"
        );
        assert!(matches!(
            EffectRunner::run_effect(rendezvous, CpCommand::topology_commit(sid, second)),
            Err(CpError::Topology(_))
        ));
        assert_eq!(
            rendezvous.expected_topology_ack(sid),
            Ok(first.ack(sid)),
            "rejected commit through the production effect path must preserve the first pending topology"
        );
    });
}

#[test]
fn topology_commit_run_effect_is_cluster_owned_and_preserves_pending_state() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(46);
        let src_lane = Lane::new(0);
        let dst_lane = Lane::new(1);
        let expected = TopologyOperands {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(11),
            src_lane: src_lane,
            dst_lane: dst_lane,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 41,
            seq_rx: 43,
        };

        rendezvous
            .prepare_topology_control_scope(src_lane)
            .expect("topology tests must bind topology storage");
        rendezvous.assoc.register(src_lane, sid);
        rendezvous
            .topology_begin_from_intent(expected.intent(sid))
            .expect("begin effect");

        assert!(matches!(
            EffectRunner::run_effect(rendezvous, CpCommand::topology_commit(sid, expected)),
            Err(CpError::Topology(
                crate::control::cluster::error::TopologyError::InvalidState
            ))
        ));
        assert_eq!(
            rendezvous.expected_topology_ack(sid),
            Ok(expected.ack(sid)),
            "direct commit rejection must preserve the source-side expected ACK"
        );
        assert_eq!(
            rendezvous.session_lane(sid),
            Some(src_lane),
            "direct commit rejection must not retire the associated source lane"
        );
    });
}

#[test]
fn topology_commit_run_effect_rejects_operandless_command_before_mutation() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(47);
        let src_lane = Lane::new(0);
        let dst_lane = Lane::new(1);
        let expected = TopologyOperands {
            src_rv: rendezvous.id,
            dst_rv: RendezvousId::new(12),
            src_lane: src_lane,
            dst_lane: dst_lane,
            old_gen: Generation::ZERO,
            new_gen: Generation::new(1),
            seq_tx: 47,
            seq_rx: 53,
        };

        rendezvous
            .prepare_topology_control_scope(src_lane)
            .expect("topology tests must bind topology storage");
        rendezvous.assoc.register(src_lane, sid);
        rendezvous
            .topology_begin_from_intent(expected.intent(sid))
            .expect("begin effect");

        assert_eq!(
            EffectRunner::run_effect(
                rendezvous,
                CpCommand::new(ControlOp::TopologyCommit)
                    .with_sid(sid)
                    .with_lane(src_lane),
            ),
            Err(CpError::Topology(
                crate::control::cluster::error::TopologyError::InvalidState,
            ))
        );
        assert_eq!(
            rendezvous.expected_topology_ack(sid),
            Ok(expected.ack(sid)),
            "operand-less direct commit rejection must preserve the canonical expected ACK",
        );
        assert_eq!(
            rendezvous.session_lane(sid),
            Some(src_lane),
            "operand-less direct commit rejection must not retire the associated source lane",
        );
    });
}

#[test]
fn state_snapshot_run_effect_respects_associated_lane() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(44);
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
        rendezvous
            .r#gen
            .check_and_update(lane_b, Generation::ZERO)
            .expect("lane B zero generation must initialize");
        rendezvous
            .r#gen
            .check_and_update(lane_b, Generation::new(3))
            .expect("lane B generation must advance");

        EffectRunner::run_effect(rendezvous, CpCommand::state_snapshot(sid, lane_b))
            .expect("state snapshot must target the lane associated with the token");

        assert_eq!(rendezvous.state_snapshots.last_snapshot(lane_a), None);
        assert_eq!(
            rendezvous.state_snapshots.last_snapshot(lane_b),
            Some(Generation::new(3))
        );
    });
}
