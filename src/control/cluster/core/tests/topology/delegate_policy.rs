use super::super::*;
use crate::control::cap::mint::{CapError, ControlPath};

#[test]
fn endpoint_delegate_identity_reads_validated_header_fields() {
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
        crate::control::cap::mint::EndpointResource::encode_identity(&handle),
    )
    .encode(&mut header);

    let token = GenericCapToken::<crate::control::cap::mint::EndpointResource>::from_raw_bytes(
        token_wire_image([0xAB; crate::control::cap::mint::CAP_NONCE_LEN], header),
    );
    let canonical = token
        .endpoint_identity()
        .expect("valid endpoint header must decode as canonical identity");
    assert_eq!(canonical.sid, handle.sid);
    assert_eq!(canonical.lane, handle.lane);
}

#[test]
fn endpoint_delegate_identity_rejects_noncanonical_headers() {
    fn endpoint_delegate_with_mutated_header(
        mutate: fn(&mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]),
    ) -> GenericCapToken<crate::control::cap::mint::EndpointResource> {
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
            crate::control::cap::mint::EndpointResource::encode_identity(&handle),
        )
        .encode(&mut header);
        mutate(&mut header);

        let token = GenericCapToken::<crate::control::cap::mint::EndpointResource>::from_raw_bytes(
            token_wire_image([0xAB; crate::control::cap::mint::CAP_NONCE_LEN], header),
        );

        token
    }

    fn mutate_tag(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[7] = <TestLoopContinueControl as crate::control::cap::mint::LocalControlKind>::TAG;
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
            .endpoint_identity()
            .expect_err("malformed endpoint header must be rejected");
        assert!(
            matches!(err, CapError),
            "{name} mutation must be rejected as invalid delegate token, got {err:?}",
        );
    }
}

#[test]
fn endpoint_delegate_identity_rejects_malformed_handle_payloads() {
    fn endpoint_delegate_with_mutated_handle(
        mutate: fn(&mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN]),
    ) -> GenericCapToken<crate::control::cap::mint::EndpointResource> {
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
            crate::control::cap::mint::EndpointResource::encode_identity(&handle),
        )
        .encode(&mut header);

        let handle_bytes = &mut header[crate::control::cap::mint::CAP_CONTROL_HEADER_FIXED_LEN
            ..crate::control::cap::mint::CAP_CONTROL_HEADER_FIXED_LEN
                + crate::control::cap::mint::CAP_HANDLE_LEN];
        let handle_bytes: &mut [u8; crate::control::cap::mint::CAP_HANDLE_LEN] = handle_bytes
            .try_into()
            .expect("endpoint handle payload must fit");
        mutate(handle_bytes);

        let token = GenericCapToken::<crate::control::cap::mint::EndpointResource>::from_raw_bytes(
            token_wire_image([0xAB; crate::control::cap::mint::CAP_NONCE_LEN], header),
        );

        token
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
            .endpoint_identity()
            .expect_err("malformed endpoint handle payload must be rejected");
        assert!(
            matches!(err, CapError),
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
                    .register_rendezvous(src_cfg, DummyTransport)
                    .expect("register src");
                let dst_id = cluster
                    .register_rendezvous(dst_cfg, DummyTransport)
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
                assert_eq!(
                    cluster.with_control_mut(|core| core.cached_operands_remove(sid0)),
                    Some(ops0)
                );
                assert_eq!(
                    cluster.with_control_mut(|core| core.cached_operands_remove(sid1)),
                    Some(ops1)
                );
                assert!(cluster.distributed_topology_operands(sid0).is_none());
                assert!(cluster.distributed_topology_operands(sid1).is_none());
            });
        });
    });
}

