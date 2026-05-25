use super::super::*;

#[test]
fn topology_begin_and_ack_execute_without_hidden_dispatch() {
    run_on_transient_compiled_test_stack(
        "topology_begin_and_ack_execute_without_hidden_dispatch",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(7);
                    let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                    let dst_lane = Lane::new(1);
                    let operands = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: src_lane,
                        dst_lane: dst_lane,
                        old_gen: Generation::new(0),
                        new_gen: Generation::new(1),
                        seq_tx: 0,
                        seq_rx: 0,
                    };

                    let pending = cluster
                        .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                        .expect("begin effect");
                    assert!(matches!(pending, PendingEffect::None));

                    let handle = TopologyHandle {
                        src_rv: src_id.raw(),
                        dst_rv: dst_id.raw(),
                        src_lane: src_lane.raw() as u16,
                        dst_lane: dst_lane.raw() as u16,
                        old_gen: operands.old_gen.raw(),
                        new_gen: operands.new_gen.raw(),
                        seq_tx: operands.seq_tx,
                        seq_rx: operands.seq_rx,
                    };
                    let decoded =
                        TopologyHandle::decode(handle.encode()).expect("decode topology handle");

                    cluster
                        .dispatch_topology_ack_with_handle(dst_id, sid, dst_lane, decoded, None)
                        .expect("dispatch succeeds");

                    let sid_fail = SessionId::new(9);
                    let operands_fail = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: src_lane,
                        dst_lane: dst_lane,
                        old_gen: Generation::new(1),
                        new_gen: Generation::new(2),
                        seq_tx: 0,
                        seq_rx: 0,
                    };

                    let err = cluster
                        .run_effect_step(src_id, CpCommand::topology_begin(sid_fail, operands_fail))
                        .expect_err("second begin must fail while begin+ack remains in-flight");
                    assert!(
                        matches!(
                            err,
                            CpError::Topology(
                                crate::control::cluster::error::TopologyError::InProgress
                                    | crate::control::cluster::error::TopologyError::LaneMismatch
                                    | crate::control::cluster::error::TopologyError::StaleGeneration
                                    | crate::control::cluster::error::TopologyError::InvalidState
                                    | crate::control::cluster::error::TopologyError::InvalidSession
                            )
                        ),
                        "error was {:?}",
                        err
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, src_id, src_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn topology_begin_rejects_target_mismatch_before_mutation() {
    run_on_transient_compiled_test_stack(
        "topology_begin_rejects_target_mismatch_before_mutation",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(15);
                    let src_lane = Lane::new(0);
                    let dst_lane = Lane::new(1);
                    let operands = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: src_lane,
                        dst_lane: dst_lane,
                        old_gen: Generation::new(0),
                        new_gen: Generation::new(1),
                        seq_tx: 0,
                        seq_rx: 0,
                    };

                    let err = cluster
                        .run_effect_step(dst_id, CpCommand::topology_begin(sid, operands))
                        .expect_err("wrong target must fail before any topology mutation");
                    assert_eq!(
                        err,
                        CpError::RendezvousMismatch {
                            expected: src_id.raw(),
                            actual: dst_id.raw(),
                        }
                    );

                    cluster.with_control_mut(|core| {
                        assert!(
                            core.topology_state.get(sid).is_none(),
                            "distributed bookkeeping must stay empty after target mismatch",
                        );
                    });
                    assert_eq!(
                        cluster
                            .get_local(&src_id)
                            .expect("registered source rendezvous")
                            .lane_generation(src_lane),
                        Generation::ZERO
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("registered destination rendezvous")
                            .lane_generation(src_lane),
                        Generation::ZERO
                    );
                });
            });
        },
    );
}

