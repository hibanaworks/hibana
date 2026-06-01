use super::super::*;
use crate::control::cap::mint::ControlPath;

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
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
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
fn topology_descriptor_rejects_invalid_wire_facts() {
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

    handle.dst_lane = 7;
    handle.src_rv = 0;
    assert!(
        TopologyDescriptor::decode_for(ControlOp::TopologyBegin, handle.encode()).is_err(),
        "topology descriptor decode must reject zero source rendezvous without panicking"
    );

    handle.src_rv = 1;
    handle.dst_rv = 0;
    assert!(
        TopologyDescriptor::decode_for(ControlOp::TopologyBegin, handle.encode()).is_err(),
        "topology descriptor decode must reject zero destination rendezvous without panicking"
    );

    handle.dst_rv = 1;
    assert!(
        TopologyDescriptor::decode_for(ControlOp::TopologyBegin, handle.encode()).is_err(),
        "topology descriptor decode must reject same-rendezvous topology handles"
    );
}
