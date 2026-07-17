use super::*;
use crate::eff::{EffAtom, EventOrigin};
use crate::g::{self, Msg, Program};
use crate::global::compiled::images::{CompiledProgramRef, ProgramImageColumns, RoleDescriptorRef};
use crate::global::compiled::lowering::RoleCompiledCounts;
use crate::global::const_dsl::{EffList, ScopeKind};
use crate::global::event_program::LocalEventProgram;
use crate::global::program::Projectable;
use crate::global::typestate::{LocalAction, LocalConflict};

#[macro_use]
#[path = "tests/final_form_protocol_matrix.rs"]
mod final_form_protocol_matrix;
mod full_role_domain;
mod protocol_matrix;

const LOCAL_STEP_STRESS_ROW_BUDGET: usize = 512;
const NESTED_PAR_ROUTE_RESOLVER: u16 = 0x91;
const ROLL_ROUTE_INTERNAL_PARALLEL_RESOLVER: u16 = 0x92;

const fn test_atom(label: u8, lane: u8) -> EffAtom {
    EffAtom {
        from: 0,
        to: 1,
        label,
        payload_schema: label as u32,
        origin: EventOrigin::User,
        lane,
    }
}

type StressEffList = EffList<{ LOCAL_STEP_STRESS_ROW_BUDGET + 1 }>;

const fn over_local_step_capacity_atom_program() -> StressEffList {
    let mut list = StressEffList::new();
    let mut idx = 0usize;
    while idx <= LOCAL_STEP_STRESS_ROW_BUDGET {
        list = list.push(test_atom(idx as u8, (idx % LANE_DOMAIN_SIZE) as u8));
        idx += 1;
    }
    list
}

static OVER_LOCAL_STEP_CAPACITY_ATOMS: StressEffList = over_local_step_capacity_atom_program();

fn with_role_descriptor<const ROLE: u8, R>(
    program: &RoleProgram<ROLE>,
    f: impl FnOnce(RoleDescriptorRef) -> R,
) -> R {
    f(RoleDescriptorRef::from_resident(program.role_image_ref()))
}

#[test]
fn explicit_resolver_route_scope_survives_nested_parallel_head() {
    let left = g::par(
        g::send::<0, 1, Msg<31, u8>>(),
        g::send::<0, 2, Msg<32, u8>>(),
    );
    let right = g::seq(
        g::send::<0, 1, Msg<33, u8>>(),
        g::send::<0, 2, Msg<34, u8>>(),
    );
    let route = g::route(left, right).resolve::<NESTED_PAR_ROUTE_RESOLVER>();
    let role0: RoleProgram<0> = project(&route);
    let program_ref = role0.role_image_ref().program;

    assert_eq!(program_ref.route_resolver_row_count(), 1);
    let (scope, resolver) = program_ref
        .route_resolver_authority_at_row(0)
        .expect("route authority row");
    assert_eq!(
        resolver.map(|resolver| resolver.resolver_id()),
        Some(NESTED_PAR_ROUTE_RESOLVER)
    );
    assert_eq!(program_ref.route_controller_role(scope), 0);
    assert!(program_ref.route_resolver(scope).is_some());
    assert_eq!(program_ref.route_participant_count(scope, 0), 3);
    assert_eq!(program_ref.route_participant_count(scope, 1), 3);
    for role in 0..=2 {
        assert!(program_ref.route_has_participant(scope, 0, role));
        assert!(program_ref.route_has_participant(scope, 1, role));
    }

    let events = LocalEventProgram::from_rows(role0.role_image_ref());
    let slot = events.route_scope_slot(scope).expect("route slot");
    let left = events
        .route_arm_event_row_by_slot(slot, 0)
        .expect("left route arm row");
    let right = events
        .route_arm_event_row_by_slot(slot, 1)
        .expect("right route arm row");
    assert!(
        left.start() < left.end(),
        "left route arm row must be nonempty"
    );
    assert!(
        right.start() < right.end(),
        "right route arm row must be nonempty"
    );
    assert_ne!(left, right, "route arm rows must stay arm-distinct");
}

#[test]
fn route_history_capacity_tracks_emitted_lane_relations() {
    let routed = g::route(
        g::send::<0, 1, Msg<21, u8>>(),
        g::send::<0, 1, Msg<22, u8>>(),
    );
    let unrelated = g::par(
        g::send::<0, 2, Msg<23, u8>>(),
        g::send::<0, 3, Msg<24, u8>>(),
    );
    let program = g::par(routed, unrelated);
    let role0: RoleProgram<0> = project(&program);
    let image = role0.role_image_ref();
    let descriptor = RoleDescriptorRef::from_resident(image);
    let footprint = image.footprint();
    let layout = descriptor.endpoint_arena_layout();

    assert_eq!(
        footprint.route_arm_state_capacity,
        image.columns.route_arm_lane_step_rows.len as usize
    );
    assert_eq!(
        layout.route_arm_history().count(),
        footprint.route_arm_state_capacity
    );
    assert!(
        footprint.route_arm_state_capacity
            < footprint.active_lane_count * footprint.max_route_commit_count,
        "sparse route history must not regress to active-lane by route-depth storage"
    );
}

