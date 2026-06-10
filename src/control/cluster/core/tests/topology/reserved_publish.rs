use super::super::*;

#[test]
fn distributed_topology_reserved_publish_consumes_prepared_commit_proofs() {
    run_on_transient_compiled_test_stack(
        "distributed_topology_reserved_publish_consumes_prepared_commit_proofs",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
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
                    )
                    .expect("ack succeeds");

                    let desc = topology_control_desc(ControlOp::TopologyCommit);
                    let bytes =
                        topology_control_token(desc, sid, src_lane, topology_handle(operands));
                    let ticket = prepare_descriptor_commit(cluster, src_id, bytes, desc, 0)
                        .expect("reserve topology commit");
                    cluster.publish_descriptor_terminal(ticket);

                    assert!(
                        topology_state_operands(cluster, sid).is_none(),
                        "prepared commit publish must consume the distributed topology proof",
                    );
                    assert!(
                        !test_session_bound_to(cluster, src_id, sid, src_lane),
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