fn test_distributed_topology_entry(seq_tx: u32) -> DistributedEntry {
    let operands = TopologyOperands {
        src_rv: RendezvousId::new(1),
        dst_rv: RendezvousId::new(2),
        src_lane: Lane::new(3),
        dst_lane: Lane::new(4),
        old_gen: Generation::new(5),
        new_gen: Generation::new(6),
        seq_tx: seq_tx,
        seq_rx: 8,
    };
    let mut tap = crate::control::automaton::txn::NoopTap;
    let (txn, _) = DistributedTopology::begin(operands.intent(SessionId::new(0)), &mut tap);
    DistributedEntry {
        operands,
        phase: DistributedPhase::Begin { txn },
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

    let mut entry = bucket.remove(sid).expect("entry removable");
    entry.operands.seq_tx = 9;
    bucket
        .insert(sid, entry)
        .expect("reinsert uses untagged storage");
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
                    .register_rendezvous(src_cfg, DummyTransport)
                    .expect("register src");
                let dst_id = cluster
                    .register_rendezvous(dst_cfg, DummyTransport)
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

                    let reserved = core
                        .reserve_distributed_topology_begin_capacity(
                            sid0,
                            ops0,
                            core.locals.owner_proof(ops0.src_rv).expect("src owner"),
                        )
                        .expect("reserve src begin bucket");
                    let (ack0, begin0) =
                        core.publish_distributed_topology_begin(reserved, sid0, ops0);
                    core.topology_state.publish_prepared_begin(begin0);
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

                    let reserved = core
                        .reserve_distributed_topology_begin_capacity(
                            sid1,
                            ops1,
                            core.locals.owner_proof(ops1.src_rv).expect("dst owner"),
                        )
                        .expect("reserve dst begin bucket");
                    let (_ack1, begin1) =
                        core.publish_distributed_topology_begin(reserved, sid1, ops1);
                    core.topology_state.publish_prepared_begin(begin1);
                    assert!(
                        !core
                            .topology_state
                            .bucket(dst_id)
                            .expect("dst bucket bound")
                            .storage_ptr()
                            .is_null()
                    );

                    assert_eq!(ack0, ops0.ack(sid0));
                    core.topology_state
                        .preflight_ack(sid0, src_id, ack0)
                        .expect("ack src shard preflight");
                    let ack0_ticket = core
                        .topology_state
                        .reserve_preflighted_ack(sid0, src_id, ack0);
                    core.topology_state.publish_prepared_ack(ack0_ticket);
                    let commit0 = core
                        .topology_state
                        .reserve_commit(sid0, src_id, Some(ack0))
                        .expect("commit src shard");
                    core.topology_state.publish_prepared_commit(commit0);
                    assert_eq!(
                        core.topology_state.get(sid0).copied(),
                        None,
                        "distributed commit must consume the source entry"
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
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
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
                        let reserved = core
                            .reserve_distributed_topology_begin_capacity(
                                sid,
                                ops,
                                core.locals.owner_proof(ops.src_rv).expect("src owner"),
                            )
                            .expect("reserve src begin bucket");
                        let (ack, begin) =
                            core.publish_distributed_topology_begin(reserved, sid, ops);
                        core.topology_state.publish_prepared_begin(begin);
                        core.topology_state
                            .preflight_ack(sid, src_id, ack)
                            .expect("begin entry must be ready for ack");
                        let ack_ticket = core
                            .topology_state
                            .reserve_preflighted_ack(sid, src_id, ack);
                        core.topology_state.publish_prepared_ack(ack_ticket);

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
                        assert!(
                            matches!(
                                core.topology_state.reserve_commit(
                                    sid,
                                    src_id,
                                    Some(mismatched_ack)
                                ),
                                Err(CpError::Topology(TopologyError::CommitFailed))
                            ),
                            "commit mismatch must fail closed without consuming the entry",
                        );
                        assert_eq!(
                            core.topology_state.get(sid).copied(),
                            Some(ops),
                            "failed commit must preserve the distributed topology owner for retry",
                        );
                        assert_eq!(
                            core.topology_state
                                .reserve_commit(sid, src_id, Some(ack))
                                .map(|ticket| {
                                    core.topology_state.publish_prepared_commit(ticket);
                                    ops
                                }),
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
                        .register_rendezvous(src_cfg, DummyTransport)
                        .expect("register src");
                    let dst_id = cluster
                        .register_rendezvous(dst_cfg, DummyTransport)
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
                    assert_eq!(
                        cluster.with_control_mut(|core| core.cached_operands_remove(sid)),
                        Some(ops1)
                    );
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
            fn defer_resolution() -> Result<DecisionResolution, ResolverError> {
                Ok(DecisionResolution::Defer)
            }

            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .register_rendezvous(config, DummyTransport)
                        .expect("register rendezvous");

                    const POLICY_ID: u16 = 913;
                    let eff_index = EffIndex::from_dense_ordinal(7);
                    let policy = crate::global::const_dsl::ResolverMode::dynamic(POLICY_ID);

                    cluster
                        .register_dynamic_policy_resolver(
                            rv_id,
                            eff_index,
                            TAG_TOPOLOGY_BEGIN_CONTROL,
                            policy,
                            ControlOp::TopologyBegin,
                            ResolverRef::<POLICY_ID>::decision_fn(defer_resolution),
                        )
                        .expect_err("topology resolver must be rejected");
                });
            });
        },
    );
}

#[test]
fn dynamic_resolver_accepts_loop_decision_registration() {
    run_on_transient_compiled_test_stack(
        "dynamic_resolver_accepts_loop_decision_registration",
        || {
            fn decision_resolution() -> Result<DecisionResolution, ResolverError> {
                Ok(DecisionResolution::Arm(DecisionArm::Left))
            }

            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .register_rendezvous(config, DummyTransport)
                        .expect("register rendezvous");
                    const POLICY_ID: u16 = 914;
                    let policy = crate::global::const_dsl::ResolverMode::dynamic(POLICY_ID)
                        .with_scope(ScopeId::route(1));

                    let loop_eff = EffIndex::from_dense_ordinal(9);
                    let loop_tag = <crate::control::cap::resource_kinds::LoopContinueKind as crate::control::cap::mint::LocalControlKind>::TAG;
                    cluster
                        .register_dynamic_policy_resolver(
                            rv_id,
                            loop_eff,
                            loop_tag,
                            policy,
                            ControlOp::LoopContinue,
                            ResolverRef::<POLICY_ID>::decision_fn(decision_resolution),
                        )
                        .expect("loop control must use the same public decision resolver");

                    // Non-binary route arms are unrepresentable in the public
                    // resolver API; the resolver type can only select left or right.
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
                    let decision_policy_program_two = decision_policy_program_two();
                    let decision_policy_projected_two: SharedBorrowRoleProgram =
                        role_program::project(&decision_policy_program_two);
                    let rv_id = cluster
                        .register_rendezvous(config, DummyTransport)
                        .expect("register rendezvous");

                    cluster
                        .set_resolver::<ROUTE_POLICY_TWO, 0>(
                            rv_id,
                            &decision_policy_projected_two,
                            ResolverRef::decision_fn(route_resolver),
                        )
                        .expect("register resolver without a free cache slot");

                    let program_ref = decision_policy_projected_two
                        .compiled_role_image()
                        .program();
                    let site = program_ref
                        .dynamic_policy_sites_for(ROUTE_POLICY_TWO)
                        .next()
                        .expect("dynamic resolver site");
                    assert!(
                        cluster
                            .dynamic_resolver(DynamicResolverKey::new(
                                rv_id,
                                site.eff_index(),
                                site.op().expect("decision policy op")
                            ))
                            .is_some(),
                        "resolver registration must succeed from resident program metadata"
                    );
                });
            });
        },
    );
}