#[test]
fn inactive_role_keeps_only_its_session_lane_and_no_frontier_reserve() {
    let program = g::send::<0, 2, Msg<20, u8>>();
    let role1: RoleProgram<1> = project(&program);
    let image = role1.role_image_ref();
    let footprint = image.footprint();
    let layout = RoleDescriptorRef::from_resident(image).endpoint_arena_layout();

    assert_eq!(footprint.active_lane_count, 0);
    assert_eq!(footprint.endpoint_lane_slot_count, 1);
    assert_eq!(footprint.logical_lane_count, 1);
    assert_eq!(image.first_active_lane(), None);
    assert_eq!(image.active_lane_row.start(), 0);
    assert_eq!(image.active_lane_row.len(), 0);
    assert_eq!(footprint.frontier_entry_count(), 0);
    assert_eq!(layout.frontier_root_active_slots().count(), 0);
    assert_eq!(layout.frontier_visited_entries().count(), 0);
}

#[test]
fn empty_route_lane_sets_have_one_canonical_byte_encoding() {
    let program = g::seq(
        g::send::<0, 2, Msg<39, u8>>(),
        g::route(
            g::send::<0, 1, Msg<40, u8>>(),
            g::send::<0, 1, Msg<41, u8>>(),
        ),
    );
    let role2: RoleProgram<2> = project(&program);
    let image = role2.role_image_ref();

    assert_eq!(image.columns.route_scopes.len, 1);
    assert_eq!(image.columns.route_arm_lane_rows.len, 2);
    for row in 0..2usize {
        let offset =
            image.columns.route_arm_lane_rows.offset as usize + row * ROLE_IMAGE_LANE_RANGE_STRIDE;
        let raw = image.blob.byte_at(offset) as u32
            | ((image.blob.byte_at(offset + 1) as u32) << 8)
            | ((image.blob.byte_at(offset + 2) as u32) << 16)
            | ((image.blob.byte_at(offset + 3) as u32) << 24);
        assert_eq!(raw, PackedLaneRange::new(0, 0).raw());
    }
}

#[test]
fn role_projections_share_program_wide_resolver_identity() {
    let route = g::route(
        g::seq(
            g::send::<0, 1, Msg<34, u8>>(),
            g::send::<0, 2, Msg<36, u8>>(),
        ),
        g::seq(
            g::send::<0, 2, Msg<35, u8>>(),
            g::send::<0, 1, Msg<37, u8>>(),
        ),
    )
    .resolve::<NESTED_PAR_ROUTE_RESOLVER>();
    let role0: RoleProgram<0> = project(&route);
    let role1: RoleProgram<1> = project(&route);
    let role2: RoleProgram<2> = project(&route);
    let program = role0.role_image_ref().program;

    assert!(program.same_image(role1.role_image_ref().program));
    assert!(program.same_image(role2.role_image_ref().program));
    let (scope, _) = program
        .route_resolver_authority_at_row(0)
        .expect("route authority row");
    assert_eq!(program.route_participant_count(scope, 0), 3);
    assert_eq!(program.route_participant_count(scope, 1), 3);
    for role in 0..=2 {
        assert!(program.route_has_participant(scope, 0, role));
        assert!(program.route_has_participant(scope, 1, role));
    }
}

#[test]
fn resolver_identity_distinguishes_equal_count_scope_topology() {
    let wide_roll = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, Msg<36, u8>>(),
                g::send::<0, 2, Msg<76, u8>>(),
            ),
            g::seq(
                g::send::<0, 2, Msg<37, u8>>(),
                g::send::<0, 1, Msg<77, u8>>(),
            ),
        )
        .resolve::<NESTED_PAR_ROUTE_RESOLVER>(),
        g::seq(
            g::send::<0, 1, Msg<38, u8>>(),
            g::send::<0, 1, Msg<39, u8>>(),
        )
        .roll(),
    );
    let narrow_roll = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, Msg<36, u8>>(),
                g::send::<0, 2, Msg<76, u8>>(),
            ),
            g::seq(
                g::send::<0, 2, Msg<37, u8>>(),
                g::send::<0, 1, Msg<77, u8>>(),
            ),
        )
        .resolve::<NESTED_PAR_ROUTE_RESOLVER>(),
        g::seq(
            g::send::<0, 1, Msg<38, u8>>().roll(),
            g::send::<0, 1, Msg<39, u8>>(),
        ),
    );
    let wide_role0: RoleProgram<0> = project(&wide_roll);
    let narrow_role0: RoleProgram<0> = project(&narrow_roll);
    let wide_program = wide_role0.role_image_ref().program;
    let narrow_program = narrow_role0.role_image_ref().program;

    assert_eq!(wide_program.facts, narrow_program.facts);
    assert_eq!(wide_program.columns, narrow_program.columns);
    assert_eq!(
        wide_program.columns.atom_count(),
        narrow_program.columns.atom_count()
    );
    assert_eq!(
        wide_program.columns.route_resolver_count(),
        narrow_program.columns.route_resolver_count()
    );
    assert_eq!(
        wide_program.columns.scope_marker_count(),
        narrow_program.columns.scope_marker_count()
    );
    for eff_idx in 0..wide_program.columns.atom_count() {
        assert_eq!(
            wide_program.atom_at(eff_idx),
            narrow_program.atom_at(eff_idx)
        );
    }
    let mut wide_resolvers = wide_program.route_resolver_sites_for(NESTED_PAR_ROUTE_RESOLVER);
    let mut narrow_resolvers = narrow_program.route_resolver_sites_for(NESTED_PAR_ROUTE_RESOLVER);
    assert_eq!(wide_resolvers.next(), narrow_resolvers.next());
    assert_eq!(wide_resolvers.next(), None);
    assert_eq!(narrow_resolvers.next(), None);

    assert!(
        !wide_program.same_image(narrow_program),
        "resolver identity must include exact scope boundaries, not only marker counts"
    );
}

