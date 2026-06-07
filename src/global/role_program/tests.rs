use super::*;
#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
    use crate::eff::{EffAtom, EffStruct};
    use crate::g::{self, ControlMsg, Msg};
    use crate::global::compiled::images::RoleDescriptorRef;
    use crate::global::const_dsl::{EffList, ScopeEvent, ScopeKind};
    use crate::global::program::{Projectable, boundary_source_program_image};

    const LEGACY_TAP_EVENT_ROW_BUDGET: usize = 512;

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
        while idx <= LEGACY_TAP_EVENT_ROW_BUDGET {
            list = list.push(test_atom(idx as u8, 0));
            idx += 1;
        }
        list
    }

    static OVER_TAP_EVENT_ATOMS: EffList = over_tap_event_atom_program();

    static OVER_TAP_EVENT_IMAGE: CompiledProgramImage =
        boundary_source_program_image(&OVER_TAP_EVENT_ATOMS);

    fn with_role_descriptor<const ROLE: u8, R>(
        program: &RoleProgram<ROLE>,
        f: impl FnOnce(RoleDescriptorRef) -> R,
    ) -> R {
        f(RoleDescriptorRef::from_resident(
            program.compiled_role_image(),
        ))
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
        assert_eq!(MAX_LOCAL_STEP_LANES, crate::eff::meta::MAX_EFF_NODES);
        assert!(MAX_ROUTE_SCOPE_LANE_ROWS >= crate::eff::meta::MAX_EFF_NODES / 2);
        assert_eq!(MAX_ROUTE_ARM_LANE_ROWS, MAX_ROUTE_SCOPE_LANE_ROWS * 2);
    }

    #[test]
    fn resident_local_step_capacity_is_not_tied_to_tap_events() {
        assert!(OVER_TAP_EVENT_ATOMS.len() > LEGACY_TAP_EVENT_ROW_BUDGET);
        let lanes = RoleLaneImage::from_program::<0>(
            &OVER_TAP_EVENT_IMAGE,
            logical_lane_count_for_role(1, RESERVED_BINDING_LANES),
        );

        let steps = lanes
            .resident_row_lane_steps(0, 0)
            .expect("lane 0 must cover every local atom");
        assert_eq!(steps.len as usize, OVER_TAP_EVENT_ATOMS.len());
        assert!(steps.is_contiguous());
        assert_eq!(
            lanes.resident_row_lane_step_at(0, 0, OVER_TAP_EVENT_ATOMS.len() - 1),
            Some((OVER_TAP_EVENT_ATOMS.len() - 1) as u16)
        );
    }

    fn assert_parallel_resident_row_shape(image: RoleDescriptorRef) {
        assert_eq!(
            image.resident_row_lane_steps(0, 0).map(|steps| steps.len),
            Some(1)
        );
        assert_eq!(
            image.resident_row_lane_steps(0, 1).map(|steps| steps.len),
            Some(1)
        );
        assert!(image.resident_row_lane_steps(1, 0).is_none());
        assert!(image.resident_row_lane_steps(1, 1).is_none());
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
        let markers = program
            .compiled_role_image()
            .program_image()
            .view()
            .scope_markers();
        with_role_descriptor(&program, |descriptor| {
            let mut found_route_internal_parallel = false;
            let mut marker_idx = 0usize;
            while marker_idx < markers.len() {
                let marker = markers[marker_idx];
                if matches!(marker.event, ScopeEvent::Enter)
                    && marker.scope_kind == ScopeKind::Parallel
                    && let Some(parent) = descriptor.scope_parent(marker.scope_id)
                    && parent.kind() == ScopeKind::Route
                {
                    found_route_internal_parallel = true;
                    assert_eq!(
                        descriptor.route_parent(parent),
                        None,
                        "fixture route is the outer selected route"
                    );
                    assert_eq!(
                        descriptor.route_parent(marker.scope_id),
                        Some(parent),
                        "parallel scope must be related to its enclosing route"
                    );
                    assert_eq!(
                        descriptor.route_parent_arm(marker.scope_id),
                        Some(0),
                        "parallel scope under the continue arm must carry exact route arm relation"
                    );
                }
                marker_idx += 1;
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
        let packed_route_lane_rows = (MAX_ROUTE_ARM_LANE_ROWS + MAX_ROUTE_SCOPE_LANE_ROWS)
            * core::mem::size_of::<PackedLaneRange>();
        let full_domain_route_lane_rows = (MAX_ROUTE_ARM_LANE_ROWS + MAX_ROUTE_SCOPE_LANE_ROWS)
            * LANE_SET_VIEW_WORDS
            * core::mem::size_of::<LaneWord>();

        assert!(
            packed_route_lane_rows < full_domain_route_lane_rows,
            "route lane rows must stay packed and must not restore full-domain lane-set copies: current={} full_domain={}",
            packed_route_lane_rows,
            full_domain_route_lane_rows
        );
    }
}
