use super::super::*;

#[test]
fn destination_attach_aborts_begin_topology_before_ack_retry() {
    run_on_transient_compiled_test_stack(
        "destination_attach_aborts_begin_topology_before_ack_retry",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(32);
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

                    publish_topology_begin_at(cluster, src_id, sid, operands)
                        .expect("begin effect");
                    assert_eq!(
                        cluster.distributed_topology_operands(sid),
                        Some(operands),
                        "begin topology must be cluster-owned before closeout",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&src_id)
                            .expect("source rendezvous")
                            .expected_topology_ack(sid),
                        Ok(operands.ack(sid)),
                        "begin topology must stage the source owner before closeout",
                    );
                    assert!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .preflight_destination_topology_commit(sid, dst_lane)
                            .is_err(),
                        "begin topology must not expose destination commit before ack",
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
                    assert_eq!(
                        cluster.distributed_topology_operands(sid),
                        None,
                        "destination attach must close begin-phase distributed topology",
                    );
                    assert!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .preflight_destination_topology_commit(sid, dst_lane)
                            .is_err(),
                        "destination attach must roll back begin-phase destination prepare",
                    );
                    assert!(
                        matches!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .expected_topology_ack(sid),
                            Err(crate::rendezvous::error::TopologyError::UnknownSession { .. })
                        ),
                        "destination attach must roll back begin-phase source owner",
                    );

                    let late_ack = publish_topology_ack_handle(
                        cluster,
                        dst_id,
                        sid,
                        dst_lane,
                        topology_handle(operands),
                        None,
                    )
                    .expect_err("late ack must not revive an attach-closed topology");
                    assert!(matches!(
                        late_ack,
                        CpError::Topology(TopologyError::InvalidSession)
                    ));

                    publish_topology_begin_at(cluster, src_id, sid, operands)
                        .expect("closed begin topology must not block a fresh begin for the sid");

                    unsafe {
                        drop_test_public_endpoint(cluster, src_id, src_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn source_attach_aborts_acked_topology_before_retry() {
    run_on_transient_compiled_test_stack(
        "source_attach_aborts_acked_topology_before_retry",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(33);
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

                    publish_topology_begin_at(cluster, src_id, sid, operands)
                        .expect("begin effect");
                    publish_topology_ack_handle(
                        cluster,
                        dst_id,
                        sid,
                        dst_lane,
                        topology_handle(operands),
                        None,
                    )
                    .expect("ack succeeds");
                    assert_eq!(
                        cluster.distributed_topology_operands(sid),
                        Some(operands),
                        "acked topology must be cluster-owned before source closeout",
                    );

                    let err = cluster.enter(
                        src_id,
                        sid,
                        &attach_program(),
                        crate::binding::BindingHandle::None(crate::binding::NoBinding),
                    );
                    assert!(matches!(
                        err.as_ref().err().and_then(AttachError::control_cause),
                        Some(CpError::Topology(TopologyError::InvalidState))
                    ));
                    assert_eq!(
                        cluster.distributed_topology_operands(sid),
                        None,
                        "source attach must close acked distributed topology",
                    );
                    assert!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .preflight_destination_topology_commit(sid, dst_lane)
                            .is_err(),
                        "source attach must roll back acked destination prepare",
                    );
                    assert!(
                        matches!(
                            cluster
                                .get_local(&src_id)
                                .expect("source rendezvous")
                                .expected_topology_ack(sid),
                            Err(crate::rendezvous::error::TopologyError::UnknownSession { .. })
                        ),
                        "source attach must roll back acked source owner",
                    );

                    let late_commit = publish_topology_commit_at(cluster, src_id, sid, operands)
                        .expect_err(
                            "late source commit must not revive a source-attach-closed topology",
                        );
                    assert!(matches!(
                        late_commit,
                        CpError::Topology(TopologyError::InvalidSession)
                    ));

                    publish_topology_begin_at(cluster, src_id, sid, operands).expect(
                        "source-closed acked topology must not block a fresh begin for the sid",
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
fn destination_attach_ready_requires_and_consumes_exact_lane() {
    run_on_transient_compiled_test_stack(
        "destination_attach_ready_requires_and_consumes_exact_lane",
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

                    let sid = SessionId::new(22);
                    let (_src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
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

                    publish_topology_begin_at(cluster, src_id, sid, operands)
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
                    let decoded =
                        TopologyHandle::decode(handle.encode()).expect("decode topology handle");
                    publish_topology_ack_handle(cluster, dst_id, sid, dst_lane, decoded, None)
                        .expect("ack succeeds");
                    publish_topology_commit_at(cluster, src_id, sid, operands)
                        .expect("source commit succeeds");
                    assert!(cluster.distributed_topology_operands(sid).is_none());
                    assert!(
                        !cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .is_session_registered(sid),
                        "successful source commit must leave the destination in attach-ready state rather than pre-registering the lane",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .session_lane(sid),
                        None,
                        "destination lane ownership must materialize on the first real attach",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .topology_session_state(sid),
                        Some(TopologySessionState::DestinationAttachReady { lane: dst_lane }),
                        "successful source commit must leave a destination attach-ready reservation owned by topology state",
                    );

                    let partial = cluster.enter(
                        dst_id,
                        sid,
                        &attach_program(),
                        crate::binding::BindingHandle::None(crate::binding::NoBinding),
                    );
                    assert!(matches!(
                        partial.as_ref().err().and_then(AttachError::control_cause),
                        Some(CpError::Topology(TopologyError::InvalidState))
                    ));
                    assert!(
                        !cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .is_session_registered(sid),
                        "partial attach must not make the destination session live",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .topology_session_state(sid),
                        Some(TopologySessionState::DestinationAttachReady { lane: dst_lane }),
                        "partial attach must preserve the exact-lane reservation for retry",
                    );

                    let dst_handle = cluster
                        .enter(
                            dst_id,
                            sid,
                            &lane1_worker_program(),
                            crate::binding::BindingHandle::None(crate::binding::NoBinding),
                        )
                        .expect("exact destination attach must open after source commit");
                    assert!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .is_session_registered(sid),
                        "first attach must materialize the migrated destination lane",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .topology_session_state(sid),
                        None,
                        "exact-lane attach must consume the migrated destination lane reservation",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .lane_generation(dst_lane),
                        operands.new_gen,
                        "first attach after topology commit must preserve the committed destination generation",
                    );

                    assert!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .preflight_destination_topology_commit(sid, dst_lane)
                            .is_err(),
                        "consumed attach-ready state must not keep blocking future topology as in-progress",
                    );

                    unsafe {
                        drop_test_public_endpoint_for_role::<1, 2>(cluster, dst_id, dst_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn enter_rejects_orphaned_destination_prepare_without_cluster_topology_state() {
    run_on_transient_compiled_test_stack(
        "enter_rejects_orphaned_destination_prepare_without_cluster_topology_state",
        || {
            with_cluster_fixture(|clock, dst_cfg| {
                with_test_cluster_1(clock, |cluster| {
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register destination rendezvous");

                    let sid = SessionId::new(22);
                    let dst_lane = Lane::new(1);
                    cluster.with_control_mut(|core| {
                        let rv = core
                            .locals
                            .get_mut(&dst_id)
                            .expect("destination rendezvous");
                        rv.prepare_topology_control_scope(dst_lane)
                            .expect("orphan prepare test must bind topology storage");
                        rv.process_topology_intent(
                            &crate::control::automaton::distributed::TopologyIntent {
                                src_rv: RendezvousId::new(99),
                                dst_rv: dst_id,
                                sid: sid.raw(),
                                old_gen: Generation::ZERO,
                                new_gen: Generation::new(1),
                                seq_tx: 0,
                                seq_rx: 0,
                                src_lane: Lane::new(0),
                                dst_lane: dst_lane,
                            },
                        )
                        .expect("direct orphan prepare must stage destination pending state");
                    });

                    assert_eq!(
                        cluster.distributed_topology_operands(sid),
                        None,
                        "orphan prepare test must not create cluster-owned distributed topology state",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .preflight_destination_topology_commit(sid, dst_lane),
                        Ok(()),
                        "direct orphan prepare must leave destination pending before explicit abort",
                    );

                    assert!(
                        matches!(
                            cluster
                                .enter(
                                    dst_id,
                                    sid,
                                    &lane1_worker_program(),
                                    crate::binding::BindingHandle::None(crate::binding::NoBinding),
                                )
                                .as_ref()
                                .err()
                                .and_then(AttachError::control_cause),
                            Some(CpError::Topology(TopologyError::InvalidState)),
                        ),
                        "attach must not silently recover orphaned destination prepare",
                    );

                    cluster.with_control_mut(|core| {
                        let rv = core
                            .locals
                            .get_mut(&dst_id)
                            .expect("destination rendezvous");
                        assert_eq!(
                            rv.abort_topology_state(sid),
                            Ok(true),
                            "explicit abort must consume orphaned destination prepare",
                        );
                    });

                    let handle = cluster
                        .enter(
                            dst_id,
                            sid,
                            &lane1_worker_program(),
                            crate::binding::BindingHandle::None(crate::binding::NoBinding),
                        )
                        .expect("attach may proceed only after explicit topology recovery");

                    assert!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .is_session_registered(sid),
                        "attach must still register the session after orphan cleanup",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .abort_topology_state(sid),
                        Ok(false),
                        "explicit abort must consume the stale prepared topology before attach completes",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, dst_id, handle);
                    }
                });
            });
        },
    );
}