#[test]
fn simple_controller_route_arm_event_rows_are_exact() {
    let route = g::route(
        g::send::<0, 1, Msg<71, u32>>(),
        g::send::<0, 1, Msg<72, u32>>(),
    );
    let program = g::seq(route, g::send::<0, 1, Msg<73, u32>>());
    let role0: RoleProgram<0> = project(&program);
    let events = LocalEventProgram::from_rows(role0.role_image_ref());
    let region = events
        .route_scope_rows_by_slot(0)
        .expect("simple route scope row");
    let slot = events
        .route_scope_slot(region.scope())
        .expect("simple route slot");
    let left = events
        .route_arm_event_row_by_slot(slot, 0)
        .expect("left route arm row");
    let right = events
        .route_arm_event_row_by_slot(slot, 1)
        .expect("right route arm row");

    assert_eq!((region.start(), region.end()), (0, 2));
    assert_eq!((left.start(), left.end()), (0, 1));
    assert_eq!((right.start(), right.end()), (1, 2));
    assert_eq!(
        events.event_conflict_for_index(0).to_conflict(),
        Some(LocalConflict::RouteArm {
            scope: region.scope(),
            arm: 0,
        })
    );
    assert_eq!(
        events.event_conflict_for_index(1).to_conflict(),
        Some(LocalConflict::RouteArm {
            scope: region.scope(),
            arm: 1,
        })
    );
    let left_commit = events.route_commit_range_by_slot(slot, 0);
    let right_commit = events.route_commit_range_by_slot(slot, 1);
    assert_eq!(left_commit.len(), 1);
    assert_eq!(right_commit.len(), 1);
    assert_eq!(
        events
            .route_commit_row_at(left_commit.start())
            .to_conflict(),
        Some(LocalConflict::RouteArm {
            scope: region.scope(),
            arm: 0,
        })
    );
    assert_eq!(
        events
            .route_commit_row_at(right_commit.start())
            .to_conflict(),
        Some(LocalConflict::RouteArm {
            scope: region.scope(),
            arm: 1,
        })
    );
}

#[test]
fn resident_route_arm_access_rejects_nonbinary_index() {
    let route = g::route(
        g::send::<0, 1, Msg<71, u32>>(),
        g::send::<0, 1, Msg<72, u32>>(),
    );
    let role0: RoleProgram<0> = project(&route);
    let events = LocalEventProgram::from_rows(role0.role_image_ref());
    let region = events
        .route_scope_rows_by_slot(0)
        .expect("simple route scope row");
    let slot = events
        .route_scope_slot(region.scope())
        .expect("simple route slot");

    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        events.route_arm_event_row_by_slot(slot, 2)
    }));
    assert!(rejected.is_err());
}

#[test]
fn logical_lane_count_stays_inside_wire_lane_domain() {
    assert_eq!(logical_lane_count_for_role(0, 1), 1);
    assert_eq!(logical_lane_count_for_role(1, 1), 1);
    assert_eq!(logical_lane_count_for_role(254, 255), 255);
    assert_eq!(logical_lane_count_for_role(255, 256), LANE_DOMAIN_SIZE);
    assert_eq!(logical_lane_count_for_role(256, 256), LANE_DOMAIN_SIZE);
}

#[test]
#[should_panic]
fn lane_set_mutation_rejects_a_lane_outside_its_exact_span() {
    let mut words = [0u32; 1];
    let mut set = core::mem::MaybeUninit::<LaneSet>::uninit();
    unsafe {
        LaneSet::init_from_parts(set.as_mut_ptr(), words.as_mut_ptr(), words.len());
    }
    let mut set = unsafe { set.assume_init() };
    set.insert(LaneWord::BITS as usize);
}

#[test]
fn lane_set_view_iterates_set_bits_without_empty_lane_scan() {
    let mut words = [0u32; 4];
    let (word, bit) = lane_word_index(3);
    words[word] |= bit;
    let (word, bit) = lane_word_index(LaneWord::BITS as usize + 5);
    words[word] |= bit;
    let (word, bit) = lane_word_index(LaneWord::BITS as usize * 2 + 1);
    words[word] |= bit;
    /* SAFETY: `words` remains live and immutable for the complete view use. */
    let view = unsafe { LaneSetView::from_parts(words.as_ptr(), words.len()) };

    assert_eq!(view.first_set(256), Some(3));
    assert_eq!(
        view.next_set_from(4, 256),
        Some(LaneWord::BITS as usize + 5)
    );
    assert_eq!(
        view.next_set_from(LaneWord::BITS as usize + 6, 256),
        Some(LaneWord::BITS as usize * 2 + 1),
    );
    assert_eq!(
        view.next_set_from(LaneWord::BITS as usize * 2 + 2, 256),
        None,
    );
    assert_eq!(view.next_set_from(LaneWord::BITS as usize + 6, 65), None);
}

