use super::super::*;

#[test]
fn topology_commit_descriptor_rejects_fixed_header_lane_mismatch_before_mutation() {
    run_on_transient_compiled_test_stack(
        "topology_commit_descriptor_rejects_fixed_header_lane_mismatch_before_mutation",
        || {
            use crate::control::cap::atomic_codecs::TopologyHandle;

            fn topology_commit_token(
                desc: ControlDesc,
                sid: SessionId,
                lane: Lane,
                handle: TopologyHandle,
            ) -> [u8; CAP_TOKEN_LEN] {
                let mut header = [0u8; CAP_HEADER_LEN];
                CapHeader::new(
                    sid,
                    lane,
                    0,
                    desc.resource_tag(),
                    desc.op(),
                    desc.path(),
                    desc.shot(),
                    desc.scope_kind(),
                    desc.header_flags(),
                    0,
                    0,
                    handle.encode(),
                )
                .encode(&mut header);
                token_wire_image([0; CAP_NONCE_LEN], header)
            }

            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(20);
                    let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                    let dst_lane = Lane::new(1);
                    let wrong_header_lane = Lane::new(2);
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
                        None,
                    )
                    .expect("ack succeeds");

                    let desc = ControlDesc::new(
                        EffIndex::MAX,
                        ControlDesc::STATIC_POLICY_SITE,
                        0x04A7,
                        TAG_TOPOLOGY_BEGIN_CONTROL,
                        ControlOp::TopologyCommit,
                        crate::global::const_dsl::ControlScopeKind::Topology,
                        ControlPath::Wire,
                        CapShot::One,
                        true,
                    );
                    let bytes = topology_commit_token(
                        desc,
                        sid,
                        wrong_header_lane,
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
                    );

                    let err = match prepare_descriptor_commit(cluster, src_id, bytes, desc, 0) {
                        Ok(_) => {
                            panic!("fixed-header lane mismatch must fail before commit")
                        }
                        Err(err) => err,
                    };
                    assert_eq!(
                        err,
                        CpError::Authorisation {
                            operation: ControlOp::TopologyCommit as u8,
                        }
                    );
                    assert_eq!(
                        cluster
                            .get_local(&src_id)
                            .expect("source rendezvous")
                            .expected_topology_ack(sid),
                        Ok(operands.ack(sid)),
                        "descriptor rejection must not clear source-side expected ACK",
                    );
                    assert_eq!(
                        cluster.distributed_topology_operands(sid),
                        Some(operands),
                        "rejected commit must leave distributed topology bookkeeping intact",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&src_id)
                            .expect("source rendezvous")
                            .expected_topology_ack(sid),
                        Ok(operands.ack(sid)),
                        "rejected token must preserve source-side expected ACK",
                    );

                    publish_topology_commit_at(cluster, src_id, sid, operands)
                        .expect("correct commit must still succeed after rejected token");

                    unsafe {
                        drop_test_public_endpoint(cluster, src_id, src_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn prepare_topology_operands_from_descriptor_decodes_typed_handle() {
    run_on_transient_compiled_test_stack(
        "prepare_topology_operands_from_descriptor_decodes_typed_handle",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let expected = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: Lane::new(3),
                        dst_lane: Lane::new(7),
                        old_gen: Generation::new(11),
                        new_gen: Generation::new(12),
                        seq_tx: 13,
                        seq_rx: 14,
                    };
                    let descriptor = TopologyDescriptor::decode_for(
                        ControlOp::TopologyBegin,
                        topology_handle(expected).encode(),
                    )
                    .expect("typed topology handle must decode");
                    let operands = cluster
                        .prepare_topology_operands_from_descriptor(
                            src_id,
                            Lane::new(3),
                            ControlDesc::new(
                                EffIndex::MAX,
                                ControlDesc::STATIC_POLICY_SITE,
                                0x04A8,
                                TAG_TOPOLOGY_BEGIN_CONTROL,
                                ControlOp::TopologyBegin,
                                crate::global::const_dsl::ControlScopeKind::Topology,
                                ControlPath::Wire,
                                CapShot::One,
                                true,
                            ),
                            descriptor,
                        )
                        .expect("typed topology descriptor must decode topology operands");

                    assert_eq!(operands, expected);
                });
            });
        },
    );
}

