use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eff::{EffAtom, EffStruct};
    use crate::g::{self, Msg, Role};
    use crate::global::compiled::images::RoleDescriptorRef;
    use crate::global::const_dsl::EffList;
    use crate::global::program::boundary_source_program_image;
    use crate::global::steps::{self, ParSteps, RouteSteps, SeqSteps, StepCons, StepNil};

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

        assert!(!lhs.equals_until(rhs, usize::BITS as usize * 2));
        assert!(lhs.equals_until_except_lane(
            rhs,
            usize::BITS as usize * 2,
            usize::BITS as usize + 9
        ));
        assert!(
            lhs.equals_until_except_lane(rhs, usize::BITS as usize * 3, usize::BITS as usize + 9),
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
            .phase_lane_steps(0, 0)
            .expect("lane 0 must cover every local atom");
        assert_eq!(steps.len as usize, OVER_TAP_EVENT_ATOMS.len());
        assert!(steps.is_contiguous());
        assert_eq!(
            lanes.phase_lane_step_at(0, 0, OVER_TAP_EVENT_ATOMS.len() - 1),
            Some((OVER_TAP_EVENT_ATOMS.len() - 1) as u16)
        );
    }

    fn assert_parallel_phase_shape(image: RoleDescriptorRef) {
        let phase_lane_set = image.phase_lane_set(0).expect("phase lane set");
        let mut lanes = [u8::MAX; 2];
        assert_eq!(
            phase_lane_set.write_lane_indices(image.logical_lane_count(), &mut lanes),
            2
        );
        assert_eq!(lanes, [0, 1]);
        assert_eq!(image.phase_lane_steps(0, 0).map(|steps| steps.len), Some(1));
        assert_eq!(image.phase_lane_steps(0, 1).map(|steps| steps.len), Some(1));
        assert!(image.phase_lane_set(1).is_none());
    }

    type ParallelLane0 = StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil>;
    type ParallelLane1 = StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>;
    fn parallel_lane0_program() -> Program<ParallelLane0> {
        g::send::<Role<0>, Role<1>, Msg<9, ()>, 0>()
    }
    fn parallel_lane1_program() -> Program<ParallelLane1> {
        g::send::<Role<1>, Role<0>, Msg<10, ()>, 1>()
    }
    fn parallel_program() -> Program<ParSteps<ParallelLane0, ParallelLane1>> {
        g::par(parallel_lane0_program(), parallel_lane1_program())
    }

    type RouteLeft = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<14, ()>, 0>, StepNil>,
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<15, ()>, 0>, StepNil>,
    >;
    type RouteRight = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<16, ()>, 0>, StepNil>,
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<17, ()>, 0>, StepNil>,
    >;
    fn route_left_program() -> Program<RouteLeft> {
        g::seq(
            g::send::<Role<0>, Role<0>, Msg<14, ()>, 0>(),
            g::send::<Role<0>, Role<1>, Msg<15, ()>, 0>(),
        )
    }
    fn route_right_program() -> Program<RouteRight> {
        g::seq(
            g::send::<Role<0>, Role<0>, Msg<16, ()>, 0>(),
            g::send::<Role<0>, Role<1>, Msg<17, ()>, 0>(),
        )
    }
    type RouteProgramSteps = RouteSteps<RouteLeft, RouteRight>;
    fn route_program() -> Program<RouteProgramSteps> {
        g::route(route_left_program(), route_right_program())
    }
    fn parallel_route_program() -> Program<ParSteps<ParallelLane1, RouteProgramSteps>> {
        g::par(parallel_lane1_program(), route_program())
    }

    type MultiPhaseProgramSteps = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<18, ()>, 0>, StepNil>,
        SeqSteps<
            ParSteps<ParallelLane0, ParallelLane1>,
            StepCons<steps::SendStep<Role<0>, Role<1>, Msg<19, ()>, 0>, StepNil>,
        >,
    >;
    fn multi_phase_program() -> Program<MultiPhaseProgramSteps> {
        g::seq(
            g::send::<Role<0>, Role<1>, Msg<18, ()>, 0>(),
            g::seq(
                parallel_program(),
                g::send::<Role<0>, Role<1>, Msg<19, ()>, 0>(),
            ),
        )
    }

    type SplitRouteLeft = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<20, ()>, 0>, StepNil>,
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<21, ()>, 0>, StepNil>,
    >;
    type SplitRouteRight = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<22, ()>, 1>, StepNil>,
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<23, ()>, 1>, StepNil>,
    >;
    type SplitRouteProgramSteps = RouteSteps<SplitRouteLeft, SplitRouteRight>;
    fn split_route_left_program() -> Program<SplitRouteLeft> {
        g::seq(
            g::send::<Role<0>, Role<0>, Msg<20, ()>, 0>(),
            g::send::<Role<0>, Role<1>, Msg<21, ()>, 0>(),
        )
    }
    fn split_route_right_program() -> Program<SplitRouteRight> {
        g::seq(
            g::send::<Role<0>, Role<0>, Msg<22, ()>, 1>(),
            g::send::<Role<0>, Role<1>, Msg<23, ()>, 1>(),
        )
    }
    fn split_route_program() -> Program<SplitRouteProgramSteps> {
        g::route(split_route_left_program(), split_route_right_program())
    }

    #[test]
    fn parallel_projection_keeps_phase_and_lane_split_internal() {
        let parallel_program = parallel_program();
        let client: RoleProgram<0> = project(&parallel_program);
        let server: RoleProgram<1> = project(&parallel_program);

        with_role_descriptor(&client, assert_parallel_phase_shape);
        with_role_descriptor(&server, assert_parallel_phase_shape);
    }

    #[test]
    fn resident_phase_rows_cover_multiple_exact_phases() {
        let program: RoleProgram<0> = project(&multi_phase_program());
        with_role_descriptor(&program, |descriptor| {
            let mut lanes = [u8::MAX; 2];

            let phase0 = descriptor.phase_lane_set(0).expect("pre-par phase");
            assert_eq!(
                phase0.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                1
            );
            assert_eq!(lanes[0], 0);
            assert_eq!(descriptor.phase_min_start(0), Some(0));
            assert_eq!(
                descriptor.phase_lane_steps(0, 0).map(|steps| steps.len),
                Some(1)
            );

            lanes = [u8::MAX; 2];
            let phase1 = descriptor.phase_lane_set(1).expect("parallel phase");
            assert_eq!(
                phase1.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                2
            );
            assert_eq!(lanes, [0, 1]);
            assert_eq!(descriptor.phase_min_start(1), Some(1));
            assert_eq!(descriptor.phase_lane_step_at(1, 0, 0), Some(1));
            assert_eq!(descriptor.phase_lane_step_at(1, 1, 0), Some(2));

            lanes = [u8::MAX; 2];
            let phase2 = descriptor.phase_lane_set(2).expect("post-par phase");
            assert_eq!(
                phase2.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                1
            );
            assert_eq!(lanes[0], 0);
            assert_eq!(descriptor.phase_min_start(2), Some(3));
            assert!(descriptor.phase_lane_set(3).is_none());
        });
    }

    #[test]
    fn resident_lane_step_lookup_keeps_noncontiguous_lane_order() {
        let program = g::seq(
            g::send::<Role<0>, Role<1>, Msg<31, ()>, 0>(),
            g::seq(
                g::send::<Role<0>, Role<1>, Msg<32, ()>, 1>(),
                g::send::<Role<0>, Role<1>, Msg<33, ()>, 0>(),
            ),
        );
        let program: RoleProgram<0> = project(&program);
        with_role_descriptor(&program, |descriptor| {
            let lane0 = descriptor.phase_lane_steps(0, 0).expect("lane 0 steps");
            assert_eq!(lane0.start, 0);
            assert_eq!(lane0.len, 2);
            assert!(!lane0.is_contiguous());
            assert_eq!(descriptor.phase_lane_step_at(0, 0, 0), Some(0));
            assert_eq!(descriptor.phase_lane_step_at(0, 0, 1), Some(2));
            assert_eq!(descriptor.phase_lane_step_at(0, 1, 0), Some(1));
            assert_eq!(descriptor.phase_lane_step_ordinal(0, 0, 0), Some(0));
            assert_eq!(descriptor.phase_lane_step_ordinal(0, 0, 2), Some(1));
            assert_eq!(descriptor.phase_lane_step_ordinal(0, 0, 1), None);
        });
    }

    #[test]
    fn parallel_route_projection_keeps_resident_descriptor_without_public_step_surface() {
        let parallel_route_program = parallel_route_program();
        let program: RoleProgram<0> = project(&parallel_route_program);
        with_role_descriptor(&program, |descriptor| {
            assert!(
                descriptor.phase_lane_set(0).is_some(),
                "parallel projection should preserve resident phase lane facts"
            );
            assert!(
                descriptor.route_scope_count() > 0,
                "route projection should preserve resident route scope facts"
            );
        });
    }

    #[test]
    fn route_arm_lane_rows_are_resident_and_exact() {
        let route_program = split_route_program();
        let program: RoleProgram<0> = project(&route_program);
        with_role_descriptor(&program, |descriptor| {
            let arm0 = descriptor
                .route_scope_arm_lane_set_by_slot(0, 0)
                .expect("arm 0 route lane row");
            let arm1 = descriptor
                .route_scope_arm_lane_set_by_slot(0, 1)
                .expect("arm 1 route lane row");
            let offer = descriptor
                .route_scope_offer_lane_set_by_slot(0)
                .expect("route offer lane row");
            let mut lanes = [u8::MAX; 2];

            assert_eq!(
                arm0.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                1
            );
            assert_eq!(lanes[0], 0);
            lanes = [u8::MAX; 2];
            assert_eq!(
                arm1.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                1
            );
            assert_eq!(lanes[0], 1);
            lanes = [u8::MAX; 2];
            assert_eq!(
                offer.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                2
            );
            assert_eq!(lanes, [0, 1]);
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