#[test]
fn descriptor_lane_byte_view_remains_byte_aligned_and_covers_lane_255() {
    let mut storage = [0u8; lane_byte_count(LANE_DOMAIN_SIZE) + 1];
    storage[1] = 1 << 3;
    storage[1 + lane_byte_count(LANE_DOMAIN_SIZE) - 1] = 1 << 7;
    /* SAFETY: the deliberately unaligned byte span is live and immutable for
    the complete view use. Byte mode must never reinterpret it as lane words. */
    let view = unsafe {
        LaneSetView::from_bytes(
            storage.as_ptr().add(1),
            lane_byte_count(LANE_DOMAIN_SIZE),
            LANE_SET_VIEW_WORDS,
        )
    };

    assert_eq!(view.first_set(LANE_DOMAIN_SIZE), Some(3));
    assert_eq!(view.next_set_from(4, LANE_DOMAIN_SIZE), Some(255));
    assert!(view.contains(255));
}

#[test]
#[should_panic]
fn descriptor_lane_byte_view_rejects_a_span_beyond_the_lane_domain() {
    let byte = 0u8;
    /* SAFETY: the invalid span must be rejected before it can read the
    one-byte allocation. */
    let _ = unsafe {
        LaneSetView::from_bytes(
            core::ptr::addr_of!(byte),
            lane_byte_count(LANE_DOMAIN_SIZE) + 1,
            LANE_SET_VIEW_WORDS,
        )
    };
}

#[test]
fn lane_set_view_word_compare_can_ignore_one_lane_without_empty_lane_scan() {
    fn equals_until(lhs: LaneSetView<'_>, rhs: LaneSetView<'_>, lane_limit: usize) -> bool {
        let mut lane = 0usize;
        while lane < lane_limit {
            if lhs.contains(lane) != rhs.contains(lane) {
                return false;
            }
            lane += 1;
        }
        true
    }

    fn equals_until_except_lane(
        lhs: LaneSetView<'_>,
        rhs: LaneSetView<'_>,
        lane_limit: usize,
        except_lane: usize,
    ) -> bool {
        let mut lane = 0usize;
        while lane < lane_limit {
            if lane != except_lane && lhs.contains(lane) != rhs.contains(lane) {
                return false;
            }
            lane += 1;
        }
        true
    }

    let mut lhs = [0u32; 4];
    let mut rhs = [0u32; 4];
    let (word, bit) = lane_word_index(3);
    lhs[word] |= bit;
    rhs[word] |= bit;
    let (word, bit) = lane_word_index(LaneWord::BITS as usize + 5);
    lhs[word] |= bit;
    rhs[word] |= bit;
    let (word, bit) = lane_word_index(LaneWord::BITS as usize + 9);
    lhs[word] |= bit;
    let (word, bit) = lane_word_index(LaneWord::BITS as usize * 3 + 7);
    rhs[word] |= bit;

    /* SAFETY: both word arrays remain live and immutable for the complete
    comparison. */
    let lhs = unsafe { LaneSetView::from_parts(lhs.as_ptr(), lhs.len()) };
    /* SAFETY: see the shared comparison owner contract above. */
    let rhs = unsafe { LaneSetView::from_parts(rhs.as_ptr(), rhs.len()) };

    assert!(!equals_until(lhs, rhs, LaneWord::BITS as usize * 2));
    assert!(equals_until_except_lane(
        lhs,
        rhs,
        LaneWord::BITS as usize * 2,
        LaneWord::BITS as usize + 9
    ));
    assert!(
        equals_until_except_lane(
            lhs,
            rhs,
            LaneWord::BITS as usize * 3,
            LaneWord::BITS as usize + 9
        ),
        "bits beyond the active lane limit are not semantic lane state"
    );
}

#[test]
fn resident_lane_views_stay_compact() {
    assert!(
        core::mem::size_of::<LaneSetView<'static>>() <= 2 * core::mem::size_of::<usize>(),
        "LaneSetView must stay a borrowed word/list descriptor, not a copied lane set"
    );
    let word = core::mem::size_of::<usize>();
    assert!(
        core::mem::size_of::<BlobPtr>() == word
            && core::mem::size_of::<CompiledProgramRef>() <= 4 * word
            && core::mem::size_of::<RoleImageRef>() <= 12 * word
            && core::mem::size_of::<RoleLaneImage<'static>>() <= 2 * word,
        "resident refs must stay thin blob column views without fat-slice lengths"
    );
}

