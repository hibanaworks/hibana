use super::*;

fn publish_state_snapshot<const MAX_RV: usize>(
    cluster: &StaticTestCluster<MAX_RV>,
    rv_id: RendezvousId,
    sid: SessionId,
    lane: Lane,
) -> Generation {
    let rv = cluster.get_local(&rv_id).expect("registered rendezvous");
    let generation = rv.lane_generation(lane);
    let proof = rv
        .prepare_state_snapshot_effect(sid, lane, generation)
        .expect("snapshot proof must match the current lane generation");
    rv.publish_prepared_state_snapshot_effect(proof);
    generation
}

fn publish_tx_commit<const MAX_RV: usize>(
    cluster: &StaticTestCluster<MAX_RV>,
    rv_id: RendezvousId,
    sid: SessionId,
    lane: Lane,
    generation: Generation,
) -> Result<(), crate::rendezvous::error::TxCommitError> {
    let rv = cluster.get_local(&rv_id).expect("registered rendezvous");
    let proof = rv.prepare_tx_commit_effect(sid, lane, generation)?;
    rv.publish_prepared_tx_commit_effect(proof);
    Ok(())
}

#[test]
fn descriptor_control_header_accepts_exact_match() {
    let (desc, header) = route_decision_header(3, 11, 0);
    StaticTestCluster::<1>::verify_control_header(desc, header, 3, 11)
        .expect("exact descriptor/header match must verify");
}

#[test]
fn descriptor_control_header_rejects_flags_scope_and_epoch_mismatch() {
    let (desc, header) = route_decision_header(
        3,
        11,
        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
            RouteDecisionKind,
        >())
        .header_flags(),
    );
    assert!(
        matches!(
            StaticTestCluster::<1>::verify_control_header(
                desc,
                CapHeader::new(
                    header.sid(),
                    header.lane(),
                    header.role(),
                    header.tag(),
                    header.op(),
                    header.path(),
                    header.shot(),
                    header.scope_kind(),
                    header.flags() | 0x80,
                    header.scope_id(),
                    header.epoch(),
                    *header.handle(),
                ),
                3,
                11,
            ),
            Err(CpError::Authorisation { operation })
                if operation == desc.op() as u8
        ),
        "reserved flag bits must fail closed",
    );
    assert!(
        matches!(
            StaticTestCluster::<1>::verify_control_header(desc, header, 4, 11),
            Err(CpError::Authorisation { operation })
                if operation == desc.op() as u8
        ),
        "scope mismatch must fail closed",
    );
    assert!(
        matches!(
            StaticTestCluster::<1>::verify_control_header(desc, header, 3, 12),
            Err(CpError::Authorisation { operation })
                if operation == desc.op() as u8
        ),
        "epoch mismatch must fail closed",
    );
}

#[test]
fn descriptor_control_header_rejects_tag_op_path_and_shot_mismatch() {
    let (desc, header) = route_decision_header(
        3,
        11,
        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
            RouteDecisionKind,
        >())
        .header_flags(),
    );

    for mismatched in [
        CapHeader::new(
            header.sid(),
            header.lane(),
            header.role(),
            header.tag().wrapping_add(1),
            header.op(),
            header.path(),
            header.shot(),
            header.scope_kind(),
            header.flags(),
            header.scope_id(),
            header.epoch(),
            *header.handle(),
        ),
        CapHeader::new(
            header.sid(),
            header.lane(),
            header.role(),
            header.tag(),
            ControlOp::LoopContinue,
            header.path(),
            header.shot(),
            header.scope_kind(),
            header.flags(),
            header.scope_id(),
            header.epoch(),
            *header.handle(),
        ),
        CapHeader::new(
            header.sid(),
            header.lane(),
            header.role(),
            header.tag(),
            header.op(),
            crate::control::cap::mint::ControlPath::Wire,
            header.shot(),
            header.scope_kind(),
            header.flags(),
            header.scope_id(),
            header.epoch(),
            *header.handle(),
        ),
        CapHeader::new(
            header.sid(),
            header.lane(),
            header.role(),
            header.tag(),
            header.op(),
            header.path(),
            crate::control::cap::mint::CapShot::Many,
            header.scope_kind(),
            header.flags(),
            header.scope_id(),
            header.epoch(),
            *header.handle(),
        ),
    ] {
        assert!(
            matches!(
                StaticTestCluster::<1>::verify_control_header(desc, mismatched, 3, 11),
                Err(CpError::Authorisation { operation })
                    if operation == desc.op() as u8
            ),
            "descriptor/header mismatch must fail closed",
        );
    }
}

