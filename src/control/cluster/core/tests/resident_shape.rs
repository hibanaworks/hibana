use super::*;
use crate::control::automaton::{delegation::DelegationLeaseSpec, topology::TopologyLeaseSpec};
use crate::control::lease::graph::LeaseGraph;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MeasuredResidentShape {
    route_scope_count: usize,
    active_lane_count: usize,
    max_route_stack_depth: usize,
    max_loop_stack_depth: usize,
    route_bytes: usize,
    loop_bytes: usize,
    cap_bytes: usize,
    endpoint_bytes: usize,
    endpoint_header_bytes: usize,
    endpoint_port_slots_bytes: usize,
    endpoint_guard_slots_bytes: usize,
    endpoint_header_padding_bytes: usize,
    resident_program_descriptor_bytes: usize,
    resident_role_descriptor_bytes: usize,
    endpoint_phase_cursor_state_bytes: usize,
    endpoint_route_state_bytes: usize,
    endpoint_route_arm_stack_bytes: usize,
    endpoint_lane_offer_state_slots_bytes: usize,
    endpoint_frontier_state_bytes: usize,
    endpoint_frontier_root_rows_bytes: usize,
    endpoint_frontier_root_active_slots_bytes: usize,
    endpoint_frontier_root_observed_key_slots_bytes: usize,
    endpoint_frontier_offer_entry_slots_bytes: usize,
    endpoint_binding_inbox_bytes: usize,
    endpoint_binding_slots_bytes: usize,
    endpoint_binding_len_bytes: usize,
    endpoint_binding_frame_label_masks_bytes: usize,
    endpoint_scope_evidence_store_bytes: usize,
    endpoint_scope_evidence_slots_bytes: usize,
    endpoint_padding_bytes: usize,
}

fn measure_huge_shape<const ROLE: u8>(
    projected: &role_program::RoleProgram<ROLE>,
) -> MeasuredResidentShape {
    with_cluster_fixture(|clock, config| {
        with_test_cluster_1(clock, |cluster| {
            let rv_id = cluster
                .add_rendezvous_from_config(config, DummyTransport)
                .expect("register rendezvous");
            let role_image = cluster
                .resident_test_role_image::<ROLE, _>(rv_id, projected)
                .expect("construct resident role image");
            let active_lane_count = role_image.active_lane_count();
            let endpoint_layout = role_image.endpoint_arena_layout_for_binding(false);
            let endpoint_storage =
                StaticTestCluster::<1>::public_endpoint_storage_requirement(role_image, false);
            let endpoint_section_bytes = endpoint_layout.phase_cursor_state().bytes()
                + endpoint_layout.decision_state().bytes()
                + endpoint_layout.route_arm_stack().bytes()
                + endpoint_layout.lane_offer_state_slots().bytes()
                + endpoint_layout.frontier_state().bytes()
                + endpoint_layout.frontier_root_rows().bytes()
                + endpoint_layout.frontier_root_active_slots().bytes()
                + endpoint_layout.frontier_root_observed_key_slots().bytes()
                + endpoint_layout.frontier_offer_entry_slots().bytes()
                + endpoint_layout.binding_inbox().bytes()
                + endpoint_layout.binding_slots().bytes()
                + endpoint_layout.binding_len().bytes()
                + endpoint_layout.binding_frame_label_masks().bytes()
                + endpoint_layout.scope_evidence_slots().bytes();

            MeasuredResidentShape {
                route_scope_count: role_image.route_scope_count(),
                active_lane_count,
                max_route_stack_depth: role_image.max_route_stack_depth(),
                max_loop_stack_depth: role_image.max_loop_stack_depth(),
                route_bytes: crate::rendezvous::tables::RouteTable::storage_bytes(
                    role_image.route_table_frame_slots(),
                    role_image.route_table_lane_slots(),
                ),
                loop_bytes: crate::rendezvous::tables::LoopTable::storage_bytes(
                    role_image.loop_table_slots(),
                    if role_image.max_loop_stack_depth() == 0 {
                        0
                    } else {
                        role_image.endpoint_lane_slot_count()
                    },
                ),
                cap_bytes: crate::rendezvous::capability::CapTable::storage_bytes(
                    role_image.resident_cap_entries(),
                ),
                endpoint_bytes: endpoint_layout.total_bytes(),
                endpoint_header_bytes: endpoint_storage.header_bytes,
                endpoint_port_slots_bytes: endpoint_storage.port_slots_bytes,
                endpoint_guard_slots_bytes: endpoint_storage.guard_slots_bytes,
                endpoint_header_padding_bytes: endpoint_storage.header_padding_bytes,
                resident_program_descriptor_bytes: size_of::<CompiledProgramRef>(),
                resident_role_descriptor_bytes: size_of::<RoleImageSlice<ROLE>>(),
                endpoint_phase_cursor_state_bytes: endpoint_layout.phase_cursor_state().bytes(),
                endpoint_route_state_bytes: endpoint_layout.decision_state().bytes(),
                endpoint_route_arm_stack_bytes: endpoint_layout.route_arm_stack().bytes(),
                endpoint_lane_offer_state_slots_bytes: endpoint_layout
                    .lane_offer_state_slots()
                    .bytes(),
                endpoint_frontier_state_bytes: endpoint_layout.frontier_state().bytes(),
                endpoint_frontier_root_rows_bytes: endpoint_layout.frontier_root_rows().bytes(),
                endpoint_frontier_root_active_slots_bytes: endpoint_layout
                    .frontier_root_active_slots()
                    .bytes(),
                endpoint_frontier_root_observed_key_slots_bytes: endpoint_layout
                    .frontier_root_observed_key_slots()
                    .bytes(),
                endpoint_frontier_offer_entry_slots_bytes: endpoint_layout
                    .frontier_offer_entry_slots()
                    .bytes(),
                endpoint_binding_inbox_bytes: endpoint_layout.binding_inbox().bytes(),
                endpoint_binding_slots_bytes: endpoint_layout.binding_slots().bytes(),
                endpoint_binding_len_bytes: endpoint_layout.binding_len().bytes(),
                endpoint_binding_frame_label_masks_bytes: endpoint_layout
                    .binding_frame_label_masks()
                    .bytes(),
                endpoint_scope_evidence_store_bytes: 0,
                endpoint_scope_evidence_slots_bytes: endpoint_layout.scope_evidence_slots().bytes(),
                endpoint_padding_bytes: endpoint_layout
                    .total_bytes()
                    .saturating_sub(endpoint_section_bytes),
            }
        })
    })
}