fn assert_minimal_send_footprint(image: RoleDescriptorRef) {
    let rows = image.local_event_rows();
    let lanes = rows.lanes();
    let columns = lanes.columns;
    assert_eq!(rows.local_step_count(), 1);
    assert_eq!(columns.events.len, 1);
    assert_eq!(columns.lanes.len, 1);
    assert!(
        columns.dependencies.len <= 1,
        "minimal send must not keep a max-capacity dependency column"
    );
    assert_eq!(
        columns.conflicts.len, 0,
        "minimal send has no route conflict rows"
    );
    assert_eq!(columns.route_scopes.len, 0);
    assert_eq!(columns.route_scope_conflicts.len, 0);
    assert_eq!(columns.route_arms.len, 0);
    assert_eq!(columns.route_arm_lane_rows.len, 0);
    assert_eq!(columns.route_offer_lane_rows.len, 0);
    assert_eq!(columns.route_arm_lane_step_rows.len, 0);
    assert_eq!(core::mem::size_of::<ColumnRange>(), 4);
    assert_eq!(
        core::mem::size_of::<ProgramImageColumns>(),
        4 * core::mem::size_of::<u16>(),
        "program columns must contain only atom, resolver, participant, and scope counts"
    );
    assert!(
        core::mem::size_of::<RoleImageColumns>() < 15 * 5,
        "RoleImageColumns must not retain stride or forbidden passive metadata"
    );
    assert!(
        core::mem::size_of::<RuntimeRoleFacts>() < 14 * core::mem::size_of::<u16>(),
        "RuntimeRoleFacts must stay below the forbidden 14-word fact block"
    );
    assert_eq!(
        columns.resident_boundaries.len, 2,
        "one resident row is encoded by start/end boundaries only"
    );
    assert!(
        columns.lane_bits.len <= 1,
        "single-lane protocol should need at most one packed lane mask byte"
    );
    assert!(rows.dependency_for_index(0).is_none());
    assert!(rows.event_conflict_for_index(0).to_conflict().is_none());
}

#[test]
fn minimal_send_descriptor_has_exact_resident_footprint() {
    let program = g::send::<0, 1, Msg<1, ()>>();
    let sender: RoleProgram<0> = project(&program);
    let receiver: RoleProgram<1> = project(&program);

    with_role_descriptor(&sender, assert_minimal_send_footprint);
    with_role_descriptor(&receiver, assert_minimal_send_footprint);
}

#[test]
fn streaming_role_image_tracks_actual_event_count() {
    assert!(OVER_LOCAL_STEP_CAPACITY_ATOMS.len() > LOCAL_STEP_STRESS_ROW_BUDGET);
    let facts = RuntimeRoleFacts::from_counts(RoleCompiledCounts {
        max_route_commit_count: 0,
        local_step_count: OVER_LOCAL_STEP_CAPACITY_ATOMS.len(),
        route_scope_count: 0,
        active_lane_count: LANE_DOMAIN_SIZE,
        endpoint_lane_slot_count: LANE_DOMAIN_SIZE,
        logical_lane_count: LANE_DOMAIN_SIZE,
    });
    let plan = RoleImagePlan::from_program(&OVER_LOCAL_STEP_CAPACITY_ATOMS, facts, 0);
    let build = plan
        .build_if_fits::<{ u16::MAX as usize }, { LOCAL_STEP_STRESS_ROW_BUDGET + 1 }>(
            &OVER_LOCAL_STEP_CAPACITY_ATOMS,
            facts,
            0,
        )
        .expect("streaming role descriptor fits the compact byte domain");

    assert_eq!(
        build.columns.events.len as usize,
        OVER_LOCAL_STEP_CAPACITY_ATOMS.len()
    );
    assert_eq!(
        build.columns.lanes.len as usize,
        OVER_LOCAL_STEP_CAPACITY_ATOMS.len()
    );
    assert_eq!(build.columns.resident_boundaries.len, 2);
    assert_eq!(
        build.columns.lane_bits.len as usize,
        LANE_DOMAIN_SIZE / u8::BITS as usize
    );
}

#[test]
fn streaming_role_image_accepts_more_than_256_resident_rows() {
    const PHASES: usize = 257;
    let mut source = EffList::<2048>::new_partitioned(PHASES * 2, PHASES * 3, 0);
    let mut phase = 0usize;
    while phase < PHASES {
        let scope = crate::global::const_dsl::ScopeId::parallel(phase as u16);
        let start = source.len();
        source.push_event_mut(test_atom(1, 0));
        let split = source.len();
        source.push_event_mut(test_atom(2, 1));
        let end = source.len();
        source.push_parallel_scope_mut(scope, start, split, end);
        phase += 1;
    }
    let facts = RuntimeRoleFacts::from_counts(RoleCompiledCounts {
        max_route_commit_count: 0,
        local_step_count: PHASES * 2,
        route_scope_count: 0,
        active_lane_count: 2,
        endpoint_lane_slot_count: 2,
        logical_lane_count: 2,
    });
    let plan = RoleImagePlan::from_program(&source, facts, 0);
    let build = plan
        .build_if_fits::<{ u16::MAX as usize }, 2048>(&source, facts, 0)
        .expect("257 resident phases fit their exact role image");

    assert_eq!(build.columns.resident_boundaries.len as usize, PHASES + 1);
}

fn assert_parallel_resident_row_shape(image: RoleDescriptorRef) {
    let rows = image.local_event_rows();
    assert_eq!(
        rows.resident_row_lane_steps(0, 0).map(|steps| steps.len),
        Some(1)
    );
    assert_eq!(
        rows.resident_row_lane_steps(0, 1).map(|steps| steps.len),
        Some(1)
    );
    assert!(rows.resident_row_lane_steps(1, 0).is_none());
    assert!(rows.resident_row_lane_steps(1, 1).is_none());
}

