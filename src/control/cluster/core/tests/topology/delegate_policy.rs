use super::super::*;

#[test]
fn prepare_reroute_handle_from_policy_rejects_out_of_domain_lane() {
    run_on_transient_compiled_test_stack(
        "prepare_reroute_handle_from_policy_rejects_out_of_domain_lane",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let err = cluster
                        .prepare_reroute_handle_from_policy(
                            src_id,
                            Lane::new(4),
                            EffIndex::from_dense_ordinal(10),
                            TAG_CAP_DELEGATE_CONTROL,
                            ControlOp::CapDelegate,
                            PolicyMode::Static,
                            [pack_u16_pair(dst_id.raw(), 256), 21, 22, 23],
                            &crate::transport::context::PolicyAttrs::EMPTY,
                        )
                        .expect_err("static cap-delegate input must reject lane 256");

                    assert_eq!(
                        err,
                        CpError::Authorisation {
                            operation: ControlOp::CapDelegate as u8
                        }
                    );
                });
            });
        },
    );
}

#[test]
fn canonicalize_delegate_reads_validated_endpoint_header_fields() {
    let handle = crate::control::cap::mint::EndpointHandle::new(
        SessionId::new(0x0102_0304),
        Lane::new(1),
        9,
    );
    let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
    crate::control::cap::mint::CapHeader::new(
        handle.sid,
        handle.lane,
        handle.role,
        crate::control::cap::mint::EndpointResource::TAG,
        ControlOp::Fence,
        ControlPath::Local,
        crate::control::cap::mint::CapShot::One,
        crate::global::const_dsl::ControlScopeKind::None,
        0,
        0,
        0,
        crate::control::cap::mint::EndpointResource::encode_handle(&handle),
    )
    .encode(&mut header);

    let token = GenericCapToken::<crate::control::cap::mint::EndpointResource>::from_bytes(
        token_wire_image([0xAB; crate::control::cap::mint::CAP_NONCE_LEN], header),
    );
    let command = CpCommand::new(ControlOp::CapDelegate).with_delegate(DelegateOperands {
        claim: false,
        token,
    });

    let canonical = command
        .canonicalize_delegate()
        .expect("valid endpoint header must canonicalize");
    assert_eq!(canonical.sid, Some(handle.sid));
    assert_eq!(canonical.lane, Some(handle.lane));
}

#[test]
fn canonicalize_delegate_rejects_noncanonical_endpoint_headers() {
    fn endpoint_delegate_with_mutated_header(
        mutate: fn(&mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]),
    ) -> CpCommand {
        let handle =
            crate::control::cap::mint::EndpointHandle::new(SessionId::new(7), Lane::new(1), 9);
        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        crate::control::cap::mint::CapHeader::new(
            handle.sid,
            handle.lane,
            handle.role,
            crate::control::cap::mint::EndpointResource::TAG,
            ControlOp::Fence,
            ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            crate::control::cap::mint::EndpointResource::encode_handle(&handle),
        )
        .encode(&mut header);
        mutate(&mut header);

        let token = GenericCapToken::<crate::control::cap::mint::EndpointResource>::from_bytes(
            token_wire_image([0xAB; crate::control::cap::mint::CAP_NONCE_LEN], header),
        );

        CpCommand::new(ControlOp::CapDelegate).with_delegate(DelegateOperands {
            claim: false,
            token,
        })
    }

    fn mutate_tag(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[7] = RouteDecisionKind::TAG;
    }

    fn mutate_op(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[8] = ControlOp::TopologyBegin.as_u8();
    }

    fn mutate_path(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[9] = ControlPath::Wire.as_u8();
    }

    fn mutate_shot(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[10] = crate::control::cap::mint::CapShot::Many.as_u8();
    }

    fn mutate_scope_kind(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[11] = crate::global::const_dsl::ControlScopeKind::Route as u8;
    }

    fn mutate_flags(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[12] = 0x01;
    }

    fn mutate_scope_id(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[13..15].copy_from_slice(&1u16.to_be_bytes());
    }

    fn mutate_epoch(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[15..17].copy_from_slice(&1u16.to_be_bytes());
    }

    let cases: &[(
        &str,
        fn(&mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]),
    )] = &[
        ("tag", mutate_tag),
        ("op", mutate_op),
        ("path", mutate_path),
        ("shot", mutate_shot),
        ("scope_kind", mutate_scope_kind),
        ("flags", mutate_flags),
        ("scope_id", mutate_scope_id),
        ("epoch", mutate_epoch),
    ];

    for (name, mutate) in cases {
        let err = endpoint_delegate_with_mutated_header(*mutate)
            .canonicalize_delegate()
            .expect_err("malformed endpoint header must be rejected");
        assert!(
            matches!(err, CpError::Delegation(DelegationError::InvalidToken)),
            "{name} mutation must be rejected as invalid delegate token, got {err:?}",
        );
    }
}

