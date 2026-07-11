use super::*;
use crate::eff::{EffAtom, EffStruct, EventOrigin};
use crate::g::{self, Msg, Program};
use crate::global::compiled::images::{CompiledProgramRef, ProgramColumnRange, RoleDescriptorRef};
use crate::global::const_dsl::{EffList, ScopeKind};
use crate::global::event_program::LocalEventProgram;
use crate::global::program::Projectable;
use crate::global::typestate::{LocalAction, LocalConflict};

#[macro_use]
#[path = "tests/final_form_protocol_matrix.rs"]
mod final_form_protocol_matrix;
mod protocol_matrix;

const LOCAL_STEP_STRESS_ROW_BUDGET: usize = 512;
const _: () = assert!(MAX_ROUTE_SCOPE_LANE_ROWS >= crate::eff::meta::MAX_EFF_NODES / 2);
const NESTED_PAR_ROUTE_RESOLVER: u16 = 0x91;

const fn test_atom(label: u8, lane: u8) -> EffStruct {
    EffStruct::atom(EffAtom {
        from: 0,
        to: 1,
        label,
        origin: EventOrigin::User,
        lane,
    })
}

const fn over_local_step_capacity_atom_program() -> EffList {
    let mut list = EffList::new();
    let mut idx = 0usize;
    while idx <= LOCAL_STEP_STRESS_ROW_BUDGET {
        list = list.push(test_atom(idx as u8, (idx % LANE_DOMAIN_SIZE) as u8));
        idx += 1;
    }
    list
}