#[test]
fn no_handle_decode_in_core_auth() {
    let desc =
        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<DecodePoisonKind>());
    let header = CapHeader::new(
        SessionId::new(7),
        Lane::new(0),
        0,
        desc.resource_tag(),
        desc.op(),
        desc.path(),
        desc.shot(),
        desc.scope_kind(),
        desc.header_flags(),
        3,
        11,
        <DecodePoisonKind as LocalControlKind>::encode_local_handle(
            SessionId::new(7),
            Lane::new(0),
            ScopeId::none(),
        ),
    );

    StaticTestCluster::<1>::verify_control_header(desc, header, 3, 11)
        .expect("core auth must not decode the typed handle");
}

#[test]
fn local_descriptor_tx_commit_uses_header_snapshot_generation() {
    run_on_transient_compiled_test_stack(
        "local_descriptor_tx_commit_uses_header_snapshot_generation",
        || {
            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(41);
                    let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                    let snapshot = publish_state_snapshot(cluster, rv_id, sid, lane);
                    assert_eq!(snapshot, Generation::new(3));

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    let bytes = session_lane_control_token_with_epoch::<LocalTxCommitControl>(
                        sid,
                        lane,
                        snapshot.raw(),
                    );
                    let result = dispatch_prepared_descriptor_commit(
                        cluster,
                        rv_id,
                        bytes,
                        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
                            LocalTxCommitControl,
                        >()),
                        snapshot.raw(),
                    );
                    assert!(
                        result.is_ok(),
                        "local descriptor tx commit must execute against the header snapshot generation",
                    );
                    assert!(
                        matches!(
                            publish_tx_commit(cluster, rv_id, sid, lane, snapshot),
                            Err(crate::rendezvous::error::TxCommitError::AlreadyFinalized {
                                sid: err_sid,
                            }) if err_sid == sid
                        ),
                        "descriptor tx commit must finalize the recorded snapshot",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn local_descriptor_tx_commit_rejects_stale_header_snapshot_generation() {
    run_on_transient_compiled_test_stack(
        "local_descriptor_tx_commit_rejects_stale_header_snapshot_generation",
        || {
            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(45);
                    let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                    let stale_snapshot = publish_state_snapshot(cluster, rv_id, sid, lane);
                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));
                    let current_snapshot = publish_state_snapshot(cluster, rv_id, sid, lane);

                    let bytes = session_lane_control_token_with_epoch::<LocalTxCommitControl>(
                        sid,
                        lane,
                        stale_snapshot.raw(),
                    );
                    let err = match prepare_descriptor_commit(
                        cluster,
                        rv_id,
                        bytes,
                        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
                            LocalTxCommitControl,
                        >()),
                        stale_snapshot.raw(),
                    ) {
                        Ok(_) => panic!("stale descriptor epoch must not commit current snapshot"),
                        Err(err) => err,
                    };
                    assert_eq!(
                        err,
                        CpError::GenerationViolation {
                            expected: stale_snapshot.raw(),
                            actual: current_snapshot.raw(),
                        }
                    );

                    publish_tx_commit(cluster, rv_id, sid, lane, current_snapshot)
                        .expect("rejected stale descriptor must leave current snapshot available");

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn local_descriptor_state_restore_uses_header_snapshot_generation() {
    run_on_transient_compiled_test_stack(
        "local_descriptor_state_restore_uses_header_snapshot_generation",
        || {
            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(42);
                    let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                    let snapshot = publish_state_snapshot(cluster, rv_id, sid, lane);
                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    let bytes = session_lane_control_token_with_epoch::<LocalStateRestoreControl>(
                        sid,
                        lane,
                        snapshot.raw(),
                    );
                    let result = dispatch_prepared_descriptor_commit(
                        cluster,
                        rv_id,
                        bytes,
                        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
                            LocalStateRestoreControl,
                        >()),
                        snapshot.raw(),
                    );
                    assert!(
                        result.is_ok(),
                        "local descriptor state restore must execute against the header snapshot generation",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .lane_generation(lane),
                        snapshot,
                        "state restore must rewind to the recorded snapshot generation",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn local_descriptor_tx_abort_uses_header_snapshot_generation() {
    run_on_transient_compiled_test_stack(
        "local_descriptor_tx_abort_uses_header_snapshot_generation",
        || {
            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(43);
                    let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                    let snapshot = publish_state_snapshot(cluster, rv_id, sid, lane);
                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    let bytes = session_lane_control_token_with_epoch::<LocalTxAbortControl>(
                        sid,
                        lane,
                        snapshot.raw(),
                    );
                    let result = dispatch_prepared_descriptor_commit(
                        cluster,
                        rv_id,
                        bytes,
                        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
                            LocalTxAbortControl,
                        >()),
                        snapshot.raw(),
                    );
                    assert!(
                        result.is_ok(),
                        "local descriptor tx abort must execute against the header snapshot generation",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .lane_generation(lane),
                        snapshot,
                        "tx abort must rewind to the recorded snapshot generation",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn local_descriptor_abort_ack_uses_header_lane_generation() {
    run_on_transient_compiled_test_stack(
        "local_descriptor_abort_ack_uses_header_lane_generation",
        || {
            with_cluster_runtime(|fixture| {
                let config = fixture.config0();
                let tap = unsafe { &*fixture.tap0 };
                with_test_cluster_1(fixture.clock(), |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(43);
                    let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    let bytes =
                        session_lane_control_token_with_epoch::<LocalAbortAckControl>(sid, lane, 7);
                    let result = dispatch_prepared_descriptor_commit(
                        cluster,
                        rv_id,
                        bytes,
                        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
                            LocalAbortAckControl,
                        >()),
                        7,
                    );
                    assert!(
                        result.is_ok(),
                        "local descriptor abort ack must execute against the header lane generation",
                    );
                    assert!(
                        tap.iter().any(|event| {
                            event.id == crate::observe::ids::ABORT_ACK
                                && event.arg0 == sid.raw()
                                && event.arg1 == 7
                        }),
                        "abort ack tap payload must carry the authoritative lane generation",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn local_descriptor_abort_ack_rejects_stale_header_lane_generation() {
    run_on_transient_compiled_test_stack(
        "local_descriptor_abort_ack_rejects_stale_header_lane_generation",
        || {
            with_cluster_runtime(|fixture| {
                let config = fixture.config0();
                let tap = unsafe { &*fixture.tap0 };
                with_test_cluster_1(fixture.clock(), |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(46);
                    let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                    let bytes =
                        session_lane_control_token_with_epoch::<LocalAbortAckControl>(sid, lane, 3);
                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    let err = match prepare_descriptor_commit(
                        cluster,
                        rv_id,
                        bytes,
                        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
                            LocalAbortAckControl,
                        >()),
                        3,
                    ) {
                        Ok(_) => panic!("stale descriptor epoch must not execute abort ack"),
                        Err(err) => err,
                    };
                    assert_eq!(
                        err,
                        CpError::GenerationViolation {
                            expected: 3,
                            actual: 7,
                        }
                    );
                    assert!(
                        !tap.iter().any(|event| {
                            event.id == crate::observe::ids::ABORT_ACK && event.arg0 == sid.raw()
                        }),
                        "stale descriptor epoch must fail before abort ack tap emission",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn prepared_abort_ack_consumes_prepared_generation_after_drift() {
    run_on_transient_compiled_test_stack(
        "prepared_abort_ack_consumes_prepared_generation_after_drift",
        || {
            with_cluster_runtime(|fixture| {
                let config = fixture.config0();
                let tap = unsafe { &*fixture.tap0 };
                with_test_cluster_1(fixture.clock(), |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(48);
                    let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                    let bytes =
                        session_lane_control_token_with_epoch::<LocalAbortAckControl>(sid, lane, 3);
                    let ticket = prepare_descriptor_commit(
                        cluster,
                        rv_id,
                        bytes,
                        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
                            LocalAbortAckControl,
                        >()),
                        3,
                    )
                    .expect("prepare abort ack at current generation");
                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    cluster.publish_descriptor_terminal(ticket);

                    assert!(
                        tap.iter().any(|event| {
                            event.id == crate::observe::ids::ABORT_ACK
                                && event.arg0 == sid.raw()
                                && event.arg1 == 3
                        }),
                        "prepared abort ack must consume the prepared generation proof",
                    );
                    assert_eq!(
                        cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .session_fault(sid),
                        None,
                        "prepared abort ack publish must not revalidate after proof prepare",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn prepared_state_snapshot_consumes_prepared_generation_after_drift() {
    run_on_transient_compiled_test_stack(
        "prepared_state_snapshot_consumes_prepared_generation_after_drift",
        || {
            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(49);
                    let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                    let bytes = session_lane_control_token_with_epoch::<LocalStateSnapshotControl>(
                        sid, lane, 3,
                    );
                    let ticket = prepare_descriptor_commit(
                        cluster,
                        rv_id,
                        bytes,
                        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
                            LocalStateSnapshotControl,
                        >()),
                        3,
                    )
                    .expect("prepare state snapshot at current generation");
                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    cluster.publish_descriptor_terminal(ticket);

                    let rv = cluster.get_local(&rv_id).expect("registered rendezvous");
                    assert_eq!(
                        rv.snapshot_generation(lane),
                        Some(Generation::new(3)),
                        "prepared state snapshot must record the prepared generation proof",
                    );
                    assert_eq!(
                        rv.session_fault(sid),
                        None,
                        "prepared state snapshot publish must not revalidate after proof prepare",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                    }
                });
            });
        },
    );
}

#[test]
fn local_descriptor_state_snapshot_rejects_stale_header_lane_generation() {
    run_on_transient_compiled_test_stack(
        "local_descriptor_state_snapshot_rejects_stale_header_lane_generation",
        || {
            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let sid = SessionId::new(47);
                    let (endpoint_handle, lane) = attach_session_lane(cluster, rv_id, sid);

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(3));
                    let bytes = session_lane_control_token_with_epoch::<LocalStateSnapshotControl>(
                        sid, lane, 3,
                    );
                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    let err = match prepare_descriptor_commit(
                        cluster,
                        rv_id,
                        bytes,
                        ControlDesc::from_static(crate::global::StaticControlDesc::of_local::<
                            LocalStateSnapshotControl,
                        >()),
                        3,
                    ) {
                        Ok(_) => panic!("stale descriptor epoch must not snapshot current state"),
                        Err(err) => err,
                    };
                    assert_eq!(
                        err,
                        CpError::GenerationViolation {
                            expected: 3,
                            actual: 7,
                        }
                    );
                    assert_eq!(
                        cluster
                            .get_local(&rv_id)
                            .expect("registered rendezvous")
                            .snapshot_generation(lane),
                        None,
                        "stale descriptor epoch must fail before recording a snapshot",
                    );

                    unsafe {
                        drop_test_public_endpoint(cluster, rv_id, endpoint_handle);
                    }
                });
            });
        },
    );
}