#[test]
fn canonicalize_delegate_rejects_malformed_endpoint_handle_payloads() {
    fn endpoint_delegate_with_mutated_handle(
        mutate: fn(&mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]),
    ) -> CpCommand {
        let handle =
            crate::control::cap::mint::EndpointHandle::new(SessionId::new(7), Lane::new(1), 9);
        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        crate::control::cap::mint::CapHeader::new(
            handle.sid,
            handle.lane,
            handle.role,
            crate::control::cap::mint::EndpointResource::TAG,
            ControlOp::Fence,
            ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            crate::control::cap::mint::EndpointResource::encode_handle(&handle),
        )
        .encode(&mut header);

        let handle_bytes = &mut header[crate::control::cap::mint::CAP_CONTROL_HEADER_FIXED_LEN
            ..crate::control::cap::mint::CAP_CONTROL_HEADER_FIXED_LEN
                + crate::control::cap::mint::CAP_HANDLE_LEN];
        let handle_bytes: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN] = handle_bytes
            .try_into()
            .expect("endpoint handle payload must fit");
        mutate(handle_bytes);

        let token = GenericCapToken::<crate::control::cap::mint::EndpointResource>::from_bytes(
            token_wire_image([0xAB; crate::control::cap::mint::CAP_NONCE_LEN], header),
        );

        CpCommand::new(ControlOp::CapDelegate).with_delegate(DelegateOperands {
            claim: false,
            token,
        })
    }

    fn mutate_sid(handle: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]) {
        handle[0] ^= 0x01;
    }

    fn mutate_lane(handle: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]) {
        handle[4] ^= 0x01;
    }

    fn mutate_role(handle: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]) {
        handle[5] ^= 0x01;
    }

    fn mutate_trailing_padding(handle: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]) {
        handle[6] = 0x7F;
    }

    let cases: &[(
        &str,
        fn(&mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]),
    )] = &[
        ("sid", mutate_sid),
        ("lane", mutate_lane),
        ("role", mutate_role),
        ("trailing_padding", mutate_trailing_padding),
    ];

    for (name, mutate) in cases {
        let err = endpoint_delegate_with_mutated_handle(*mutate)
            .canonicalize_delegate()
            .expect_err("malformed endpoint handle payload must be rejected");
        assert!(
            matches!(err, CpError::Delegation(DelegationError::InvalidToken)),
            "{name} mutation must be rejected as invalid delegate token, got {err:?}",
        );
    }
}

