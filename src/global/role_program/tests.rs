use super::*;
use crate::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
use crate::eff::{EffAtom, EffStruct};
use crate::g::{self, ControlMsg, Msg, Program};
use crate::global::compiled::images::{
    PROGRAM_IMAGE_ATOM_STRIDE, PROGRAM_IMAGE_CONTROL_DESC_STRIDE, PROGRAM_IMAGE_POLICY_STRIDE,
    PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE, ProgramColumnRange, RoleDescriptorRef,
};
use crate::global::const_dsl::{EffList, ScopeKind};
use crate::global::program::Projectable;
use crate::global::typestate::LocalConflict;
use std::println;

#[macro_use]
mod final_form_protocol_matrix;
#[macro_use]
mod final_form_protocol_measure_roles;

const TAP_EVENT_STRESS_ROW_BUDGET: usize = 512;

const fn test_atom(label: u8, lane: u8) -> EffStruct {
    EffStruct::atom(EffAtom {
        from: 0,
        to: 1,
        label,
        is_control: false,
        resource: None,
        lane,
    })
}

const fn over_tap_event_atom_program() -> EffList {
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx <= TAP_EVENT_STRESS_ROW_BUDGET {
        list = list.push(test_atom(idx as u8, (idx % LANE_DOMAIN_SIZE) as u8));
        idx += 1;
    }
    list
}

static OVER_TAP_EVENT_ATOMS: EffList = over_tap_event_atom_program();

static OVER_TAP_EVENT_IMAGE: CompiledProgramImage =
    CompiledProgramImage::scan_const(&OVER_TAP_EVENT_ATOMS);

fn with_role_descriptor<const ROLE: u8, R>(
    program: &RoleProgram<ROLE>,
    f: impl FnOnce(RoleDescriptorRef) -> R,
) -> R {
    f(RoleDescriptorRef::from_resident(program.role_image_ref()))
}

#[derive(Clone, Copy)]
struct ProtocolMatrixMeasurement {
    program_blob_len: usize,
    role_blob_len: usize,
    endpoint_scratch_bytes: usize,
    largest_section_bytes: usize,
}

impl ProtocolMatrixMeasurement {
    const EMPTY: Self = Self {
        program_blob_len: 0,
        role_blob_len: 0,
        endpoint_scratch_bytes: 0,
        largest_section_bytes: 0,
    };

    fn max(self, other: Self) -> Self {
        Self {
            program_blob_len: self.program_blob_len.max(other.program_blob_len),
            role_blob_len: self.role_blob_len.max(other.role_blob_len),
            endpoint_scratch_bytes: self
                .endpoint_scratch_bytes
                .max(other.endpoint_scratch_bytes),
            largest_section_bytes: self.largest_section_bytes.max(other.largest_section_bytes),
        }
    }
}

fn endpoint_largest_section(layout: crate::endpoint::kernel::EndpointArenaLayout) -> usize {
    let mut largest = layout.event_cursor_state().bytes;
    largest = largest.max(layout.decision_state().bytes);
    largest = largest.max(layout.route_arm_stack().bytes);
    largest = largest.max(layout.lane_offer_state_slots().bytes);
    largest = largest.max(layout.frontier_state().bytes);
    largest = largest.max(layout.frontier_root_rows().bytes);
    largest = largest.max(layout.frontier_root_active_slots().bytes);
    largest = largest.max(layout.frontier_root_observed_key_slots().bytes);
    largest = largest.max(layout.frontier_offer_entry_slots().bytes);
    largest = largest.max(layout.scope_evidence_slots().bytes);
    largest
}

fn pbl(column: ProgramColumnRange, stride: usize) -> usize {
    column.byte_len(stride)
}

fn rbl(column: ColumnRange, stride: usize) -> usize {
    column.byte_len(stride)
}

fn largest_program_section(
    program_ref: crate::global::compiled::images::CompiledProgramRef,
) -> usize {
    let columns = program_ref.columns;
    pbl(columns.atoms, PROGRAM_IMAGE_ATOM_STRIDE)
        .max(pbl(columns.policies, PROGRAM_IMAGE_POLICY_STRIDE))
        .max(pbl(
            columns.control_descs,
            PROGRAM_IMAGE_CONTROL_DESC_STRIDE,
        ))
        .max(pbl(
            columns.route_controls,
            PROGRAM_IMAGE_ROUTE_CONTROL_STRIDE,
        ))
}

