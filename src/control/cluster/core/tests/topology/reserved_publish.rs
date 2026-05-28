use super::super::*;

fn topology_control_token(
    desc: ControlDesc,
    sid: SessionId,
    lane: Lane,
    handle: crate::control::cap::atomic_codecs::TopologyHandle,
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

#[test]
fn distributed_topology_reserved_publish_consumes_prepared_commit_proofs() {
    run_on_transient_compiled_test_stack(
        "distributed_topology_reserved_publish_consumes_prepared_commit_proofs",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");
                    let sid = SessionId::new(63);
                    let (src_handle, src_lane) = attach_session_lane(cluster, src_id, sid);
                    let dst_lane = Lane::new(1);
                    let operands = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane,
                        dst_lane,
                        old_gen: Generation::ZERO,
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

                    let desc = ControlDesc::new(
                        EffIndex::MAX,
                        ControlDesc::STATIC_POLICY_SITE,
                        0x04A9,
                        TAG_TOPOLOGY_BEGIN_CONTROL,
                        ControlOp::TopologyCommit,
                        crate::global::const_dsl::ControlScopeKind::Topology,
                        ControlPath::Wire,
                        CapShot::One,
                        true,
                    );
                    let bytes =
                        topology_control_token(desc, sid, src_lane, topology_handle(operands));
                    let ticket = prepare_descriptor_commit(cluster, src_id, bytes, desc, 0)
                        .expect("reserve topology commit");
                    cluster.publish_descriptor_terminal(ticket);

                    assert!(
                        cluster.distributed_topology_operands(sid).is_none(),
                        "prepared commit publish must consume the distributed topology proof",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&src_id)
                            .expect("source rendezvous")
                            .session_lane(sid),
                        None,
                        "prepared source proof consumption must retire the source lane",
                    );
                    assert!(
                        cluster
                            .public_endpoint_header_ptr(src_id, src_handle.0, src_handle.1)
                            .is_none(),
                        "prepared source proof consumption must revoke the source endpoint",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&dst_id)
                            .expect("destination rendezvous")
                            .lane_generation(dst_lane),
                        operands.new_gen,
                        "prepared destination proof consumption must publish the destination generation",
                    );
                });
            });
        },
    );
}