#[test]
fn topology_descriptor_rejects_lanes_outside_wire_domain() {
    let mut handle = TopologyHandle {
        src_rv: 1,
        dst_rv: 2,
        src_lane: 256,
        dst_lane: 7,
        old_gen: 11,
        new_gen: 12,
        seq_tx: 13,
        seq_rx: 14,
    };

    assert!(
        TopologyDescriptor::decode_for(ControlOp::TopologyBegin, handle.encode()).is_err(),
        "topology descriptor decode must fail closed instead of truncating source lane 256"
    );

    handle.src_lane = 3;
    handle.dst_lane = 256;
    assert!(
        TopologyDescriptor::decode_for(ControlOp::TopologyBegin, handle.encode()).is_err(),
        "topology descriptor decode must fail closed instead of truncating destination lane 256"
    );
}

#[test]
fn prepare_topology_operands_from_descriptor_rejects_same_rendezvous() {
    run_on_transient_compiled_test_stack(
        "prepare_topology_operands_from_descriptor_rejects_same_rendezvous",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, _dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(22);

                    let operands = TopologyOperands {
                        src_rv: rv_id,
                        dst_rv: rv_id,
                        src_lane: Lane::new(3),
                        dst_lane: Lane::new(7),
                        old_gen: Generation::new(11),
                        new_gen: Generation::new(12),
                        seq_tx: 13,
                        seq_rx: 14,
                    };
                    let descriptor = TopologyDescriptor::decode_for(
                        ControlOp::TopologyBegin,
                        topology_handle(operands).encode(),
                    )
                    .expect("typed topology handle must decode");
                    let err = cluster
                        .prepare_topology_operands_from_descriptor(
                            rv_id,
                            Lane::new(3),
                            ControlDesc::new(
                                EffIndex::MAX,
                                ControlDesc::STATIC_POLICY_SITE,
                                0x04A8,
                                TAG_TOPOLOGY_BEGIN_CONTROL,
                                ControlOp::TopologyBegin,
                                crate::global::const_dsl::ControlScopeKind::Topology,
                                ControlPath::Wire,
                                CapShot::One,
                                true,
                            ),
                            descriptor,
                        )
                        .expect_err("typed topology descriptor must reject same-rendezvous");

                    assert_eq!(
                        err,
                        CpError::Authorisation {
                            operation: ControlOp::TopologyBegin as u8,
                        }
                    );
                    cluster.with_control_mut(|core| {
                            assert!(
                                core.topology_state.get(sid).is_none(),
                                "same-rendezvous topology descriptor must not create distributed topology state",
                            );
                        });
                });
            });
        },
    );
}

#[test]
fn validate_topology_operands_from_descriptor_rejects_ack_mismatch() {
    run_on_transient_compiled_test_stack(
        "validate_topology_operands_from_descriptor_rejects_ack_mismatch",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let operands = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: Lane::new(3),
                        dst_lane: Lane::new(7),
                        old_gen: Generation::new(11),
                        new_gen: Generation::new(12),
                        seq_tx: 13,
                        seq_rx: 14,
                    };

                    let mismatched = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: Lane::new(3),
                        dst_lane: Lane::new(7),
                        old_gen: Generation::new(11),
                        new_gen: Generation::new(13),
                        seq_tx: 13,
                        seq_rx: 14,
                    };
                    let descriptor = TopologyDescriptor::decode_for(
                        ControlOp::TopologyAck,
                        topology_handle(mismatched).encode(),
                    )
                    .expect("typed topology handle must decode");
                    let err = cluster
                        .validate_topology_operands_from_descriptor(
                            dst_id,
                            Lane::new(7),
                            ControlDesc::new(
                                EffIndex::MAX,
                                ControlDesc::STATIC_POLICY_SITE,
                                0x04A9,
                                0x7C,
                                ControlOp::TopologyAck,
                                crate::global::const_dsl::ControlScopeKind::Topology,
                                ControlPath::Wire,
                                CapShot::One,
                                true,
                            ),
                            descriptor,
                            operands,
                        )
                        .expect_err("typed ack descriptor validation must reject mismatch");

                    assert_eq!(
                        err,
                        CpError::Authorisation {
                            operation: ControlOp::TopologyAck as u8,
                        }
                    );
                });
            });
        },
    );
}