#[test]
fn topology_handle_validation_rejects_same_rendezvous_before_mutation() {
    run_on_transient_compiled_test_stack(
        "topology_handle_validation_rejects_same_rendezvous_before_mutation",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, _dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register rendezvous");

                    let sid = SessionId::new(16);
                    let (src_handle, src_lane) = attach_session_lane(cluster, rv_id, sid);
                    let dst_lane = Lane::new(1);
                    let handle = TopologyHandle {
                        src_rv: rv_id.raw(),
                        dst_rv: rv_id.raw(),
                        src_lane: src_lane.raw() as u16,
                        dst_lane: dst_lane.raw() as u16,
                        old_gen: 0,
                        new_gen: 1,
                        seq_tx: 0,
                        seq_rx: 0,
                    };
                    let descriptor =
                        TopologyDescriptor::decode_for(ControlOp::TopologyBegin, handle.encode())
                            .expect("topology handle must lower to descriptor");
                    let operands = descriptor.operands();

                    assert_eq!(
                        cluster.validate_topology_begin_operands(rv_id, src_lane, operands, None,),
                        Err(CpError::Authorisation {
                            operation: ControlOp::TopologyBegin as u8,
                        }),
                        "same-rendezvous topology begin must be rejected at descriptor validation",
                    );
                    assert_eq!(
                        cluster.validate_topology_ack_operands(rv_id, dst_lane, operands, None),
                        Err(CpError::Authorisation {
                            operation: ControlOp::TopologyAck as u8,
                        }),
                        "same-rendezvous topology ack must be rejected at descriptor validation",
                    );
                    assert_eq!(
                        cluster.validate_topology_commit_operands(rv_id, src_lane, operands, None,),
                        Err(CpError::Authorisation {
                            operation: ControlOp::TopologyCommit as u8,
                        }),
                        "same-rendezvous topology commit must be rejected at descriptor validation",
                    );

                    cluster.with_control_mut(|core| {
                        assert!(
                            core.topology_state.get(sid).is_none(),
                            "same-rendezvous validation must not create distributed topology state",
                        );
                    });
                    assert_eq!(
                        cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .topology_session_state(sid),
                        None,
                        "same-rendezvous validation must not stage local topology state",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, src_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn topology_begin_rejects_stale_source_generation_before_mutation() {
    run_on_transient_compiled_test_stack(
        "topology_begin_rejects_stale_source_generation_before_mutation",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(16);
                    let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                    let dst_lane = Lane::new(1);

                    advance_lane_generation(cluster, src_id, src_lane, Generation::new(1));

                    let stale = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: src_lane,
                        dst_lane: dst_lane,
                        old_gen: Generation::new(0),
                        new_gen: Generation::new(2),
                        seq_tx: 0,
                        seq_rx: 0,
                    };

                    let err = cluster
                        .run_effect_step(src_id, CpCommand::topology_begin(sid, stale))
                        .expect_err("stale predecessor generation must fail before mutation");
                    assert!(matches!(
                        err,
                        CpError::Topology(TopologyError::StaleGeneration)
                    ));

                    cluster.with_control_mut(|core| {
                        assert!(
                            core.topology_state.get(sid).is_none(),
                            "distributed bookkeeping must stay empty after stale begin",
                        );
                    });
                    assert_eq!(
                        cluster
                            .get_local(&src_id)
                            .expect("registered source rendezvous")
                            .lane_generation(src_lane),
                        Generation::new(1)
                    );

                    let correct = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: src_lane,
                        dst_lane: dst_lane,
                        old_gen: Generation::new(1),
                        new_gen: Generation::new(2),
                        seq_tx: 0,
                        seq_rx: 0,
                    };

                    let pending = cluster
                        .run_effect_step(src_id, CpCommand::topology_begin(sid, correct))
                        .expect("correct predecessor generation must still succeed");
                    assert!(matches!(pending, PendingEffect::None));

                    unsafe {
                        drop_test_public_endpoint(cluster, src_id, src_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn topology_ack_preflight_rejects_mismatch_before_destination_mutation() {
    run_on_transient_compiled_test_stack(
        "topology_ack_preflight_rejects_mismatch_before_destination_mutation",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(17);
                    let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                    let dst_lane = Lane::new(1);
                    let operands = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: src_lane,
                        dst_lane: dst_lane,
                        old_gen: Generation::new(0),
                        new_gen: Generation::new(1),
                        seq_tx: 0,
                        seq_rx: 0,
                    };

                    cluster
                        .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                        .expect("begin effect");

                    let bad_handle = TopologyHandle {
                        src_rv: src_id.raw(),
                        dst_rv: dst_id.raw(),
                        src_lane: src_lane.raw() as u16,
                        dst_lane: dst_lane.raw() as u16,
                        old_gen: operands.old_gen.raw(),
                        new_gen: Generation::new(2).raw(),
                        seq_tx: operands.seq_tx,
                        seq_rx: operands.seq_rx,
                    };

                    let err = cluster
                        .dispatch_topology_ack_with_handle(
                            dst_id,
                            sid,
                            dst_lane,
                            TopologyHandle::decode(bad_handle.encode())
                                .expect("decode topology handle"),
                            None,
                        )
                        .expect_err("mismatched ack must fail before mutating destination");
                    assert!(matches!(
                        err,
                        CpError::Topology(TopologyError::GenerationMismatch)
                    ));

                    let good_handle = TopologyHandle {
                        src_rv: src_id.raw(),
                        dst_rv: dst_id.raw(),
                        src_lane: src_lane.raw() as u16,
                        dst_lane: dst_lane.raw() as u16,
                        old_gen: operands.old_gen.raw(),
                        new_gen: operands.new_gen.raw(),
                        seq_tx: operands.seq_tx,
                        seq_rx: operands.seq_rx,
                    };

                    cluster
                        .dispatch_topology_ack_with_handle(
                            dst_id,
                            sid,
                            dst_lane,
                            TopologyHandle::decode(good_handle.encode())
                                .expect("decode topology handle"),
                            None,
                        )
                        .expect("correct ack must still succeed after the rejected attempt");

                    unsafe {
                        drop_test_public_endpoint(cluster, src_id, src_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn topology_commit_preflight_rejects_mismatch_before_source_mutation() {
    run_on_transient_compiled_test_stack(
        "topology_commit_preflight_rejects_mismatch_before_source_mutation",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(19);
                    let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                    let dst_lane = Lane::new(1);
                    let operands = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: src_lane,
                        dst_lane: dst_lane,
                        old_gen: Generation::new(0),
                        new_gen: Generation::new(1),
                        seq_tx: 0,
                        seq_rx: 0,
                    };

                    cluster
                        .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                        .expect("begin effect");

                    let handle = TopologyHandle {
                        src_rv: src_id.raw(),
                        dst_rv: dst_id.raw(),
                        src_lane: src_lane.raw() as u16,
                        dst_lane: dst_lane.raw() as u16,
                        old_gen: operands.old_gen.raw(),
                        new_gen: operands.new_gen.raw(),
                        seq_tx: operands.seq_tx,
                        seq_rx: operands.seq_rx,
                    };
                    cluster
                        .dispatch_topology_ack_with_handle(
                            dst_id,
                            sid,
                            dst_lane,
                            TopologyHandle::decode(handle.encode())
                                .expect("decode topology handle"),
                            None,
                        )
                        .expect("ack succeeds");
                    assert_eq!(
                        cluster
                            .get_local(&src_id)
                            .expect("source rendezvous")
                            .expected_topology_ack(sid),
                        Ok(operands.ack(sid)),
                        "ack must preserve source-side expected ACK until commit",
                    );

                    let bad_operands = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: src_lane,
                        dst_lane: dst_lane,
                        old_gen: operands.old_gen,
                        new_gen: Generation::new(2),
                        seq_tx: operands.seq_tx,
                        seq_rx: operands.seq_rx,
                    };

                    let err = cluster
                        .run_effect_step(src_id, CpCommand::topology_commit(sid, bad_operands))
                        .expect_err("mismatched commit must fail before mutating source");
                    assert!(matches!(
                        err,
                        CpError::Topology(TopologyError::CommitFailed)
                    ));
                    assert!(
                        matches!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .expected_topology_ack(sid),
                            Err(crate::rendezvous::error::TopologyError::UnknownSession { .. })
                        ),
                        "rejected commit must abort the source-side topology owner instead of preserving a wedged retry path",
                    );
                    assert!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .preflight_destination_topology_commit(sid, dst_lane)
                            .is_err(),
                        "rejected commit must roll back destination prepare state",
                    );
                    assert_eq!(
                        cluster.distributed_topology_operands(sid),
                        None,
                        "rejected commit must clear cluster-owned distributed topology state",
                    );
                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, src_handle.0, src_handle.1)
                            .is_some(),
                        "rejected commit must not revoke the source public endpoint",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, src_id, src_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn destination_attach_aborts_acked_topology_before_retry() {
    run_on_transient_compiled_test_stack(
        "destination_attach_aborts_acked_topology_before_retry",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(21);
                    let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                    let dst_lane = Lane::new(1);
                    let operands = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: src_lane,
                        dst_lane: dst_lane,
                        old_gen: Generation::new(0),
                        new_gen: Generation::new(1),
                        seq_tx: 0,
                        seq_rx: 0,
                    };

                    cluster
                        .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                        .expect("begin effect");
                    let handle = TopologyHandle {
                        src_rv: src_id.raw(),
                        dst_rv: dst_id.raw(),
                        src_lane: src_lane.raw() as u16,
                        dst_lane: dst_lane.raw() as u16,
                        old_gen: operands.old_gen.raw(),
                        new_gen: operands.new_gen.raw(),
                        seq_tx: operands.seq_tx,
                        seq_rx: operands.seq_rx,
                    };
                    cluster
                        .dispatch_topology_ack_with_handle(
                            dst_id,
                            sid,
                            dst_lane,
                            TopologyHandle::decode(handle.encode())
                                .expect("decode topology handle"),
                            None,
                        )
                        .expect("ack succeeds");
                    assert!(
                        !cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .is_session_registered(sid),
                        "destination ack must keep the destination rendezvous provisional until source commit",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .session_lane(sid),
                        None,
                        "destination ack must not expose a committed session lane before source commit",
                    );

                    assert_eq!(
                        cluster.distributed_topology_operands(sid),
                        Some(operands),
                        "distributed topology state must own the in-flight session until source commit",
                    );
                    let err = cluster.enter(
                        dst_id,
                        sid,
                        &attach_program(),
                        crate::binding::BindingHandle::None(crate::binding::NoBinding),
                    );
                    assert!(matches!(
                        err.as_ref().err().and_then(AttachError::control_cause),
                        Some(CpError::Topology(TopologyError::InvalidState))
                    ));
                    assert!(
                        !cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .is_session_registered(sid),
                        "rejected destination attach must not make the prepared destination session live",
                    );
                    assert_eq!(
                        cluster.distributed_topology_operands(sid),
                        None,
                        "rejected destination attach must close the cluster-owned topology owner",
                    );
                    assert!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .preflight_destination_topology_commit(sid, dst_lane)
                            .is_err(),
                        "rejected destination attach must roll back the destination prepare state",
                    );
                    assert!(
                        matches!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .expected_topology_ack(sid),
                            Err(crate::rendezvous::error::TopologyError::UnknownSession { .. })
                        ),
                        "rejected destination attach must roll back the source-side topology owner",
                    );
                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, src_handle.0, src_handle.1)
                            .is_some(),
                        "rejected destination attach must not revoke the existing source public endpoint",
                    );

                    let late_commit = cluster
                        .run_effect_step(src_id, CpCommand::topology_commit(sid, operands))
                        .expect_err("late source commit must not revive an attach-closed topology");
                    assert!(matches!(
                        late_commit,
                        CpError::Topology(TopologyError::InvalidSession)
                    ));

                    cluster
                        .run_effect_step(src_id, CpCommand::topology_begin(sid, operands))
                        .expect("closed acked topology must not block a fresh begin for the sid");

                    unsafe {
                        drop_test_public_endpoint(cluster, src_id, src_handle);
                    }
                });
            });
        },
    );
}