#[test]
fn cached_topology_operands_shard_by_source_rv() {
    run_on_transient_compiled_test_stack("cached_topology_operands_shard_by_source_rv", || {
        with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
            with_test_cluster_2(clock, |cluster| {
                let src_id = cluster
                    .add_rendezvous_from_config(src_cfg, DummyTransport)
                    .expect("register src");
                let dst_id = cluster
                    .add_rendezvous_from_config(dst_cfg, DummyTransport)
                    .expect("register dst");

                let sid0 = SessionId::new(7);
                let sid1 = SessionId::new(9);
                let ops0 = TopologyOperands {
                    src_rv: src_id,
                    dst_rv: dst_id,
                    src_lane: Lane::new(0),
                    dst_lane: Lane::new(1),
                    old_gen: Generation::new(0),
                    new_gen: Generation::new(1),
                    seq_tx: 0,
                    seq_rx: 0,
                };
                let ops1 = TopologyOperands {
                    src_rv: dst_id,
                    dst_rv: src_id,
                    src_lane: Lane::new(1),
                    dst_lane: Lane::new(0),
                    old_gen: Generation::new(2),
                    new_gen: Generation::new(3),
                    seq_tx: 1,
                    seq_rx: 1,
                };

                cluster
                    .cache_topology_operands(sid0, ops0)
                    .expect("cache first shard");
                cluster
                    .cache_topology_operands(sid1, ops1)
                    .expect("cache second shard");

                assert_eq!(cluster.distributed_topology_operands(sid0), Some(ops0));
                assert_eq!(cluster.distributed_topology_operands(sid1), Some(ops1));
                assert_eq!(cluster.take_cached_topology_operands(sid0), Some(ops0));
                assert_eq!(cluster.take_cached_topology_operands(sid1), Some(ops1));
                assert!(cluster.distributed_topology_operands(sid0).is_none());
                assert!(cluster.distributed_topology_operands(sid1).is_none());
            });
        });
    });
}

fn test_distributed_topology_entry(seq_tx: u32) -> DistributedEntry {
    DistributedEntry {
        operands: TopologyOperands {
            src_rv: RendezvousId::new(1),
            dst_rv: RendezvousId::new(2),
            src_lane: Lane::new(3),
            dst_lane: Lane::new(4),
            old_gen: Generation::new(5),
            new_gen: Generation::new(6),
            seq_tx: seq_tx,
            seq_rx: 8,
        },
        phase: DistributedPhase::Begin { txn: None },
    }
}

#[test]
fn distributed_topology_bucket_accesses_untagged_entries() {
    let capacity = 2usize;
    let layout = std::alloc::Layout::from_size_align(
        DistributedTopologyBucket::storage_bytes(capacity),
        DistributedTopologyBucket::storage_align(),
    )
    .expect("bucket storage layout");
    let storage = unsafe { std::alloc::alloc(layout) };
    if storage.is_null() {
        std::alloc::handle_alloc_error(layout);
    }

    let mut bucket = DistributedTopologyBucket::empty();
    let reclaim_delta = 1usize;
    assert!(
        DistributedTopologyBucket::STORAGE_TAG_MASK >= reclaim_delta,
        "test requires a non-zero reclaim tag bit"
    );

    unsafe {
        bucket.bind_from_storage(storage, capacity, reclaim_delta);
    }

    let entries = bucket.entries_ptr();
    assert_ne!(bucket.raw_entries().addr(), entries.addr());

    let sid = SessionId::new(17);
    bucket
        .insert(sid, test_distributed_topology_entry(7))
        .expect("insert uses untagged storage");

    let stored = unsafe { (&*entries).as_ref() }.expect("entry stored at untagged base");
    assert_eq!(stored.sid, sid);
    assert_eq!(stored.entry.operands.seq_tx, 7);
    assert_eq!(bucket.occupied_len(), 1);
    assert!(bucket.contains_sid(sid));
    assert_eq!(bucket.get(sid).map(|entry| entry.operands.seq_tx), Some(7));

    let entry = bucket.get_mut(sid).expect("mutable entry");
    entry.operands.seq_tx = 9;
    assert_eq!(
        unsafe {
            (&*entries)
                .as_ref()
                .map(|stored| stored.entry.operands.seq_tx)
        },
        Some(9)
    );

    let removed = bucket.remove(sid).expect("remove entry");
    assert_eq!(removed.operands.seq_tx, 9);
    assert_eq!(bucket.occupied_len(), 0);
    assert!(!bucket.contains_sid(sid));
    assert!(bucket.get(sid).is_none());

    unsafe {
        std::alloc::dealloc(storage, layout);
    }
}