#[test]
fn public_endpoint_leases_stay_small_and_metadata_only() {
    assert!(
        size_of::<crate::rendezvous::core::EndpointLeaseSlot>() <= 6 * size_of::<usize>(),
        "public endpoint lease must stay a small metadata owner"
    );
    let endpoint_storage_bytes = size_of::<
        crate::endpoint::kernel::CursorEndpoint<
            'static,
            0,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
            2,
            crate::control::cap::mint::MintConfig,
            crate::binding::BindingHandle<'static>,
        >,
    >();
    assert!(
        endpoint_storage_bytes <= CLUSTER_TEST_SLAB_CAPACITY,
        "shared cluster test slab must cover one leased public endpoint (required={}, cap={})",
        endpoint_storage_bytes,
        CLUSTER_TEST_SLAB_CAPACITY,
    );
}

#[test]
fn same_rendezvous_multi_enter_is_not_limited_by_max_rv() {
    run_on_transient_compiled_test_stack(
        "same_rendezvous_multi_enter_is_not_limited_by_max_rv",
        || {
            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let controller_program = linear_program::controller_program();
                    let worker_program = linear_program::worker_program();
                    let rv_id = cluster
                        .add_rendezvous_from_config_auto(config, DummyTransport)
                        .expect("register rendezvous");
                    let lease_capacity = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .endpoint_lease_capacity();
                    assert_eq!(
                        lease_capacity,
                        EndpointLeaseId::ZERO,
                        "public-path rendezvous must not preallocate endpoint leases before resident descriptor attach"
                    );

                    let first = cluster
                        .enter(
                            rv_id,
                            SessionId::new(1),
                            &controller_program,
                            crate::binding::BindingHandle::None(crate::binding::NoBinding),
                        )
                        .expect("enter controller on single rendezvous");
                    let second = cluster
                        .enter(
                            rv_id,
                            SessionId::new(1),
                            &worker_program,
                            crate::binding::BindingHandle::None(crate::binding::NoBinding),
                        )
                        .expect("enter worker on same rendezvous");

                    assert_ne!(
                        first.0, second.0,
                        "same-session controller/worker enters must keep distinct lease identities"
                    );
                    let lease_capacity = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .endpoint_lease_capacity();
                    assert_eq!(
                        lease_capacity,
                        EndpointLeaseId::from(2u16),
                        "endpoint lease table must grow to exactly the number of attached endpoints"
                    );

                    unsafe {
                        drop_test_public_endpoint_for_role::<1, 1>(cluster, rv_id, second);
                        drop_test_public_endpoint(cluster, rv_id, first);
                    }
                });
            });
        },
    );
}