static OVER_LOCAL_STEP_CAPACITY_ATOMS: EffList = over_local_step_capacity_atom_program();

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
    let right = g::send::<0, 1, Msg<33, u8>>();
    let route = g::route(left, right).resolve::<NESTED_PAR_ROUTE_RESOLVER>();
    let role0: RoleProgram<0> = project(&route);
    let program_ref = role0.role_image_ref().program;

    assert_eq!(program_ref.route_resolver_row_count(), 1);
    let scope = program_ref
        .route_resolver_scope_at_row(0)
        .expect("route scope row");
    assert_eq!(
        program_ref.route_resolver_id_at_row(0),
        Some(NESTED_PAR_ROUTE_RESOLVER)
    );
    assert_eq!(program_ref.route_controller_role(scope), Some(0));
    assert!(program_ref.route_resolver(scope).is_some());

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
    assert_eq!(logical_lane_count_for_role(0, 1), MIN_ENDPOINT_LANE_SLOTS);
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
    let word = core::mem::size_of::<usize>();
    assert!(
        core::mem::size_of::<BlobPtr>() == word
            && core::mem::size_of::<CompiledProgramRef>() <= 4 * word
            && core::mem::size_of::<RoleImageRef>() <= 12 * word
            && core::mem::size_of::<RoleLaneImage<'static>>() <= 2 * word,
        "resident refs must stay thin blob column views without fat-slice lengths"
    );
    assert_eq!(
        MAX_LOCAL_STEP_LANES,
        crate::eff::meta::MAX_EFF_NODES,
        "max local rows are scratch/projection capacity only"
    );
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
        rows.columns.blob_len() < MAX_LOCAL_STEP_LANES,
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
fn resident_local_step_capacity_uses_effect_node_budget() {
    assert!(OVER_LOCAL_STEP_CAPACITY_ATOMS.len() > LOCAL_STEP_STRESS_ROW_BUDGET);
    let lanes = RoleLaneScratch::from_program(&OVER_LOCAL_STEP_CAPACITY_ATOMS, LANE_DOMAIN_SIZE, 0);

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
    assert_eq!(total_steps, OVER_LOCAL_STEP_CAPACITY_ATOMS.len());
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
        g::send::<2, 3, Msg<108, ()>>(),
        g::send::<2, 3, Msg<109, ()>>(),
        g::send::<2, 3, Msg<110, ()>>(),
        g::send::<2, 3, Msg<111, ()>>(),
        g::send::<2, 3, Msg<112, ()>>(),
        g::send::<2, 3, Msg<113, ()>>(),
        g::send::<2, 3, Msg<114, ()>>(),
        g::send::<2, 3, Msg<115, ()>>(),
        g::send::<2, 3, Msg<116, ()>>(),
        g::send::<2, 3, Msg<117, ()>>(),
        g::send::<2, 3, Msg<118, ()>>(),
        g::send::<2, 3, Msg<119, ()>>(),
        g::send::<2, 3, Msg<120, ()>>(),
        g::send::<2, 3, Msg<121, ()>>(),
        g::send::<2, 3, Msg<122, ()>>(),
        g::send::<2, 3, Msg<123, ()>>(),
        g::send::<2, 3, Msg<124, ()>>(),
        g::send::<2, 3, Msg<125, ()>>(),
        g::send::<2, 3, Msg<126, ()>>(),
        g::send::<2, 3, Msg<127, ()>>(),
        g::send::<2, 3, Msg<128, ()>>(),
        g::send::<2, 3, Msg<129, ()>>(),
        g::send::<2, 3, Msg<130, ()>>(),
        g::send::<2, 3, Msg<131, ()>>(),
        g::send::<2, 3, Msg<132, ()>>(),
        g::send::<2, 3, Msg<133, ()>>(),
        g::send::<2, 3, Msg<134, ()>>(),
        g::send::<2, 3, Msg<135, ()>>(),
        g::send::<2, 3, Msg<136, ()>>(),
        g::send::<2, 3, Msg<137, ()>>(),
        g::send::<2, 3, Msg<138, ()>>(),
        g::send::<2, 3, Msg<139, ()>>(),
        g::send::<2, 3, Msg<140, ()>>(),
        g::send::<2, 3, Msg<141, ()>>(),
        g::send::<2, 3, Msg<142, ()>>(),
        g::send::<2, 3, Msg<143, ()>>(),
        g::send::<2, 3, Msg<144, ()>>(),
        g::send::<2, 3, Msg<145, ()>>(),
        g::send::<2, 3, Msg<146, ()>>(),
        g::send::<2, 3, Msg<147, ()>>(),
        g::send::<2, 3, Msg<148, ()>>(),
        g::send::<2, 3, Msg<149, ()>>(),
        g::send::<2, 3, Msg<150, ()>>(),
        g::send::<2, 3, Msg<151, ()>>(),
        g::send::<2, 3, Msg<152, ()>>(),
        g::send::<2, 3, Msg<153, ()>>(),
        g::send::<2, 3, Msg<154, ()>>(),
        g::send::<2, 3, Msg<155, ()>>(),
        g::send::<2, 3, Msg<156, ()>>(),
        g::send::<2, 3, Msg<157, ()>>(),
        g::send::<2, 3, Msg<158, ()>>(),
        g::send::<2, 3, Msg<159, ()>>(),
        g::send::<2, 3, Msg<160, ()>>(),
        g::send::<2, 3, Msg<161, ()>>(),
        g::send::<2, 3, Msg<162, ()>>(),
        g::send::<2, 3, Msg<163, ()>>(),
        g::send::<2, 3, Msg<164, ()>>(),
        g::send::<2, 3, Msg<165, ()>>(),
        g::send::<2, 3, Msg<166, ()>>(),
        g::send::<2, 3, Msg<167, ()>>(),
        g::send::<2, 3, Msg<168, ()>>(),
        g::send::<2, 3, Msg<169, ()>>(),
        g::send::<2, 3, Msg<170, ()>>(),
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
        g::send::<1, 1, Msg<145, u8>>(),
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
        g::send::<1, 1, Msg<146, u8>>(),
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
fn route_arm_row_keeps_exact_ranges_in_compact_scalar_limbs() {
    let separate_exact_range_columns =
        (core::mem::size_of::<PackedLaneRange>() * 2) + core::mem::size_of::<u8>();
    assert_eq!(
        core::mem::size_of::<PackedRouteArmRow>(),
        ROLE_IMAGE_ROUTE_ARM_STRIDE
    );
    assert!(
        ROLE_IMAGE_ROUTE_ARM_STRIDE < separate_exact_range_columns,
        "route arm row should keep event range, child delta, and lane-step range in one compact scalar row"
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