#[test]
fn distributed_topology_state_binds_by_source_rv() {
    run_on_transient_compiled_test_stack("distributed_topology_state_binds_by_source_rv", || {
        with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
            with_test_cluster_2(clock, |cluster| {
                let src_id = cluster
                    .add_rendezvous_from_config(src_cfg, DummyTransport)
                    .expect("register src");
                let dst_id = cluster
                    .add_rendezvous_from_config(dst_cfg, DummyTransport)
                    .expect("register dst");

                let sid0 = SessionId::new(11);
                let sid1 = SessionId::new(13);
                let ops0 = TopologyOperands {
                    src_rv: src_id,
                    dst_rv: dst_id,
                    src_lane: Lane::new(0),
                    dst_lane: Lane::new(1),
                    old_gen: Generation::new(0),
                    new_gen: Generation::new(1),
                    seq_tx: 0,
                    seq_rx: 0,
                };
                let ops1 = TopologyOperands {
                    src_rv: dst_id,
                    dst_rv: src_id,
                    src_lane: Lane::new(1),
                    dst_lane: Lane::new(0),
                    old_gen: Generation::new(2),
                    new_gen: Generation::new(3),
                    seq_tx: 1,
                    seq_rx: 1,
                };

                cluster.with_control_mut(|core| {
                    assert!(
                        core.topology_state
                            .bucket(src_id)
                            .expect("src bucket")
                            .storage_ptr()
                            .is_null()
                    );
                    assert!(
                        core.topology_state
                            .bucket(dst_id)
                            .expect("dst bucket")
                            .storage_ptr()
                            .is_null()
                    );

                    core.ensure_distributed_topology_capacity(src_id, 1)
                        .expect("bind src bucket");
                    core.topology_state.begin(sid0, ops0).expect("begin src");
                    assert!(
                        !core
                            .topology_state
                            .bucket(src_id)
                            .expect("src bucket bound")
                            .storage_ptr()
                            .is_null()
                    );
                    assert!(
                        core.topology_state
                            .bucket(dst_id)
                            .expect("dst bucket still unbound")
                            .storage_ptr()
                            .is_null()
                    );

                    core.ensure_distributed_topology_capacity(dst_id, 1)
                        .expect("bind dst bucket");
                    core.topology_state.begin(sid1, ops1).expect("begin dst");
                    assert!(
                        !core
                            .topology_state
                            .bucket(dst_id)
                            .expect("dst bucket bound")
                            .storage_ptr()
                            .is_null()
                    );

                    let ack0 = core
                        .topology_state
                        .acknowledge(sid0, src_id)
                        .expect("ack src shard");
                    assert_eq!(ack0, ops0.ack(sid0));
                    assert_eq!(
                        core.topology_state
                            .topology_commit(sid0, src_id, Some(ack0)),
                        Ok(ops0)
                    );
                    assert_eq!(core.topology_state.get(sid1).copied(), Some(ops1));
                });

                assert!(cluster.distributed_topology_operands(sid0).is_none());
                assert_eq!(cluster.distributed_topology_operands(sid1), Some(ops1));
            });
        });
    });
}

#[test]
fn distributed_topology_commit_mismatch_preserves_entry_for_retry() {
    run_on_transient_compiled_test_stack(
        "distributed_topology_commit_mismatch_preserves_entry_for_retry",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(29);
                    let ops = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: Lane::new(0),
                        dst_lane: Lane::new(1),
                        old_gen: Generation::new(0),
                        new_gen: Generation::new(1),
                        seq_tx: 0,
                        seq_rx: 0,
                    };

                    cluster.with_control_mut(|core| {
                        core.ensure_distributed_topology_capacity(src_id, 1)
                            .expect("bind src bucket");
                        let (_intent, ack) =
                            core.topology_state.begin(sid, ops).expect("begin topology");
                        assert_eq!(
                            core.topology_state.acknowledge(sid, src_id),
                            Ok(ack),
                            "begin entry must advance to acked phase before commit",
                        );

                        let mismatched_ack = TopologyAck {
                            src_rv: ops.src_rv,
                            dst_rv: ops.dst_rv,
                            sid: sid.raw(),
                            new_gen: Generation::new(2),
                            src_lane: ops.src_lane,
                            new_lane: ops.dst_lane,
                            seq_tx: ops.seq_tx,
                            seq_rx: ops.seq_rx,
                        };
                        assert_eq!(
                            core.topology_state
                                .topology_commit(sid, src_id, Some(mismatched_ack)),
                            Err(CpError::Topology(TopologyError::CommitFailed)),
                            "commit mismatch must fail closed without consuming the entry",
                        );
                        assert_eq!(
                            core.topology_state.get(sid).copied(),
                            Some(ops),
                            "failed commit must preserve the distributed topology owner for retry",
                        );
                        assert_eq!(
                            core.topology_state.topology_commit(sid, src_id, Some(ack)),
                            Ok(ops),
                            "correct commit must still succeed after the rejected attempt",
                        );
                    });
                });
            });
        },
    );
}