#[test]
fn public_endpoint_slot_ids_do_not_truncate_above_u8() {
    run_on_transient_compiled_test_stack(
        "public_endpoint_slot_ids_do_not_truncate_above_u8",
        || {
            with_cluster_fixture(|clock, config| {
                with_test_cluster_1(clock, |cluster| {
                    let rv_id = cluster
                        .with_control_mut(|core| {
                            core.locals
                                .register_local_from_config_auto(config, DummyTransport)
                        })
                        .expect("register descriptor-sized rendezvous");
                    let lease_capacity = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .endpoint_lease_capacity();
                    assert_eq!(
                        lease_capacity,
                        EndpointLeaseId::ZERO,
                        "rendezvous must not preallocate endpoint slots before resident descriptors attach"
                    );

                    let mut handles =
                        [(EndpointLeaseId::ZERO, 0u32, 0usize, 0usize); u8::MAX as usize + 2];
                    cluster.with_control_mut(|core| {
                        let rv = core.locals.get_mut(&rv_id).expect("registered rendezvous");
                        for handle in &mut handles {
                            *handle = unsafe {
                                rv.allocate_endpoint_lease(
                                    1,
                                    1,
                                    crate::rendezvous::core::EndpointResidentBudget::ZERO,
                                )
                            }
                            .expect("lease across wide slot ids");
                        }
                    });

                    let lease_capacity = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .endpoint_lease_capacity();
                    assert!(
                        lease_capacity > EndpointLeaseId::from(u8::MAX),
                        "endpoint lease table must grow from attach/allocation demand without u8 truncation (capacity={lease_capacity})"
                    );

                    assert_eq!(
                        handles[u8::MAX as usize].0,
                        EndpointLeaseId::from(u8::MAX),
                        "slot 255 must remain addressable without narrowing"
                    );
                    assert_eq!(
                        handles[u8::MAX as usize + 1].0,
                        u16::from(EndpointLeaseId::from(u8::MAX))
                            .saturating_add(1)
                            .into(),
                        "slot 256 must survive without truncation"
                    );

                    let slot_255_storage = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .endpoint_lease_storage(
                            handles[u8::MAX as usize].0,
                            handles[u8::MAX as usize].1,
                        )
                        .expect("slot 255 storage");
                    let slot_256_storage = cluster
                        .get_local(&rv_id)
                        .expect("registered rendezvous")
                        .endpoint_lease_storage(
                            handles[u8::MAX as usize + 1].0,
                            handles[u8::MAX as usize + 1].1,
                        )
                        .expect("slot 256 storage");
                    assert_ne!(
                        slot_255_storage.0, slot_256_storage.0,
                        "distinct wide lease ids must resolve to distinct storage offsets"
                    );
                    assert_eq!(slot_255_storage.1, 1);
                    assert_eq!(slot_256_storage.1, 1);

                    cluster.with_control_mut(|core| {
                        let rv = core.locals.get_mut(&rv_id).expect("registered rendezvous");
                        for handle in handles.into_iter().rev() {
                            rv.release_endpoint_lease(handle.0, handle.1);
                        }
                    });
                });
            });
        },
    );
}

