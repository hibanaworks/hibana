use super::*;

#[test]
fn descriptor_control_header_accepts_exact_match() {
    let (desc, header) = route_decision_header(3, 11, 0);
    StaticTestCluster::<1>::verify_control_header(desc, header, 3, 11)
        .expect("exact descriptor/header match must verify");
}

#[test]
fn descriptor_control_header_rejects_flags_scope_and_epoch_mismatch() {
    let (desc, header) =
        route_decision_header(3, 11, ControlDesc::of::<RouteDecisionKind>().header_flags());
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
    let (desc, header) =
        route_decision_header(3, 11, ControlDesc::of::<RouteDecisionKind>().header_flags());

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
    let desc = ControlDesc::of::<DecodePoisonKind>();
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
        DecodePoisonKind::encode_handle(&()),
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
                    let snapshot = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .state_snapshot_at_lane(sid, lane);
                    assert_eq!(snapshot, Generation::new(3));

                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    let bytes = session_lane_control_token_with_epoch::<LocalTxCommitControl>(
                        sid,
                        lane,
                        snapshot.raw(),
                    );
                    let result = cluster.dispatch_descriptor_control_frame(
                        rv_id,
                        bytes,
                        ControlDesc::of::<LocalTxCommitControl>(),
                        0,
                        snapshot.raw(),
                        None,
                    );
                    assert!(
                        result.is_ok(),
                        "local descriptor tx commit must execute against the header snapshot generation",
                    );
                    assert!(
                        matches!(
                            cluster
                                .get_local(&rv_id)
                                .expect("registered rendezvous")
                                .tx_commit_at_lane(sid, lane, snapshot),
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
                    let stale_snapshot = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .state_snapshot_at_lane(sid, lane);
                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));
                    let current_snapshot = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .state_snapshot_at_lane(sid, lane);

                    let bytes = session_lane_control_token_with_epoch::<LocalTxCommitControl>(
                        sid,
                        lane,
                        stale_snapshot.raw(),
                    );
                    let err = cluster
                        .dispatch_descriptor_control_frame(
                            rv_id,
                            bytes,
                            ControlDesc::of::<LocalTxCommitControl>(),
                            0,
                            stale_snapshot.raw(),
                            None,
                        )
                        .expect_err("stale descriptor epoch must not commit current snapshot");
                    assert!(matches!(
                        err,
                        CpError::TxCommit(TxCommitError::GenerationMismatch)
                    ));

                    cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .tx_commit_at_lane(sid, lane, current_snapshot)
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
                    let snapshot = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .state_snapshot_at_lane(sid, lane);
                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    let bytes = session_lane_control_token_with_epoch::<LocalStateRestoreControl>(
                        sid,
                        lane,
                        snapshot.raw(),
                    );
                    let result = cluster.dispatch_descriptor_control_frame(
                        rv_id,
                        bytes,
                        ControlDesc::of::<LocalStateRestoreControl>(),
                        0,
                        snapshot.raw(),
                        None,
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
                    let snapshot = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .state_snapshot_at_lane(sid, lane);
                    advance_lane_generation(cluster, rv_id, lane, Generation::new(7));

                    let bytes = session_lane_control_token_with_epoch::<LocalTxAbortControl>(
                        sid,
                        lane,
                        snapshot.raw(),
                    );
                    let result = cluster.dispatch_descriptor_control_frame(
                        rv_id,
                        bytes,
                        ControlDesc::of::<LocalTxAbortControl>(),
                        0,
                        snapshot.raw(),
                        None,
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
                    let result = cluster.dispatch_descriptor_control_frame(
                        rv_id,
                        bytes,
                        ControlDesc::of::<LocalAbortAckControl>(),
                        0,
                        7,
                        None,
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

                    let err = cluster
                        .dispatch_descriptor_control_frame(
                            rv_id,
                            bytes,
                            ControlDesc::of::<LocalAbortAckControl>(),
                            0,
                            3,
                            None,
                        )
                        .expect_err("stale descriptor epoch must not execute abort ack");
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

                    let err = cluster
                        .dispatch_descriptor_control_frame(
                            rv_id,
                            bytes,
                            ControlDesc::of::<LocalStateSnapshotControl>(),
                            0,
                            3,
                            None,
                        )
                        .expect_err("stale descriptor epoch must not snapshot current state");
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

#[test]
fn descriptor_cap_delegate_fails_closed() {
    run_on_transient_compiled_test_stack("descriptor_cap_delegate_fails_closed", || {
        with_cluster_fixture(|clock, config| {
            with_test_cluster_1(clock, |cluster| {
                let rv_id = cluster
                    .add_rendezvous_from_config(config, DummyTransport)
                    .expect("register rendezvous");
                let sid = SessionId::new(44);
                let lane = Lane::new(0);

                let bytes = session_lane_control_token::<WireCapDelegateControl>(sid, lane);
                let err = cluster
                    .dispatch_descriptor_control_frame(
                        rv_id,
                        bytes,
                        ControlDesc::of::<WireCapDelegateControl>(),
                        0,
                        0,
                        None,
                    )
                    .expect_err("descriptor cap-delegate must fail closed");
                assert_eq!(
                    err,
                    CpError::UnsupportedEffect(ControlOp::CapDelegate as u8)
                );
            });
        });
    });
}
