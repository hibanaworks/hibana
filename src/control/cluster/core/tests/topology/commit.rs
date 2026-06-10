use super::super::*;

#[test]
fn destination_attach_after_topology_commit_still_initializes_effect_image() {
    run_on_transient_compiled_test_stack(
        "destination_attach_after_topology_commit_still_initializes_effect_image",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let state_snapshot_program = state_snapshot_transfer_program();
                    let source_projected: crate::integration::program::RoleProgram<0> =
                        role_program::project(&state_snapshot_program);
                    let destination_projected: crate::integration::program::RoleProgram<1> =
                        role_program::project(&state_snapshot_program);
                    let role_image = cluster
                        .ensure_role_image_slice(dst_id, &destination_projected)
                        .expect("materialize destination role image");
                    let program_image = role_image.program();
                    let effect_envelope = program_image.effect_envelope();
                    assert!(
                        effect_envelope.resources().next().is_some(),
                        "state-snapshot program must expose a control resource"
                    );

                    let sid = SessionId::new(31);
                    let (src_handle, src_lane) =
                        attach_session_lane_for_program(cluster, src_id, sid, &source_projected);
                    let dst_lane = Lane::new(0);
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
                        TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        },
                    )
                    .expect("ack succeeds");
                    publish_topology_commit_at(cluster, src_id, sid, operands)
                        .expect("commit succeeds");

                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .snapshot_generation(dst_lane),
                        None,
                        "topology finalization alone must not masquerade as endpoint effect initialization",
                    );

                    let dst_handle = cluster
                        .enter(dst_id, sid, &destination_projected)
                        .expect("destination attach must succeed after source commit");

                    let destination = cluster.get_local(&dst_id).expect("destination rendezvous");
                    let generation = destination.lane_generation(dst_lane);
                    let snapshot = destination
                        .prepare_state_snapshot_effect(sid, dst_lane, generation)
                        .expect("destination attach must install the state-snapshot effect image");
                    destination.publish_prepared_state_snapshot_effect(snapshot);
                    assert_eq!(
                        destination.snapshot_generation(dst_lane),
                        Some(generation),
                        "destination attach must still install the compiled effect image even when topology commit pre-registered the migrated session",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, src_id, src_handle);
                        drop_test_public_endpoint(cluster, dst_id, dst_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn topology_commit_preserves_source_until_cluster_owned_commit() {
    run_on_transient_compiled_test_stack(
        "topology_commit_preserves_source_until_cluster_owned_commit",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(23);
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
                    publish_topology_ack_handle(
                        cluster,
                        dst_id,
                        sid,
                        dst_lane,
                        TopologyHandle::decode(encode_topology_handle(handle))
                            .expect("decode topology handle"),
                    )
                    .expect("ack succeeds");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, src_handle.0, src_handle.1)
                            .is_some(),
                        "source endpoint must be live before the rejected direct commit",
                    );

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, src_handle.0, src_handle.1)
                            .is_some(),
                        "source endpoint must remain live until the cluster-owned topology commit",
                    );
                    assert!(
                        test_session_bound_to(cluster, src_id, sid, src_lane),
                        "source lane must remain live until the cluster-owned topology commit",
                    );
                    assert_eq!(
                        topology_state_operands(cluster, sid),
                        Some(operands),
                        "distributed topology owner must remain until cluster-owned commit",
                    );
                    assert!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .preflight_destination_topology_commit(sid, dst_lane)
                            .is_ok(),
                        "destination topology must stay pending until cluster-owned commit",
                    );

                    publish_topology_commit_at(cluster, src_id, sid, operands)
                        .expect("cluster-owned topology commit must succeed");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, src_handle.0, src_handle.1)
                            .is_none(),
                        "cluster-owned topology commit must revoke the source public endpoint",
                    );
                    assert!(
                        !test_session_bound_to(cluster, src_id, sid, src_lane),
                        "cluster-owned topology commit must retire the source lane",
                    );
                    assert!(
                        !test_session_bound_to(cluster, dst_id, sid, dst_lane),
                        "cluster-owned topology commit must leave the destination attach-ready rather than pre-registering the session",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .topology_session_state(sid),
                        Some(TopologySessionState::DestinationAttachReady { lane: dst_lane }),
                        "cluster-owned topology commit must transfer destination ownership into topology attach-ready state",
                    );
                });
            });
        },
    );
}