#[test]
fn pico2_resident_component_sizes() {
    let session_cluster_bytes = size_of::<StaticTestCluster<1>>();
    let control_core_bytes = size_of::<
        ControlCore<
            'static,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
            1,
        >,
    >();
    let rv_core_bytes = size_of::<
        crate::control::lease::core::ControlCore<
            'static,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
            1,
        >,
    >();
    let resolver_core_bytes = size_of::<ResolverCore<'static, 1>>();
    let lowering_summary_bytes = size_of::<CompiledProgramImage>();
    let compiled_program_bytes = size_of::<crate::global::compiled::images::CompiledProgramRef>();
    let compiled_role_bytes = size_of::<crate::global::compiled::images::CompiledRoleImage>();
    let route_heavy_worker = huge_program::worker_program();
    let route_heavy_footprint = route_heavy_worker.compiled_role_image().footprint();
    let role_compile_scratch_bytes = 0usize;
    let endpoint_storage_bytes = size_of::<
        crate::endpoint::kernel::CursorEndpoint<
            'static,
            0,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
            1,
            crate::control::cap::mint::MintConfig,
            crate::binding::BindingHandle<'static>,
        >,
    >();
    let rendezvous_header_bytes = size_of::<
        crate::rendezvous::core::Rendezvous<
            'static,
            'static,
            DummyTransport,
            DefaultLabelUniverse,
            CounterClock,
            crate::control::cap::mint::EpochTbl,
        >,
    >();
    let route_table_bytes = size_of::<crate::rendezvous::tables::RouteTable>();
    let loop_table_bytes = size_of::<crate::rendezvous::tables::LoopTable>();
    let cap_table_bytes = size_of::<crate::rendezvous::capability::CapTable>();
    let delegation_graph_bytes = size_of::<
        LeaseGraph<
            'static,
            DelegationLeaseSpec<
                DummyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
            >,
        >,
    >();
    let topology_graph_bytes = size_of::<
        LeaseGraph<
            'static,
            TopologyLeaseSpec<
                DummyTransport,
                DefaultLabelUniverse,
                CounterClock,
                crate::control::cap::mint::EpochTbl,
            >,
        >,
    >();
    assert!(
        session_cluster_bytes <= 1_700_000
            && control_core_bytes <= 1_700_000
            && rv_core_bytes <= 250_000
            && resolver_core_bytes <= 8_000
            && lowering_summary_bytes <= 184_224
            && compiled_program_bytes <= 64
            && compiled_role_bytes <= 64
            && role_compile_scratch_bytes == 0
            && endpoint_storage_bytes <= 90_000
            && rendezvous_header_bytes <= 32_768
            && route_table_bytes <= 128
            && loop_table_bytes <= 64
            && cap_table_bytes <= 64
            && delegation_graph_bytes <= 3_000
            && topology_graph_bytes <= 2_000,
        "resident regression: session_cluster={session_cluster_bytes} control_core={control_core_bytes} rv_core={rv_core_bytes} resolver={resolver_core_bytes} lowering_summary={lowering_summary_bytes} compiled_program={compiled_program_bytes} compiled_role={compiled_role_bytes} role_compile_scratch={role_compile_scratch_bytes} route_heavy_footprint(scope={}, active_depth={}, eff={}, local_steps={}, phases={}, phase_lane_entries={}, phase_lane_words={}, parallel={}) endpoint_storage={endpoint_storage_bytes} rendezvous_header={rendezvous_header_bytes} route_table={route_table_bytes} loop_table={loop_table_bytes} cap_table={cap_table_bytes} delegation_graph={delegation_graph_bytes} topology_graph={topology_graph_bytes}",
        route_heavy_footprint.scope_count,
        route_heavy_footprint.max_active_scope_depth,
        route_heavy_footprint.eff_count,
        route_heavy_footprint.local_step_count,
        route_heavy_footprint.phase_count,
        route_heavy_footprint.phase_lane_entry_count,
        route_heavy_footprint.phase_lane_word_count,
        route_heavy_footprint.parallel_enter_count,
    );
}