fn largest_role_section(rows: RoleImageRef) -> usize {
    let columns = rows.columns;
    rbl(columns.events, ROLE_IMAGE_EVENT_STRIDE)
        .max(rbl(columns.lanes, ROLE_IMAGE_LANE_STRIDE))
        .max(rbl(columns.dependencies, ROLE_IMAGE_DEPENDENCY_STRIDE))
        .max(rbl(columns.conflicts, ROLE_IMAGE_CONFLICT_STRIDE))
        .max(rbl(columns.route_scopes, ROLE_IMAGE_U16_STRIDE))
        .max(rbl(
            columns.route_scope_conflicts,
            ROLE_IMAGE_CONFLICT_STRIDE,
        ))
        .max(rbl(columns.route_arms, ROLE_IMAGE_ROUTE_ARM_STRIDE))
        .max(rbl(columns.resident_boundaries, ROLE_IMAGE_U16_STRIDE))
        .max(rbl(columns.lane_bits, ROLE_IMAGE_LANE_STRIDE))
        .max(rbl(
            columns.route_arm_lane_rows,
            ROLE_IMAGE_LANE_RANGE_STRIDE,
        ))
        .max(rbl(
            columns.route_offer_lane_rows,
            ROLE_IMAGE_LANE_RANGE_STRIDE,
        ))
        .max(rbl(
            columns.route_arm_lane_step_rows,
            ROLE_IMAGE_ROUTE_ARM_LANE_STEP_STRIDE,
        ))
        .max(rbl(
            columns.route_commit_ranges,
            ROLE_IMAGE_LANE_RANGE_STRIDE,
        ))
        .max(rbl(columns.route_commit_rows, ROLE_IMAGE_CONFLICT_STRIDE))
}

fn measure_role<const ROLE: u8>(program: &RoleProgram<ROLE>) -> ProtocolMatrixMeasurement {
    let compiled = program.role_image_ref();
    let program_ref = *compiled.program;
    let descriptor = RoleDescriptorRef::from_resident(compiled);
    let rows = descriptor.local_event_rows();
    let endpoint_layout = descriptor.endpoint_arena_layout();
    ProtocolMatrixMeasurement {
        program_blob_len: program_ref.blob.len(),
        role_blob_len: rows.blob.len(),
        endpoint_scratch_bytes: endpoint_layout.total_bytes(),
        largest_section_bytes: largest_program_section(program_ref)
            .max(largest_role_section(rows))
            .max(endpoint_largest_section(endpoint_layout)),
    }
}

fn report_protocol_matrix(name: &str, measured: ProtocolMatrixMeasurement) {
    println!(
        "protocol-matrix name={name} program_blob_len={} role_blob_len={} endpoint_scratch_bytes={} largest_section_bytes={}",
        measured.program_blob_len,
        measured.role_blob_len,
        measured.endpoint_scratch_bytes,
        measured.largest_section_bytes
    );
    assert!(
        measured.program_blob_len < crate::eff::meta::MAX_EFF_NODES,
        "{name} program blob must not scale to MAX_EFF_NODES"
    );
    assert!(
        measured.role_blob_len < crate::eff::meta::MAX_EFF_NODES,
        "{name} role blob must not scale to MAX_EFF_NODES"
    );
}

#[test]
fn logical_lane_count_stays_inside_wire_lane_domain() {
    assert_eq!(logical_lane_count_for_role(0, 1), RESERVED_BINDING_LANES);
    assert_eq!(logical_lane_count_for_role(254, 255), LANE_DOMAIN_SIZE);
    assert_eq!(logical_lane_count_for_role(255, 256), LANE_DOMAIN_SIZE);
    assert_eq!(logical_lane_count_for_role(256, 256), LANE_DOMAIN_SIZE);
}