#[test]
fn topology_commit_revokes_nonzero_role_public_endpoint() {
    run_on_transient_compiled_test_stack(
        "topology_commit_revokes_nonzero_role_public_endpoint",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(24);
                    let worker_program = linear_program::worker_program();
                    let (src_handle, src_lane) =
                        attach_session_lane_for_program(cluster, src_id, sid, &worker_program);
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
                    publish_topology_ack_handle(
                        cluster,
                        dst_id,
                        sid,
                        dst_lane,
                        TopologyHandle::decode(encode_topology_handle(handle))
                            .expect("decode topology handle"),
                    )
                    .expect("ack succeeds");

                    publish_topology_commit_at(cluster, src_id, sid, operands)
                        .expect("commit succeeds");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, src_handle.0, src_handle.1)
                            .is_none(),
                        "commit must revoke a nonzero-role source endpoint without role punning",
                    );
                    assert!(
                        !test_session_bound_to(cluster, src_id, sid, src_lane),
                        "commit must retire the worker-owned source lane",
                    );
                });
            });
        },
    );
}

#[test]
fn topology_commit_revokes_same_session_endpoints_even_when_only_one_owns_source_lane() {
    run_on_transient_compiled_test_stack(
        "topology_commit_revokes_same_session_endpoints_even_when_only_one_owns_source_lane",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(26);
                    let (controller_handle, controller_lane) =
                        attach_session_lane(cluster, src_id, sid);
                    let worker_program = lane1_worker_program();
                    let (worker_handle, _worker_lane) =
                        attach_session_lane_for_program(cluster, src_id, sid, &worker_program);
                    let src_lane = Lane::new(1);
                    let dst_lane = Lane::new(2);

                    assert_eq!(
                        controller_lane,
                        Lane::new(0),
                        "controller endpoint should remain on the canonical control lane",
                    );

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
                        .expect("begin effect for non-control source lane");
                    publish_topology_ack_handle(
                        cluster,
                        dst_id,
                        sid,
                        dst_lane,
                        TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        },
                    )
                    .expect("ack succeeds");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(
                                src_id,
                                controller_handle.0,
                                controller_handle.1,
                            )
                            .is_some(),
                        "controller endpoint must be live before commit",
                    );
                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, worker_handle.0, worker_handle.1,)
                            .is_some(),
                        "worker endpoint must be live before commit",
                    );

                    publish_topology_commit_at(cluster, src_id, sid, operands)
                        .expect("commit succeeds");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(
                                src_id,
                                controller_handle.0,
                                controller_handle.1,
                            )
                            .is_none(),
                        "commit must revoke controller endpoints that share the migrated session even without owning the source lane",
                    );
                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, worker_handle.0, worker_handle.1,)
                            .is_none(),
                        "commit must also revoke the endpoint that owns the migrated source lane",
                    );
                    assert!(
                        !test_session_bound_to(cluster, src_id, sid, src_lane),
                        "session-wide revoke must retire the migrated session after commit",
                    );
                });
            });
        },
    );
}

#[test]
fn topology_commit_retires_hidden_source_lane_even_when_same_session_public_endpoint_exists() {
    run_on_transient_compiled_test_stack(
        "topology_commit_retires_hidden_source_lane_even_when_same_session_public_endpoint_exists",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(27);
                    let (controller_handle, controller_lane) =
                        attach_session_lane(cluster, src_id, sid);
                    let src_lane = Lane::new(2);
                    let dst_lane = Lane::new(3);

                    assert_eq!(
                        controller_lane,
                        Lane::new(0),
                        "public controller endpoint should stay on the canonical control lane",
                    );

                    cluster.with_control_mut(|core| {
                        let rv = core.locals.get_mut(&src_id).expect("source rendezvous");
                        rv.activate_lane_attachment(sid, src_lane)
                            .expect("test fixture must create a non-public source lane");
                    });

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
                        .expect("begin effect for non-public source lane");
                    publish_topology_ack_handle(
                        cluster,
                        dst_id,
                        sid,
                        dst_lane,
                        TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        },
                    )
                    .expect("ack succeeds");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(
                                src_id,
                                controller_handle.0,
                                controller_handle.1,
                            )
                            .is_some(),
                        "controller endpoint must be live before commit",
                    );

                    publish_topology_commit_at(cluster, src_id, sid, operands)
                        .expect("commit succeeds");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(
                                src_id,
                                controller_handle.0,
                                controller_handle.1,
                            )
                            .is_none(),
                        "commit must still revoke same-session public endpoints before retiring the hidden source lane",
                    );
                    assert!(
                        !test_session_bound_to(cluster, src_id, sid, src_lane),
                        "commit must explicitly retire the migrated source lane even when revoke found only unrelated public endpoints",
                    );
                });
            });
        },
    );
}