#[test]
fn cached_topology_operands_replace_same_session_across_rendezvous_shards() {
    run_on_transient_compiled_test_stack(
        "cached_topology_operands_replace_same_session_across_rendezvous_shards",
        || {
            with_cluster_fixture_pair(|clock, src_cfg, dst_cfg| {
                with_test_cluster_2(clock, |cluster| {
                    let src_id = cluster
                        .add_rendezvous_from_config(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .add_rendezvous_from_config(dst_cfg, DummyTransport)
                        .expect("register dst");

                    let sid = SessionId::new(23);
                    let ops0 = TopologyOperands {
                        src_rv: src_id,
                        dst_rv: dst_id,
                        src_lane: Lane::new(0),
                        dst_lane: Lane::new(1),
                        old_gen: Generation::new(0),
                        new_gen: Generation::new(1),
                        seq_tx: 0,
                        seq_rx: 0,
                    };
                    let ops1 = TopologyOperands {
                        src_rv: dst_id,
                        dst_rv: src_id,
                        src_lane: Lane::new(1),
                        dst_lane: Lane::new(0),
                        old_gen: Generation::new(2),
                        new_gen: Generation::new(3),
                        seq_tx: 1,
                        seq_rx: 1,
                    };

                    cluster
                        .cache_topology_operands(sid, ops0)
                        .expect("cache first shard");
                    assert_eq!(cluster.distributed_topology_operands(sid), Some(ops0));

                    cluster
                        .cache_topology_operands(sid, ops1)
                        .expect("replace cached operands on second shard");

                    assert_eq!(
                        cluster.distributed_topology_operands(sid),
                        Some(ops1),
                        "same-session cached topology operands must stay globally unique across rendezvous shards"
                    );
                    assert_eq!(cluster.take_cached_topology_operands(sid), Some(ops1));
                    assert!(cluster.distributed_topology_operands(sid).is_none());
                });
            });
        },
    );
}

#[test]
fn register_dynamic_resolver_rejects_topology_and_reroute_ops() {
    run_on_transient_compiled_test_stack(
        "register_dynamic_resolver_rejects_topology_and_reroute_ops",
        || {
            fn defer_resolution(_ctx: ResolverContext) -> Result<RouteResolution, ResolverError> {
                Ok(RouteResolution::Defer)
            }

            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");

                    let policy_id = 913u16;
                    let eff_index = EffIndex::from_dense_ordinal(7);
                    let policy = crate::global::const_dsl::PolicyMode::dynamic(policy_id);

                    cluster
                        .register_dynamic_policy_resolver(
                            rv_id,
                            eff_index,
                            TAG_TOPOLOGY_BEGIN_CONTROL,
                            policy,
                            TAG_TOPOLOGY_BEGIN_CONTROL,
                            ControlOp::TopologyBegin,
                            None,
                            ResolverRef::route_fn(defer_resolution),
                        )
                        .expect_err("topology resolver must be rejected");
                    cluster
                        .register_dynamic_policy_resolver(
                            rv_id,
                            eff_index,
                            TAG_CAP_DELEGATE_CONTROL,
                            policy,
                            TAG_CAP_DELEGATE_CONTROL,
                            ControlOp::CapDelegate,
                            None,
                            ResolverRef::route_fn(defer_resolution),
                        )
                        .expect_err("reroute resolver must be rejected");
                });
            });
        },
    );
}