type ParallelLane0 = g::Send<0, 1, Msg<9, ()>>;
type ParallelLane1 = g::Send<1, 0, Msg<10, ()>>;
fn parallel_lane0_program() -> Program<ParallelLane0> {
    g::send::<0, 1, Msg<9, ()>>()
}
fn parallel_lane1_program() -> Program<ParallelLane1> {
    g::send::<1, 0, Msg<10, ()>>()
}
fn parallel_program() -> Program<g::Par<ParallelLane0, ParallelLane1>> {
    g::par(parallel_lane0_program(), parallel_lane1_program())
}

type RouteLeft = g::Seq<g::Send<0, 0, Msg<14, ()>>, g::Send<0, 1, Msg<15, ()>>>;
type RouteRight = g::Seq<g::Send<0, 0, Msg<16, ()>>, g::Send<0, 1, Msg<17, ()>>>;
fn route_left_program() -> Program<RouteLeft> {
    g::seq(
        g::send::<0, 0, Msg<14, ()>>(),
        g::send::<0, 1, Msg<15, ()>>(),
    )
}
fn route_right_program() -> Program<RouteRight> {
    g::seq(
        g::send::<0, 0, Msg<16, ()>>(),
        g::send::<0, 1, Msg<17, ()>>(),
    )
}
type RouteProgramSteps = g::Route<RouteLeft, RouteRight>;
fn route_program() -> Program<RouteProgramSteps> {
    g::route(route_left_program(), route_right_program())
}
fn parallel_route_program() -> Program<g::Par<ParallelLane1, RouteProgramSteps>> {
    g::par(parallel_lane1_program(), route_program())
}

type SparseRoute0 = g::Route<g::Send<0, 1, Msg<100, ()>>, g::Send<0, 1, Msg<101, ()>>>;
type SparseRoute1 = g::Route<g::Send<0, 1, Msg<102, ()>>, g::Send<0, 1, Msg<103, ()>>>;
type SparseRoute2 = g::Route<g::Send<0, 1, Msg<104, ()>>, g::Send<0, 1, Msg<105, ()>>>;
type SparseRoute3 = g::Route<g::Send<0, 1, Msg<106, ()>>, g::Send<0, 1, Msg<107, ()>>>;
type SparseRouteArmProgram =
    g::Seq<g::Seq<SparseRoute0, SparseRoute1>, g::Seq<SparseRoute2, SparseRoute3>>;

fn sparse_route_arm_program() -> Program<SparseRouteArmProgram> {
    let route0 = g::route(
        g::send::<0, 1, Msg<100, ()>>(),
        g::send::<0, 1, Msg<101, ()>>(),
    );
    let route1 = g::route(
        g::send::<0, 1, Msg<102, ()>>(),
        g::send::<0, 1, Msg<103, ()>>(),
    );
    let route2 = g::route(
        g::send::<0, 1, Msg<104, ()>>(),
        g::send::<0, 1, Msg<105, ()>>(),
    );
    let route3 = g::route(
        g::send::<0, 1, Msg<106, ()>>(),
        g::send::<0, 1, Msg<107, ()>>(),
    );
    g::seq(g::seq(route0, route1), g::seq(route2, route3))
}

macro_rules! par_frontier {
    ($single:expr $(,)?) => {
        $single
    };
    ($first:expr, $($rest:expr),+ $(,)?) => {
        g::par($first, par_frontier!($($rest),+))
    };
}