#[test]
fn huge_shape_matrix_resident_bytes_stay_measured_and_local() {
    let route_worker = huge_program::worker_program();
    let route = measure_huge_shape::<1>(&route_worker);
    let linear_worker = linear_program::worker_program();
    let linear = measure_huge_shape::<1>(&linear_worker);
    let fanout_worker = fanout_program::worker_program();
    let fanout = measure_huge_shape::<1>(&fanout_worker);

    for (name, measured) in [
        ("route_heavy", route),
        ("linear_heavy", linear),
        ("fanout_heavy", fanout),
    ] {
        std::println!(
            "resident-shape name={name} route_bytes={} loop_bytes={} cap_bytes={} endpoint_bytes={} endpoint_header_bytes={} endpoint_port_slots_bytes={} endpoint_guard_slots_bytes={} endpoint_header_padding_bytes={} resident_program_descriptor_bytes={} resident_role_descriptor_bytes={} endpoint_phase_cursor_state_bytes={} endpoint_route_state_bytes={} endpoint_route_arm_stack_bytes={} endpoint_lane_offer_state_slots_bytes={} endpoint_frontier_state_bytes={} endpoint_frontier_root_rows_bytes={} endpoint_frontier_root_active_slots_bytes={} endpoint_frontier_root_observed_key_slots_bytes={} endpoint_frontier_offer_entry_slots_bytes={} endpoint_binding_inbox_bytes={} endpoint_binding_slots_bytes={} endpoint_binding_len_bytes={} endpoint_binding_frame_label_masks_bytes={} endpoint_scope_evidence_store_bytes={} endpoint_scope_evidence_slots_bytes={} endpoint_padding_bytes={}",
            measured.route_bytes,
            measured.loop_bytes,
            measured.cap_bytes,
            measured.endpoint_bytes,
            measured.endpoint_header_bytes,
            measured.endpoint_port_slots_bytes,
            measured.endpoint_guard_slots_bytes,
            measured.endpoint_header_padding_bytes,
            measured.resident_program_descriptor_bytes,
            measured.resident_role_descriptor_bytes,
            measured.endpoint_phase_cursor_state_bytes,
            measured.endpoint_route_state_bytes,
            measured.endpoint_route_arm_stack_bytes,
            measured.endpoint_lane_offer_state_slots_bytes,
            measured.endpoint_frontier_state_bytes,
            measured.endpoint_frontier_root_rows_bytes,
            measured.endpoint_frontier_root_active_slots_bytes,
            measured.endpoint_frontier_root_observed_key_slots_bytes,
            measured.endpoint_frontier_offer_entry_slots_bytes,
            measured.endpoint_binding_inbox_bytes,
            measured.endpoint_binding_slots_bytes,
            measured.endpoint_binding_len_bytes,
            measured.endpoint_binding_frame_label_masks_bytes,
            measured.endpoint_scope_evidence_store_bytes,
            measured.endpoint_scope_evidence_slots_bytes,
            measured.endpoint_padding_bytes,
        );
    }

    assert_eq!(route.route_scope_count, huge_program::ROUTE_SCOPE_COUNT);
    assert_eq!(linear.route_scope_count, linear_program::ROUTE_SCOPE_COUNT);
    assert_eq!(fanout.route_scope_count, fanout_program::ROUTE_SCOPE_COUNT);

    assert!(
        route.route_bytes <= 2 * 1024,
        "route-heavy route resident bytes regressed: {:?}",
        route
    );
    assert!(
        linear.route_bytes <= 2 * 1024,
        "linear-heavy route resident bytes regressed: {:?}",
        linear
    );
    assert!(
        fanout.route_bytes <= 2 * 1024,
        "fanout-heavy route resident bytes regressed: {:?}",
        fanout
    );

    assert!(
        route.loop_bytes <= 2 * 1024,
        "route-heavy loop resident bytes regressed: {:?}",
        route
    );
    assert!(
        linear.loop_bytes <= 2 * 1024,
        "linear-heavy loop resident bytes regressed: {:?}",
        linear
    );
    assert!(
        fanout.loop_bytes <= 2 * 1024,
        "fanout-heavy loop resident bytes regressed: {:?}",
        fanout
    );

    assert!(
        route.cap_bytes <= 512,
        "route-heavy cap resident bytes regressed: {:?}",
        route
    );
    assert!(
        linear.cap_bytes <= 512,
        "linear-heavy cap resident bytes regressed: {:?}",
        linear
    );
    assert!(
        fanout.cap_bytes <= 512,
        "fanout-heavy cap resident bytes regressed: {:?}",
        fanout
    );

    assert!(
        route.endpoint_bytes <= 888,
        "route-heavy endpoint resident bytes regressed: {:?}",
        route
    );
    // The endpoint header budget includes the pending-send commit proof that
    // prevents post-transport progress/decision preflight replay on Pico-class
    // hosts. Endpoint arena bytes remain the resident storage ceiling.
    assert!(
        route.endpoint_header_bytes <= 1256,
        "route-heavy endpoint header bytes regressed: {:?}",
        route
    );
    assert!(
        linear.endpoint_bytes <= 544,
        "linear-heavy endpoint resident bytes regressed: {:?}",
        linear
    );
    assert!(
        linear.endpoint_header_bytes <= 1256,
        "linear-heavy endpoint header bytes regressed: {:?}",
        linear
    );
    assert!(
        fanout.endpoint_bytes <= 1040,
        "fanout-heavy endpoint resident bytes regressed: {:?}",
        fanout
    );
    assert!(
        fanout.endpoint_header_bytes <= 1256,
        "fanout-heavy endpoint header bytes regressed: {:?}",
        fanout
    );
    assert_eq!(
        route.endpoint_bytes,
        route.endpoint_phase_cursor_state_bytes
            + route.endpoint_route_state_bytes
            + route.endpoint_route_arm_stack_bytes
            + route.endpoint_lane_offer_state_slots_bytes
            + route.endpoint_frontier_state_bytes
            + route.endpoint_frontier_root_rows_bytes
            + route.endpoint_frontier_root_active_slots_bytes
            + route.endpoint_frontier_root_observed_key_slots_bytes
            + route.endpoint_frontier_offer_entry_slots_bytes
            + route.endpoint_binding_inbox_bytes
            + route.endpoint_binding_slots_bytes
            + route.endpoint_binding_len_bytes
            + route.endpoint_binding_frame_label_masks_bytes
            + route.endpoint_scope_evidence_store_bytes
            + route.endpoint_scope_evidence_slots_bytes
            + route.endpoint_padding_bytes,
        "route-heavy endpoint arena breakdown must cover the full resident total: {route:?}"
    );
    assert_eq!(
        linear.endpoint_bytes,
        linear.endpoint_phase_cursor_state_bytes
            + linear.endpoint_route_state_bytes
            + linear.endpoint_route_arm_stack_bytes
            + linear.endpoint_lane_offer_state_slots_bytes
            + linear.endpoint_frontier_state_bytes
            + linear.endpoint_frontier_root_rows_bytes
            + linear.endpoint_frontier_root_active_slots_bytes
            + linear.endpoint_frontier_root_observed_key_slots_bytes
            + linear.endpoint_frontier_offer_entry_slots_bytes
            + linear.endpoint_binding_inbox_bytes
            + linear.endpoint_binding_slots_bytes
            + linear.endpoint_binding_len_bytes
            + linear.endpoint_binding_frame_label_masks_bytes
            + linear.endpoint_scope_evidence_store_bytes
            + linear.endpoint_scope_evidence_slots_bytes
            + linear.endpoint_padding_bytes,
        "linear-heavy endpoint arena breakdown must cover the full resident total: {linear:?}"
    );
    assert_eq!(
        fanout.endpoint_bytes,
        fanout.endpoint_phase_cursor_state_bytes
            + fanout.endpoint_route_state_bytes
            + fanout.endpoint_route_arm_stack_bytes
            + fanout.endpoint_lane_offer_state_slots_bytes
            + fanout.endpoint_frontier_state_bytes
            + fanout.endpoint_frontier_root_rows_bytes
            + fanout.endpoint_frontier_root_active_slots_bytes
            + fanout.endpoint_frontier_root_observed_key_slots_bytes
            + fanout.endpoint_frontier_offer_entry_slots_bytes
            + fanout.endpoint_binding_inbox_bytes
            + fanout.endpoint_binding_slots_bytes
            + fanout.endpoint_binding_len_bytes
            + fanout.endpoint_binding_frame_label_masks_bytes
            + fanout.endpoint_scope_evidence_store_bytes
            + fanout.endpoint_scope_evidence_slots_bytes
            + fanout.endpoint_padding_bytes,
        "fanout-heavy endpoint arena breakdown must cover the full resident total: {fanout:?}"
    );

    assert!(
        route.resident_program_descriptor_bytes <= 32
            && linear.resident_program_descriptor_bytes <= 32
            && fanout.resident_program_descriptor_bytes <= 32
            && route.resident_role_descriptor_bytes <= 64
            && linear.resident_role_descriptor_bytes <= 64
            && fanout.resident_role_descriptor_bytes <= 64,
        "resident descriptor refs must stay small and must not reintroduce materialized blobs: route={route:?} linear={linear:?} fanout={fanout:?}"
    );

    assert!(
        route.route_bytes >= linear.route_bytes,
        "route-heavy resident bytes must not fall below linear when route scopes are present: route={route:?} linear={linear:?}"
    );
    assert_eq!(
        route.route_bytes, fanout.route_bytes,
        "route resident bytes must stay tied to live route depth rather than total scope count: route={route:?} fanout={fanout:?}"
    );
    assert!(
        fanout.endpoint_bytes >= route.endpoint_bytes,
        "fanout-heavy endpoint resident bytes should dominate route-heavy due to larger branch fan-out: route={route:?} fanout={fanout:?}"
    );
}