#[test]
fn lane_set_view_iterates_set_bits_without_empty_lane_scan() {
    let mut words = [0usize; 4];
    let (word, bit) = lane_word_index(3);
    words[word] |= bit;
    let (word, bit) = lane_word_index(usize::BITS as usize + 5);
    words[word] |= bit;
    let (word, bit) = lane_word_index(usize::BITS as usize * 2 + 1);
    words[word] |= bit;
    let view = LaneSetView::from_parts(words.as_ptr(), words.len());

    assert_eq!(view.first_set(256), Some(3));
    assert_eq!(view.next_set_from(4, 256), Some(usize::BITS as usize + 5));
    assert_eq!(
        view.next_set_from(usize::BITS as usize + 6, 256),
        Some(usize::BITS as usize * 2 + 1),
    );
    assert_eq!(view.next_set_from(usize::BITS as usize * 2 + 2, 256), None,);
    assert_eq!(view.next_set_from(usize::BITS as usize + 6, 65), None);
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

    let mut lhs = [0usize; 4];
    let mut rhs = [0usize; 4];
    let (word, bit) = lane_word_index(3);
    lhs[word] |= bit;
    rhs[word] |= bit;
    let (word, bit) = lane_word_index(usize::BITS as usize + 5);
    lhs[word] |= bit;
    rhs[word] |= bit;
    let (word, bit) = lane_word_index(usize::BITS as usize + 9);
    lhs[word] |= bit;
    let (word, bit) = lane_word_index(usize::BITS as usize * 3 + 7);
    rhs[word] |= bit;

    let lhs = LaneSetView::from_parts(lhs.as_ptr(), lhs.len());
    let rhs = LaneSetView::from_parts(rhs.as_ptr(), rhs.len());

    assert!(!equals_until(lhs, rhs, usize::BITS as usize * 2));
    assert!(equals_until_except_lane(
        lhs,
        rhs,
        usize::BITS as usize * 2,
        usize::BITS as usize + 9
    ));
    assert!(
        equals_until_except_lane(lhs, rhs, usize::BITS as usize * 3, usize::BITS as usize + 9),
        "bits beyond the active lane limit are not semantic lane state"
    );
}