#[test]
fn topology_commit_retires_extra_hidden_same_session_lanes() {
    run_on_transient_compiled_test_stack(
        "topology_commit_retires_extra_hidden_same_session_lanes",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(28);
                    let (controller_handle, controller_lane) =
                        attach_session_lane(cluster, src_id, sid);
                    let extra_lane = Lane::new(1);
                    let src_lane = Lane::new(2);
                    let dst_lane = Lane::new(3);

                    assert_eq!(
                        controller_lane,
                        Lane::new(0),
                        "public controller endpoint should stay on the canonical control lane",
                    );

                    cluster.with_control_mut(|core| {
                        let rv = core.locals.get_mut(&src_id).expect("source rendezvous");
                        rv.activate_lane_attachment(sid, extra_lane)
                            .expect("fixture must create an extra hidden same-session lane");
                        rv.activate_lane_attachment(sid, src_lane)
                            .expect("fixture must create the hidden source lane");
                    });

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
                        .expect("begin effect for hidden source lane");
                    publish_topology_ack_handle(
                        cluster,
                        dst_id,
                        sid,
                        dst_lane,
                        TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: src_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        },
                    )
                    .expect("ack succeeds");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(
                                src_id,
                                controller_handle.0,
                                controller_handle.1,
                            )
                            .is_some(),
                        "controller endpoint must be live before commit",
                    );
                    assert!(
                        test_session_bound_to(cluster, src_id, sid, controller_lane),
                        "same-session lookup should still see a live source-side lane before commit",
                    );

                    publish_topology_commit_at(cluster, src_id, sid, operands)
                        .expect("commit succeeds");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(
                                src_id,
                                controller_handle.0,
                                controller_handle.1,
                            )
                            .is_none(),
                        "commit must revoke the public controller endpoint before session teardown",
                    );
                    assert!(
                        !test_session_bound_to(cluster, src_id, sid, controller_lane),
                        "commit must retire every same-session source lane, not just the migrated source lane",
                    );
                });
            });
        },
    );
}

#[test]
fn topology_commit_revokes_all_public_endpoints_for_the_source_session() {
    run_on_transient_compiled_test_stack(
        "topology_commit_revokes_all_public_endpoints_for_the_source_session",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(25);
                    let worker_program = linear_program::worker_program();
                    let (controller_handle, controller_lane) =
                        attach_session_lane(cluster, src_id, sid);
                    let (worker_handle, worker_lane) =
                        attach_session_lane_for_program(cluster, src_id, sid, &worker_program);
                    assert_eq!(
                        controller_lane, worker_lane,
                        "same-session public endpoints must share the source lane authority"
                    );

                    let dst_lane = Lane::new(1);
                    let operands = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: controller_lane,
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
                        TopologyHandle {
                            src_rv: src_id.raw(),
                            dst_rv: dst_id.raw(),
                            src_lane: controller_lane.raw() as u16,
                            dst_lane: dst_lane.raw() as u16,
                            old_gen: operands.old_gen.raw(),
                            new_gen: operands.new_gen.raw(),
                            seq_tx: operands.seq_tx,
                            seq_rx: operands.seq_rx,
                        },
                    )
                    .expect("ack succeeds");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(
                                src_id,
                                controller_handle.0,
                                controller_handle.1,
                            )
                            .is_some(),
                        "controller endpoint must be live before commit",
                    );
                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, worker_handle.0, worker_handle.1,)
                            .is_some(),
                        "worker endpoint must be live before commit",
                    );

                    publish_topology_commit_at(cluster, src_id, sid, operands)
                        .expect("commit succeeds");

                    assert!(
                        cluster
                            .public_endpoint_header_ptr(
                                src_id,
                                controller_handle.0,
                                controller_handle.1,
                            )
                            .is_none(),
                        "commit must revoke the controller endpoint",
                    );
                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, worker_handle.0, worker_handle.1,)
                            .is_none(),
                        "commit must revoke every public endpoint sharing the source lane",
                    );
                    assert!(
                        !test_session_bound_to(cluster, src_id, sid, controller_lane),
                        "commit must retire the source lane after all public leases are revoked",
                    );
                });
            });
        },
    );
}