#[test]
fn dynamic_resolver_rejects_cross_semantic_registration() {
    run_on_transient_compiled_test_stack(
        "dynamic_resolver_rejects_cross_semantic_registration",
        || {
            fn loop_resolution(_ctx: ResolverContext) -> Result<LoopResolution, ResolverError> {
                Ok(LoopResolution::Break)
            }

            fn route_resolution(_ctx: ResolverContext) -> Result<RouteResolution, ResolverError> {
                Ok(RouteResolution::Arm(0))
            }

            fn non_binary_route_resolution(
                _ctx: ResolverContext,
            ) -> Result<RouteResolution, ResolverError> {
                Ok(RouteResolution::Arm(2))
            }

            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");
                    let policy = crate::global::const_dsl::PolicyMode::dynamic(914)
                        .with_scope(ScopeId::route(1));
                    let eff_index = EffIndex::from_dense_ordinal(8);
                    let tag = crate::control::cap::resource_kinds::RouteDecisionKind::TAG;

                    cluster
                        .register_dynamic_policy_resolver(
                            rv_id,
                            eff_index,
                            tag,
                            policy,
                            tag,
                            ControlOp::RouteDecision,
                            None,
                            ResolverRef::loop_fn(loop_resolution),
                        )
                        .expect_err("route decision must reject loop resolver type");

                    let loop_eff = EffIndex::from_dense_ordinal(9);
                    let loop_tag = crate::control::cap::resource_kinds::LoopContinueKind::TAG;
                    cluster
                        .register_dynamic_policy_resolver(
                            rv_id,
                            loop_eff,
                            loop_tag,
                            policy,
                            loop_tag,
                            ControlOp::LoopContinue,
                            None,
                            ResolverRef::route_fn(route_resolution),
                        )
                        .expect_err("loop control must reject route resolver type");

                    let non_binary_eff = EffIndex::from_dense_ordinal(10);
                    cluster
                        .register_dynamic_policy_resolver(
                            rv_id,
                            non_binary_eff,
                            tag,
                            policy,
                            tag,
                            ControlOp::RouteDecision,
                            None,
                            ResolverRef::route_fn(non_binary_route_resolution),
                        )
                        .expect("register non-binary route resolver");
                    assert!(
                        matches!(
                            cluster.resolve_dynamic_policy(
                                rv_id,
                                None,
                                Lane::new(1),
                                non_binary_eff,
                                tag,
                                ControlOp::RouteDecision,
                                [0; 4],
                                &crate::transport::context::PolicyAttrs::EMPTY,
                            ),
                            Err(CpError::PolicyAbort { reason: 914 })
                        ),
                        "route decision must reject non-binary route arms"
                    );
                });
            });
        },
    );
}

#[test]
fn set_resolver_registers_dynamic_policy_sites_without_resident_cache() {
    run_on_transient_compiled_test_stack(
        "set_resolver_registers_dynamic_policy_sites_without_resident_cache",
        || {
            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let route_policy_program_two = route_policy_program_two();
                    let route_policy_projected_two: SharedBorrowRoleProgram =
                        role_program::project(&route_policy_program_two);
                    let rv_id = cluster
                        .add_rendezvous_from_config(config, DummyTransport)
                        .expect("register rendezvous");

                    cluster
                        .set_resolver::<ROUTE_POLICY_TWO, 0>(
                            rv_id,
                            &route_policy_projected_two,
                            ResolverRef::route_fn(route_resolver),
                        )
                        .expect("register resolver without a free cache slot");

                    let program_ref = route_policy_projected_two.compiled_role_image().program();
                    let site = program_ref
                        .dynamic_policy_sites_for(ROUTE_POLICY_TWO)
                        .next()
                        .expect("dynamic policy site");
                    assert!(
                        cluster
                            .dynamic_resolver(DynamicResolverKey::new(
                                rv_id,
                                site.eff_index(),
                                site.op().expect("route policy op")
                            ))
                            .is_some(),
                        "resolver registration must succeed from resident program metadata"
                    );
                });
            });
        },
    );
}