fn sparse_route_high_lane_program() -> impl Projectable {
    let non_participant_lanes = par_frontier!(
        g::send::<1, 2, Msg<108, ()>>(),
        g::send::<1, 2, Msg<109, ()>>(),
        g::send::<1, 2, Msg<110, ()>>(),
        g::send::<1, 2, Msg<111, ()>>(),
        g::send::<1, 2, Msg<112, ()>>(),
        g::send::<1, 2, Msg<113, ()>>(),
        g::send::<1, 2, Msg<114, ()>>(),
        g::send::<1, 2, Msg<115, ()>>(),
        g::send::<1, 2, Msg<116, ()>>(),
        g::send::<1, 2, Msg<117, ()>>(),
        g::send::<1, 2, Msg<118, ()>>(),
        g::send::<1, 2, Msg<119, ()>>(),
        g::send::<1, 2, Msg<120, ()>>(),
        g::send::<1, 2, Msg<121, ()>>(),
        g::send::<1, 2, Msg<122, ()>>(),
        g::send::<1, 2, Msg<123, ()>>(),
        g::send::<1, 2, Msg<124, ()>>(),
        g::send::<1, 2, Msg<125, ()>>(),
        g::send::<1, 2, Msg<126, ()>>(),
        g::send::<1, 2, Msg<127, ()>>(),
        g::send::<1, 2, Msg<128, ()>>(),
        g::send::<1, 2, Msg<129, ()>>(),
        g::send::<1, 2, Msg<130, ()>>(),
        g::send::<1, 2, Msg<131, ()>>(),
        g::send::<1, 2, Msg<132, ()>>(),
        g::send::<1, 2, Msg<133, ()>>(),
        g::send::<1, 2, Msg<134, ()>>(),
        g::send::<1, 2, Msg<135, ()>>(),
        g::send::<1, 2, Msg<136, ()>>(),
        g::send::<1, 2, Msg<137, ()>>(),
        g::send::<1, 2, Msg<138, ()>>(),
        g::send::<1, 2, Msg<139, ()>>(),
        g::send::<1, 2, Msg<140, ()>>(),
        g::send::<1, 2, Msg<141, ()>>(),
        g::send::<1, 2, Msg<142, ()>>(),
        g::send::<1, 2, Msg<143, ()>>(),
        g::send::<1, 2, Msg<144, ()>>(),
        g::send::<1, 2, Msg<145, ()>>(),
        g::send::<1, 2, Msg<146, ()>>(),
        g::send::<1, 2, Msg<147, ()>>(),
        g::send::<1, 2, Msg<148, ()>>(),
        g::send::<1, 2, Msg<149, ()>>(),
        g::send::<1, 2, Msg<150, ()>>(),
        g::send::<1, 2, Msg<151, ()>>(),
        g::send::<1, 2, Msg<152, ()>>(),
        g::send::<1, 2, Msg<153, ()>>(),
        g::send::<1, 2, Msg<154, ()>>(),
        g::send::<1, 2, Msg<155, ()>>(),
        g::send::<1, 2, Msg<156, ()>>(),
        g::send::<1, 2, Msg<157, ()>>(),
        g::send::<1, 2, Msg<158, ()>>(),
        g::send::<1, 2, Msg<159, ()>>(),
        g::send::<1, 2, Msg<160, ()>>(),
        g::send::<1, 2, Msg<161, ()>>(),
        g::send::<1, 2, Msg<162, ()>>(),
        g::send::<1, 2, Msg<163, ()>>(),
        g::send::<1, 2, Msg<164, ()>>(),
        g::send::<1, 2, Msg<165, ()>>(),
        g::send::<1, 2, Msg<166, ()>>(),
        g::send::<1, 2, Msg<167, ()>>(),
        g::send::<1, 2, Msg<168, ()>>(),
        g::send::<1, 2, Msg<169, ()>>(),
        g::send::<1, 2, Msg<170, ()>>(),
    );
    g::par(non_participant_lanes, sparse_route_arm_program())
}

type MultiPhaseProgramSteps = g::Seq<
    g::Send<0, 1, Msg<18, ()>>,
    g::Seq<g::Par<ParallelLane0, ParallelLane1>, g::Send<0, 1, Msg<19, ()>>>,
>;
fn multi_resident_row_program() -> Program<MultiPhaseProgramSteps> {
    g::seq(
        g::send::<0, 1, Msg<18, ()>>(),
        g::seq(parallel_program(), g::send::<0, 1, Msg<19, ()>>()),
    )
}

type RolledSeqProgramSteps =
    g::Roll<g::Seq<g::Send<0, 1, Msg<201, ()>>, g::Send<1, 0, Msg<202, ()>>>>;

fn rolled_seq_program() -> Program<RolledSeqProgramSteps> {
    g::seq(
        g::send::<0, 1, Msg<201, ()>>(),
        g::send::<1, 0, Msg<202, ()>>(),
    )
    .roll()
}

fn roll_route_internal_parallel_program() -> impl Projectable {
    let left = g::seq(
        g::send::<1, 1, Msg<145, ()>>(),
        g::seq(
            g::send::<1, 2, Msg<87, u8>>(),
            g::seq(
                g::par(
                    g::seq(
                        g::send::<2, 3, Msg<153, u8>>(),
                        g::send::<3, 2, Msg<151, u8>>(),
                    ),
                    g::send::<2, 4, Msg<154, u8>>(),
                ),
                g::send::<2, 1, Msg<88, u8>>(),
            ),
        ),
    );
    let right = g::seq(
        g::send::<1, 1, Msg<146, ()>>(),
        g::seq(
            g::send::<1, 2, Msg<11, u8>>(),
            g::seq(
                g::send::<1, 3, Msg<155, u8>>(),
                g::send::<1, 4, Msg<156, u8>>(),
            ),
        ),
    );
    let routed = g::route(left, right).resolve::<ROLL_ROUTE_INTERNAL_PARALLEL_RESOLVER>();
    g::seq(
        g::send::<1, 2, Msg<1, u8>>(),
        g::seq(
            g::send::<2, 1, Msg<2, u8>>(),
            g::seq(
                g::send::<1, 2, Msg<3, u8>>(),
                g::seq(
                    g::send::<2, 1, Msg<4, u8>>(),
                    g::seq(
                        g::send::<1, 2, Msg<5, u8>>(),
                        g::seq(g::send::<2, 1, Msg<6, u8>>(), routed),
                    ),
                ),
            ),
        ),
    )
}

#[test]
fn roll_projection_marks_seq_body_with_roll_scope() {
    let program: RoleProgram<0> = project(&rolled_seq_program());
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        let first = rows
            .local_step_node(0)
            .expect("rolled seq first local event");
        let second = rows
            .local_step_node(1)
            .expect("rolled seq second local event");
        assert_eq!(first.scope().kind(), Some(ScopeKind::Roll));
        assert_eq!(second.scope().kind(), Some(ScopeKind::Roll));
        assert_eq!(first.scope(), second.scope());
    });
}