#[test]
fn resident_lane_view_and_route_caps_stay_compact() {
    assert!(
        core::mem::size_of::<LaneSetView<'static>>() <= 2 * core::mem::size_of::<usize>(),
        "LaneSetView must stay a borrowed word/list descriptor, not a copied lane set"
    );
    assert!(
        core::mem::size_of::<RoleLaneImage>() <= 24 * core::mem::size_of::<usize>(),
        "resident RoleLaneImage must stay a ptr+len column view, not a copied max-capacity image"
    );
    assert_eq!(
        MAX_LOCAL_STEP_LANES,
        crate::eff::meta::MAX_EFF_NODES,
        "max local rows are scratch/projection capacity only"
    );
    assert!(MAX_ROUTE_SCOPE_LANE_ROWS >= crate::eff::meta::MAX_EFF_NODES / 2);
    assert_eq!(MAX_ROUTE_ARM_LANE_ROWS, MAX_ROUTE_SCOPE_LANE_ROWS * 2);
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
    assert_eq!(core::mem::size_of::<ProgramColumnRange>(), 4);
    assert!(
        core::mem::size_of::<RoleImageColumns>() < 15 * 5,
        "RoleImageColumns must not retain stride or removed passive metadata"
    );
    assert!(
        core::mem::size_of::<RuntimeRoleFacts>() < 14 * core::mem::size_of::<u16>(),
        "RuntimeRoleFacts must stay below the removed 14-word fact block"
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

    let largest_resident_column = columns
        .events
        .len
        .max(columns.lanes.len)
        .max(columns.dependencies.len)
        .max(columns.conflicts.len)
        .max(columns.route_scopes.len)
        .max(columns.route_scope_conflicts.len)
        .max(columns.route_arms.len)
        .max(columns.resident_boundaries.len)
        .max(columns.lane_bits.len)
        .max(columns.route_arm_lane_rows.len)
        .max(columns.route_offer_lane_rows.len)
        .max(columns.route_arm_lane_step_rows.len) as usize;
    assert!(
        largest_resident_column < MAX_LOCAL_STEP_LANES,
        "small protocol descriptor must not scale to MAX_EFF_NODES"
    );
    assert!(
        rows.blob.len() < MAX_LOCAL_STEP_LANES,
        "small protocol blob must stay byte-exact, not max-capacity"
    );
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
fn projected_protocol_matrix_reports_compact_resident_images() {
    macro_rules! report {
        ($name:ident) => {{
            let program = final_form_protocol!($name);
            report_protocol_matrix(
                stringify!($name),
                final_form_protocol_measure_roles!($name, &program),
            );
        }};
    }
    report!(minimal_send_recv);
    report!(nested_par_join);
    report!(route_with_unselected_nested_par);
    report!(triple_nested_route);
    report!(passive_nested_route_observer);
    report!(alternating_par_route);
    report!(huge_legal_choreography);
}

#[test]
fn resident_local_step_capacity_is_not_tied_to_tap_events() {
    assert!(OVER_TAP_EVENT_ATOMS.len() > TAP_EVENT_STRESS_ROW_BUDGET);
    let lanes = RoleLaneScratch::from_program::<0>(&OVER_TAP_EVENT_IMAGE, LANE_DOMAIN_SIZE);

    fn row_range(lanes: &RoleLaneScratch, idx: usize) -> Option<(usize, usize)> {
        if idx >= lanes.resident_row_len as usize {
            return None;
        }
        let start = lanes.resident_row_boundaries[idx] as usize;
        let end = lanes.resident_row_boundaries[idx + 1] as usize;
        Some((start, end))
    }

    fn lane_steps(lanes: &RoleLaneScratch, row_idx: usize, lane_idx: usize) -> usize {
        let Some((start, end)) = row_range(lanes, row_idx) else {
            return 0;
        };
        let mut count = 0usize;
        let mut pos = start;
        while pos < end {
            if lanes.local_step_lanes[pos] as usize == lane_idx {
                count += 1;
            }
            pos += 1;
        }
        count
    }

    fn lane_step_at(
        lanes: &RoleLaneScratch,
        row_idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        let (start, end) = row_range(lanes, row_idx)?;
        let mut seen = 0usize;
        let mut pos = start;
        while pos < end {
            if lanes.local_step_lanes[pos] as usize == lane_idx {
                if seen == ordinal {
                    return Some(pos as u16);
                }
                seen += 1;
            }
            pos += 1;
        }
        None
    }

    let mut total_steps = 0usize;
    let mut lane_idx = 0usize;
    while lane_idx < LANE_DOMAIN_SIZE {
        total_steps += lane_steps(&lanes, 0, lane_idx);
        lane_idx += 1;
    }
    assert_eq!(total_steps, OVER_TAP_EVENT_ATOMS.len());
    assert_eq!(lane_step_at(&lanes, 0, 0, 0), Some(0));
    assert_eq!(lane_step_at(&lanes, 0, 0, 1), Some(LANE_DOMAIN_SIZE as u16));
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

type OtherLane = g::Send<2, 3, Msg<108, ()>>;
type OtherLanes2 = g::Par<OtherLane, OtherLane>;
type OtherLanes4 = g::Par<OtherLanes2, OtherLanes2>;
type OtherLanes8 = g::Par<OtherLanes4, OtherLanes4>;
type OtherLanes16 = g::Par<OtherLanes8, OtherLanes8>;
type OtherLanes32 = g::Par<OtherLanes16, OtherLanes16>;
type OtherLanes63 = g::Par<
    OtherLanes32,
    g::Par<OtherLanes16, g::Par<OtherLanes8, g::Par<OtherLanes4, g::Par<OtherLanes2, OtherLane>>>>,
>;

fn other_lane() -> Program<g::Send<2, 3, Msg<108, ()>>> {
    g::send::<2, 3, Msg<108, ()>>()
}

fn other_lanes_2() -> Program<OtherLanes2> {
    g::par(other_lane(), other_lane())
}

fn other_lanes_4() -> Program<OtherLanes4> {
    g::par(other_lanes_2(), other_lanes_2())
}

fn other_lanes_8() -> Program<OtherLanes8> {
    g::par(other_lanes_4(), other_lanes_4())
}

fn other_lanes_16() -> Program<OtherLanes16> {
    g::par(other_lanes_8(), other_lanes_8())
}

fn other_lanes_32() -> Program<OtherLanes32> {
    g::par(other_lanes_16(), other_lanes_16())
}

fn other_lanes_63() -> Program<OtherLanes63> {
    g::par(
        other_lanes_32(),
        g::par(
            other_lanes_16(),
            g::par(
                other_lanes_8(),
                g::par(other_lanes_4(), g::par(other_lanes_2(), other_lane())),
            ),
        ),
    )
}

type SparseRouteHighLaneProgram = g::Par<OtherLanes63, SparseRouteArmProgram>;

fn sparse_route_high_lane_program() -> Program<SparseRouteHighLaneProgram> {
    g::par(other_lanes_63(), sparse_route_arm_program())
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

fn loop_route_internal_parallel_program() -> impl Projectable {
    let left = g::seq(
        g::send::<1, 1, ControlMsg<145, LoopContinueKind>>(),
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
        g::send::<1, 1, ControlMsg<146, LoopBreakKind>>(),
        g::send::<1, 2, Msg<11, u8>>(),
    );
    let routed = g::route(left, right);
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
    let program: RoleProgram<2> = project(&loop_route_internal_parallel_program());
    with_role_descriptor(&program, |descriptor| {
        let rows = descriptor.local_event_rows();
        let mut found_route_internal_parallel = false;
        let mut step_idx = 0usize;
        while step_idx < rows.local_step_count() {
            if let Some(node) = rows.local_step_node(step_idx)
                && node.scope().kind() == ScopeKind::Parallel
            {
                match rows.event_conflict_for_index(step_idx).to_conflict() {
                    Some(LocalConflict::RouteArm { scope, arm }) => {
                        assert_eq!(
                            scope.kind(),
                            ScopeKind::Route,
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
            "fixture must contain a route-internal parallel scope"
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

#[test]
fn lane_resident_route_rows_do_not_restore_full_domain_copies() {
    let packed_route_lane_rows = MAX_ROUTE_ARM_LANE_ROWS
        * core::mem::size_of::<PackedRouteArmRow>()
        + MAX_ROUTE_SCOPE_LANE_ROWS * core::mem::size_of::<PackedLaneRange>();
    let full_domain_route_lane_rows = (MAX_ROUTE_ARM_LANE_ROWS + MAX_ROUTE_SCOPE_LANE_ROWS)
        * LANE_SET_VIEW_WORDS
        * core::mem::size_of::<LaneWord>();

    assert!(
        packed_route_lane_rows < full_domain_route_lane_rows,
        "route lane rows must stay packed and must not restore full-domain lane-set copies: current={} full_domain={}",
        packed_route_lane_rows,
        full_domain_route_lane_rows
    );
    assert!(
        core::mem::size_of::<RouteArmLaneStepRow>()
            < LANE_SET_VIEW_WORDS * core::mem::size_of::<LaneWord>(),
        "one sparse first/last row must stay smaller than a full-domain lane-set row"
    );
}

#[test]
fn route_arm_row_packs_exact_ranges_into_one_word() {
    let separate_exact_range_columns =
        (core::mem::size_of::<PackedLaneRange>() * 2) + core::mem::size_of::<u8>();
    assert_eq!(
        core::mem::size_of::<PackedRouteArmRow>(),
        ROLE_IMAGE_ROUTE_ARM_STRIDE
    );
    assert!(
        ROLE_IMAGE_ROUTE_ARM_STRIDE < separate_exact_range_columns,
        "route arm row should keep event range, child delta, and lane-step range in one packed word"
    );
}

type SparseMultiLaneLeft = g::Seq<
    g::Send<0, 1, Msg<109, ()>>,
    g::Par<g::Send<0, 2, Msg<110, ()>>, g::Send<0, 3, Msg<111, ()>>>,
>;
type SparseMultiLaneRight = g::Seq<
    g::Send<0, 1, Msg<112, ()>>,
    g::Par<g::Send<0, 2, Msg<113, ()>>, g::Send<0, 3, Msg<114, ()>>>,
>;
type SparseMultiLaneRoute = g::Route<SparseMultiLaneLeft, SparseMultiLaneRight>;

fn sparse_multi_lane_route_program() -> Program<SparseMultiLaneRoute> {
    g::route(
        g::seq(
            g::send::<0, 1, Msg<109, ()>>(),
            g::par(
                g::send::<0, 2, Msg<110, ()>>(),
                g::send::<0, 3, Msg<111, ()>>(),
            ),
        ),
        g::seq(
            g::send::<0, 1, Msg<112, ()>>(),
            g::par(
                g::send::<0, 2, Msg<113, ()>>(),
                g::send::<0, 3, Msg<114, ()>>(),
            ),
        ),
    )
}

mod route_arm_lane_steps;