#[test]
fn equal_range_nested_rolls_assign_events_to_the_innermost_scope() {
    let program: RoleProgram<0> = project(&g::send::<0, 1, Msg<203, ()>>().roll().roll().roll());
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        let event = rows.local_step_node(0).expect("triple rolled event");
        assert_eq!(event.scope().kind(), Some(ScopeKind::Roll));
        assert_eq!(event.scope().local_ordinal(), 2);
        assert!(rows.local_step_node(1).is_none());
    });
}

#[test]
fn rolled_nested_route_keeps_inner_arm_conflicts() {
    let a = g::seq(
        g::send::<0, 1, Msg<181, ()>>(),
        g::send::<1, 0, Msg<182, ()>>(),
    );
    let b = g::seq(
        g::send::<0, 1, Msg<183, ()>>(),
        g::send::<1, 0, Msg<184, ()>>(),
    );
    let c = g::seq(
        g::send::<0, 1, Msg<185, ()>>(),
        g::send::<1, 0, Msg<186, ()>>(),
    );
    let program: RoleProgram<0> = project(&g::route(a, g::route(b, c)).roll());
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        let mut b_scope = None;
        let mut c_scope = None;
        let mut idx = 0usize;
        while idx < rows.local_step_count() {
            let Some(node) = rows.local_step_node(idx) else {
                break;
            };
            let label = match node.action() {
                LocalAction::Send { label, .. } | LocalAction::Recv { label, .. } => label,
                LocalAction::Local { label, .. } => label,
                LocalAction::Terminate => {
                    idx += 1;
                    continue;
                }
            };
            match (label, rows.event_conflict_for_index(idx).to_conflict()) {
                (183, Some(LocalConflict::RouteArm { scope, arm })) => {
                    b_scope = Some(scope);
                    assert_eq!(arm, 0);
                }
                (185, Some(LocalConflict::RouteArm { scope, arm })) => {
                    c_scope = Some(scope);
                    assert_eq!(arm, 1);
                }
                _ => {}
            }
            idx += 1;
        }
        assert_eq!(b_scope, c_scope);
    });
}

#[test]
fn parallel_projection_keeps_resident_rows_and_lane_split_internal() {
    let parallel_program = parallel_program();
    let client: RoleProgram<0> = project(&parallel_program);
    let server: RoleProgram<1> = project(&parallel_program);

    with_role_descriptor(&client, assert_parallel_resident_row_shape);
    with_role_descriptor(&server, assert_parallel_resident_row_shape);
}

#[test]
fn resident_rows_cover_multiple_exact_layout_rows() {
    let program: RoleProgram<0> = project(&multi_resident_row_program());
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        assert_eq!(rows.resident_row_min_start(0), Some(0));
        assert_eq!(
            rows.resident_row_lane_steps(0, 0).map(|steps| steps.len),
            Some(1)
        );
        assert!(rows.resident_row_lane_steps(0, 1).is_none());

        assert_eq!(rows.resident_row_min_start(1), Some(1));
        assert_eq!(rows.resident_row_lane_step_at(1, 0, 0), Some(1));
        assert_eq!(rows.resident_row_lane_step_at(1, 1, 0), Some(2));

        assert_eq!(rows.resident_row_min_start(2), Some(3));
        assert_eq!(
            rows.resident_row_lane_steps(2, 0).map(|steps| steps.len),
            Some(1)
        );
        assert!(rows.resident_row_lane_steps(2, 1).is_none());
        assert!(rows.resident_row_min_start(3).is_none());
    });
}

#[test]
fn route_internal_parallel_scope_has_exact_resident_arm_relation() {
    let program: RoleProgram<2> = project(&roll_route_internal_parallel_program());
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        let mut found_route_internal_parallel = false;
        let mut step_idx = 0usize;
        while step_idx < rows.local_step_count() {
            if let Some(node) = rows.local_step_node(step_idx)
                && node.scope().kind() == Some(ScopeKind::Parallel)
            {
                match rows.event_conflict_for_index(step_idx).to_conflict() {
                    Some(LocalConflict::RouteArm { scope, arm }) => {
                        assert_eq!(
                            scope.kind(),
                            Some(ScopeKind::Route),
                            "parallel body event must carry an enclosing route conflict"
                        );
                        assert_eq!(
                            arm, 0,
                            "parallel scope under the continue arm must carry exact route arm relation"
                        );
                        found_route_internal_parallel = true;
                    }
                    other => panic!(
                        "parallel body event must carry a route-arm conflict row, got {other:?}"
                    ),
                }
            }
            step_idx += 1;
        }
        assert!(
            found_route_internal_parallel,
            "test program must contain a route-internal parallel scope"
        );
    });
}

#[test]
fn parallel_route_projection_keeps_resident_descriptor_without_public_step_surface() {
    let parallel_route_program = parallel_route_program();
    let program: RoleProgram<0> = project(&parallel_route_program);
    with_role_descriptor(&program, |descriptor| {
        assert!(
            descriptor
                .local_event_rows()
                .resident_row_lane_steps(0, 0)
                .is_some(),
            "parallel projection should preserve compact lane step facts"
        );
        assert!(
            descriptor.route_scope_count() > 0,
            "route projection should preserve resident route scope facts"
        );
    });
}

mod route_arm_lane_steps;
