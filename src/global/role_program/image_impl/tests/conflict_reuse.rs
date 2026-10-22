use super::*;
use crate::global::{
    const_dsl::ScopeId,
    role_program::{BlobPtr, RoleLaneImage},
    typestate::LocalConflict,
};
use crate::{
    g::{Msg, Par, ProgramShape, ProgramSourceData, Roll, Route, Send, Seq},
    global::compiled::lowering::CompiledProgramImage,
};
use std::{boxed::Box, format, string::String};

#[path = "conflict_reuse/original.rs"]
mod original;

type A = Send<0, 1, Msg<1, ()>>;
type B = Send<0, 1, Msg<2, ()>>;
type C = Send<1, 0, Msg<3, ()>>;
type D = Send<0, 2, Msg<4, ()>>;
type F = Send<0, 3, Msg<5, ()>>;
type H = Send<254, 255, Msg<6, ()>>;
type R = Route<A, B>;
type Pair = Seq<A, C>;

fn panic_text(error: Box<dyn std::any::Any + std::marker::Send>) -> String {
    if let Some(text) = error.downcast_ref::<&str>() {
        String::from(*text)
    } else if let Some(text) = error.downcast_ref::<String>() {
        text.clone()
    } else {
        panic!("unexpected non-text panic payload")
    }
}

fn outcome<T>(f: impl FnOnce() -> T + std::panic::UnwindSafe) -> Result<T, String> {
    std::panic::catch_unwind(f).map_err(panic_text)
}

fn compare<const E: usize, const N: usize>(
    source: &EffList<E>,
    scopes: &projection::ScopeFacts,
    facts: RuntimeRoleFacts,
    role: u8,
    columns: RoleImageColumns,
) -> bool {
    let old = outcome(|| {
        let image = RoleImageBytes::<N>::emit_reference((source, scopes), facts, role, columns);
        (image.bytes.bytes, format!("{:?}", image.columns))
    });
    let new = outcome(|| {
        let image = RoleImageBytes::<N>::emit(
            (source, scopes),
            facts,
            projection::LocalRows::Role(role),
            columns,
        );
        (image.bytes.bytes, format!("{:?}", image.columns))
    });
    assert_eq!(old, new, "role {role}, columns {columns:?}");
    old.is_ok()
}

fn case<Shape: ProgramShape>(totals: &mut [usize; 4]) {
    let source = ProgramSourceData::<512>::lower::<Shape>();
    case_source(source.eff_list(), totals);
}

fn case_source(source: &EffList<512>, totals: &mut [usize; 4]) {
    let scopes = projection::ScopeFacts::new(source);
    let compiled = CompiledProgramImage::scan_const(source);
    use crate::global::compiled::images::{ProgramImageBytes, ProgramImagePlan};
    let global_columns = ProgramImagePlan::from_program(source).columns();
    let global_bytes = Box::leak(Box::new(ProgramImageBytes::<4096>::from_image(
        source,
        global_columns,
    )));
    let global = Box::leak(Box::new(
        global_bytes.compiled_ref(&compiled, global_columns),
    ));
    for role in 0..=compiled.max_role() {
        let counts =
            compiled.role_lowering_counts(source, &scopes, projection::LocalRows::Role(role));
        let facts = RuntimeRoleFacts::from_counts(counts);
        let plan = RoleImagePlan::from_program(
            (source, &scopes),
            facts,
            projection::LocalRows::Role(role),
        );
        let full_counts = RoleImagePlan::full_reference_counts(source, role);
        let full_facts = RuntimeRoleFacts::from_counts(full_counts);
        let full_plan = RoleImagePlan::from_program_reference((source, &scopes), full_facts, role);
        normalized_compare(
            (source, &scopes),
            global,
            full_facts,
            full_plan.columns,
            facts,
            plan.columns,
            role,
        );
        totals[3] += 1;
        let unchanged = full_counts.route_scope_count == counts.route_scope_count
            && full_counts.max_route_commit_count == counts.max_route_commit_count;
        if unchanged {
            assert_eq!(
                format!("{:?}", full_plan.columns),
                format!("{:?}", plan.columns)
            );
            assert!(compare::<512, 4096>(
                source,
                &scopes,
                facts,
                role,
                plan.columns
            ));
            totals[0] += 1;
        }
        for index in 0..source.len() {
            let old = outcome(|| {
                format!(
                    "{:?}",
                    original::local_event_row_for_eff(source, index, role)
                )
            });
            let new = outcome(|| {
                format!(
                    "{:?}",
                    projection::local_event_row_for_eff(
                        source,
                        index,
                        projection::LocalRows::Role(role),
                        0
                    )
                    .0
                )
            });
            assert_eq!(old, new);
            // Foreign logical roles have no direction to encode. The
            // differential above checks their rejection; only accepted rows
            // have a conflict fact to inspect below.
            if old.is_err() {
                continue;
            }
            let (_, reused) = projection::local_event_row_for_eff(
                source,
                index,
                projection::LocalRows::Role(role),
                0,
            );
            assert_eq!(
                reused,
                projection::route_conflict_for_eff(source.scope_markers(), index)
            );
            totals[2] += 1;
        }
        // Exact byte/panic differential has the same input contract only when
        // filtering leaves this role's complete plan/counts unchanged. Changed
        // layouts use the independent normalized differential above instead.
        if !unchanged || ![0, 1, 2, 3, 127, 128, 254, 255].contains(&role) {
            continue;
        }
        assert!(!compare::<512, 0>(
            source,
            &scopes,
            facts,
            role,
            plan.columns
        ));
        totals[1] += 1;
        for column in 0..16 {
            let mut columns = plan.columns;
            let selected = match column {
                0 => &mut columns.events,
                1 => &mut columns.lanes,
                2 => &mut columns.dependencies,
                3 => &mut columns.conflicts,
                4 => &mut columns.route_scopes,
                5 => &mut columns.route_scope_conflicts,
                6 => &mut columns.route_arms,
                7 => &mut columns.resident_boundaries,
                8 => &mut columns.lane_bits,
                9 => &mut columns.route_arm_lane_rows,
                10 => &mut columns.route_offer_lane_rows,
                11 => &mut columns.route_arm_lane_step_rows,
                12 => &mut columns.route_commit_ranges,
                13 => &mut columns.route_commit_rows,
                14 => &mut columns.roll_scopes,
                _ => &mut columns.events,
            };
            if column == 15 {
                selected.offset = u16::MAX;
            } else {
                selected.len += 1;
            }
            if compare::<512, 4096>(source, &scopes, facts, role, columns) {
                totals[0] += 1;
            } else {
                totals[1] += 1;
            }
        }
        let mut wrong = counts;
        wrong.local_step_count += 1;
        assert!(!compare::<512, 4096>(
            source,
            &scopes,
            RuntimeRoleFacts::from_counts(wrong),
            role,
            plan.columns
        ));
        totals[1] += 1;
        // Packed event-index overflow preserves the exact original panic text.
        let old = outcome(|| original::local_event_row_for_eff(source, usize::MAX, role));
        let new = outcome(|| {
            projection::local_event_row_for_eff(
                source,
                usize::MAX,
                projection::LocalRows::Role(role),
                0,
            )
            .0
        });
        assert_eq!(old.map(|r| format!("{r:?}")), new.map(|r| format!("{r:?}")));
        totals[1] += 1;
    }
}

fn overlapping_marker_failures_match() {
    use crate::eff::{EffAtom, EventOrigin};
    use crate::global::const_dsl::{ReentryMark, ScopeId};
    let mut source = EffList::<32>::new_partitioned(6, 8, 0);
    for label in 0..6 {
        source.push_event_mut(EffAtom {
            from: 0,
            to: 1,
            label,
            payload: crate::eff::EventPayload::Wire(0),
            origin: EventOrigin::User,
            lane: 0,
        });
    }
    source.push_route_scope_mut(ScopeId::route(0), 0, 2, 4, ReentryMark::SinglePass);
    source.push_route_scope_mut(ScopeId::route(1), 1, 3, 6, ReentryMark::Reentrant);
    let mut failures = 0;
    for index in 0..6 {
        for role in [0, 1, 2, 255] {
            let old = outcome(|| {
                format!(
                    "{:?}",
                    original::local_event_row_for_eff(&source, index, role)
                )
            });
            let new = outcome(|| {
                format!(
                    "{:?}",
                    projection::local_event_row_for_eff(
                        &source,
                        index,
                        projection::LocalRows::Role(role),
                        0
                    )
                    .0
                )
            });
            assert_eq!(old, new, "overlapping marker index {index}, role {role}");
            failures += usize::from(old.is_err());
        }
    }
    assert!(failures > 0);
    std::println!("projection-conflict-reuse overlapping_markers=24 exact_errors={failures}");
}

#[test]
fn emitted_projection_bytes_and_errors_match_original_corpus() {
    overlapping_marker_failures_match();
    let mut totals = [0usize; 4];
    case::<A>(&mut totals);
    case::<Pair>(&mut totals);
    case::<Par<A, D>>(&mut totals);
    case::<Roll<Pair>>(&mut totals);
    case::<R>(&mut totals);
    case::<Roll<R>>(&mut totals);
    case::<Route<Route<R, A>, B>>(&mut totals);
    case::<Route<A, Route<B, R>>>(&mut totals);
    case::<Seq<Par<R, D>, C>>(&mut totals);
    case::<Route<Seq<R, D>, Seq<B, D>>>(&mut totals);
    case::<Roll<Route<Roll<Par<A, B>>, Pair>>>(&mut totals);
    case::<Par<Roll<R>, Roll<Seq<D, F>>>>(&mut totals);
    case::<Seq<Roll<Route<Pair, Seq<B, C>>>, Par<D, F>>>(&mut totals);
    case::<Route<Seq<Par<R, D>, B>, Seq<A, D>>>(&mut totals);
    case::<Seq<Roll<Route<H, Send<254, 255, Msg<7, ()>>>>, A>>(&mut totals);
    std::println!(
        "projection-conflict-reuse cases=15 full_bytes={} exact_errors={} row_and_conflict={} normalized={}",
        totals[0],
        totals[1],
        totals[2],
        totals[3]
    );
}

fn normalized_compare<const E: usize>(
    (source, scopes): (&EffList<E>, &projection::ScopeFacts),
    global: &'static crate::global::compiled::images::CompiledProgramRef,
    full_facts: RuntimeRoleFacts,
    full_columns: RoleImageColumns,
    new_facts: RuntimeRoleFacts,
    new_columns: RoleImageColumns,
    role: u8,
) {
    let full =
        RoleImageBytes::<4096>::emit_reference((source, scopes), full_facts, role, full_columns);
    let new = RoleImageBytes::<4096>::emit(
        (source, scopes),
        new_facts,
        projection::LocalRows::Role(role),
        new_columns,
    );
    let full_bytes = Box::leak(Box::new(full.bytes.bytes));
    let new_bytes = Box::leak(Box::new(new.bytes.bytes));
    let old = RoleLaneImage::new(
        &full.columns,
        BlobPtr::from_array(full_bytes, full.columns.blob_len()),
    );
    let new = RoleLaneImage::new(
        &new.columns,
        BlobPtr::from_array(new_bytes, new.columns.blob_len()),
    );
    let old_fp = full_facts.footprint();
    let new_fp = new_facts.footprint();
    assert_eq!(old_fp.local_step_count, new_fp.local_step_count);
    assert_eq!(old_fp.active_lane_count, new_fp.active_lane_count);
    assert_eq!(
        old_fp.endpoint_lane_slot_count,
        new_fp.endpoint_lane_slot_count
    );
    assert_eq!(old_fp.logical_lane_count, new_fp.logical_lane_count);
    // These columns do not refer to compact route slots: exact bytes preserve
    // every local event/header, lane, dependency, conflict, resident boundary,
    // and roll identity/range even when their column offsets move.
    for (left, right, stride) in [
        (
            full_columns.events,
            new_columns.events,
            ROLE_IMAGE_EVENT_STRIDE,
        ),
        (
            full_columns.lanes,
            new_columns.lanes,
            ROLE_IMAGE_LANE_STRIDE,
        ),
        (
            full_columns.dependencies,
            new_columns.dependencies,
            ROLE_IMAGE_DEPENDENCY_STRIDE,
        ),
        (
            full_columns.conflicts,
            new_columns.conflicts,
            ROLE_IMAGE_CONFLICT_STRIDE,
        ),
        (
            full_columns.resident_boundaries,
            new_columns.resident_boundaries,
            ROLE_IMAGE_U16_STRIDE,
        ),
        (
            full_columns.roll_scopes,
            new_columns.roll_scopes,
            ROLE_IMAGE_ROLL_SCOPE_STRIDE,
        ),
    ] {
        assert_eq!(left.len, right.len, "role {role}");
        assert_eq!(
            &full_bytes[left.offset as usize..left.end_offset(stride)],
            &new_bytes[right.offset as usize..right.end_offset(stride)],
            "role {role}"
        );
    }
    let active_bytes = if old_fp.local_step_count == 0 {
        0
    } else {
        old_fp.logical_lane_count.div_ceil(8)
    };
    assert_eq!(
        &full_bytes[full_columns.lane_bits.offset as usize
            ..full_columns.lane_bits.offset as usize + active_bytes],
        &new_bytes[new_columns.lane_bits.offset as usize
            ..new_columns.lane_bits.offset as usize + active_bytes]
    );

    // Independent authority oracle over OLD/FULL emitted rows and the unchanged
    // global atlas. Do not call the production retained_route_scopes predicate.
    let mut retained = std::vec![false; old_fp.route_scope_count];
    for (slot, retained) in retained.iter_mut().enumerate() {
        let scope = old.route_scope_by_slot(slot).unwrap();
        let nonempty = (0..2).any(|arm| !old.route_arm_event_row_by_slot(slot, arm).is_zero_len());
        let controller = global.route_controller_role(scope) == role;
        let participant = global.route_has_participant(scope, 0, role)
            || global.route_has_participant(scope, 1, role);
        assert!(
            !(controller || participant) || nonempty,
            "global authority must have local arm events: role={role} scope={}",
            scope.raw()
        );
        let dependency = (0..old_fp.local_step_count).any(|index| {
            matches!(old.dependency_for_index(index).map(|d| d.conflict()),
                Some(LocalConflict::RouteArm { scope: candidate, .. }) if candidate.same(scope))
        });
        *retained = nonempty || controller || participant || dependency;
    }
    for iteration in 0..=old_fp.route_scope_count {
        let mut changed = false;
        for slot in 0..old_fp.route_scope_count {
            if !retained[slot] {
                continue;
            }
            if let Some(LocalConflict::RouteArm { scope, .. }) =
                old.route_scope_conflict_by_slot(slot).to_conflict()
            {
                let parent = old.route_scope_slot(scope).unwrap();
                if !retained[parent] {
                    retained[parent] = true;
                    changed = true;
                }
            }
            for arm in 0..2 {
                if let Some(child) = old.passive_arm_child_ordinal_by_slot(slot, arm) {
                    let child = old.route_scope_slot(ScopeId::route(child)).unwrap();
                    if !retained[child] {
                        retained[child] = true;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
        assert!(
            iteration < old_fp.route_scope_count,
            "reference closure did not converge"
        );
    }
    let expected: std::vec::Vec<_> = (0..old_fp.route_scope_count)
        .filter(|&slot| retained[slot])
        .map(|slot| old.route_scope_by_slot(slot).unwrap().raw())
        .collect();
    let actual: std::vec::Vec<_> = (0..new_fp.route_scope_count)
        .map(|slot| new.route_scope_by_slot(slot).unwrap().raw())
        .collect();
    assert_eq!(
        expected, actual,
        "exact independently retained scope IDs, role {role}"
    );
    let require_retained = |scope: ScopeId| {
        if matches!(scope.kind(), Some(ScopeKind::Route)) {
            assert!(
                retained[old.route_scope_slot(scope).unwrap()],
                "lost route reference role={role} scope={}",
                scope.raw()
            );
        }
    };
    for index in 0..old_fp.local_step_count {
        require_retained(old.local_step_event(index).unwrap().scope());
        if let Some(LocalConflict::RouteArm { scope, .. }) =
            old.event_conflict_for_index(index).to_conflict()
        {
            require_retained(scope);
        }
        if let Some(dependency) = old.dependency_for_index(index) {
            require_retained(dependency.scope());
            if let LocalConflict::RouteArm { scope, .. } = dependency.conflict() {
                require_retained(scope);
            }
        }
    }
    let mut expected_max_commit = 0;
    for new_slot in 0..new_fp.route_scope_count {
        let scope = new.route_scope_by_slot(new_slot).unwrap();
        let old_slot = old.route_scope_slot(scope).unwrap();
        assert_eq!(
            old.route_scope_conflict_by_slot(old_slot),
            new.route_scope_conflict_by_slot(new_slot)
        );
        assert_eq!(
            old.route_scope_reentry_by_slot(old_slot),
            new.route_scope_reentry_by_slot(new_slot)
        );
        for lane in 0..old_fp.logical_lane_count {
            assert_eq!(
                old.route_scope_offer_lane_set_by_slot(old_slot, old_fp.lane_word_count())
                    .contains(lane),
                new.route_scope_offer_lane_set_by_slot(new_slot, new_fp.lane_word_count())
                    .contains(lane)
            );
        }
        for arm in 0..2 {
            assert_eq!(
                old.route_arm_event_row_by_slot(old_slot, arm).raw(),
                new.route_arm_event_row_by_slot(new_slot, arm).raw()
            );
            // Child identities are global route ordinals, never compact slots.
            assert_eq!(
                old.passive_arm_child_ordinal_by_slot(old_slot, arm),
                new.passive_arm_child_ordinal_by_slot(new_slot, arm)
            );
            for lane in 0..old_fp.logical_lane_count {
                assert_eq!(
                    old.route_scope_arm_lane_set_by_slot(old_slot, arm, old_fp.lane_word_count())
                        .contains(lane),
                    new.route_scope_arm_lane_set_by_slot(new_slot, arm, new_fp.lane_word_count())
                        .contains(lane)
                );
                assert_eq!(
                    old.route_arm_lane_first_step_by_slot(
                        old_slot,
                        arm,
                        lane as u8,
                        old_fp.logical_lane_count
                    ),
                    new.route_arm_lane_first_step_by_slot(
                        new_slot,
                        arm,
                        lane as u8,
                        new_fp.logical_lane_count
                    )
                );
                assert_eq!(
                    old.route_arm_lane_last_step_by_slot(
                        old_slot,
                        arm,
                        lane as u8,
                        old_fp.logical_lane_count
                    ),
                    new.route_arm_lane_last_step_by_slot(
                        new_slot,
                        arm,
                        lane as u8,
                        new_fp.logical_lane_count
                    )
                );
            }
            let old_chain = old.route_commit_range_by_slot(old_slot, arm);
            let new_chain = new.route_commit_range_by_slot(new_slot, arm);
            expected_max_commit = expected_max_commit.max(old_chain.len());
            assert_eq!(old_chain.len(), new_chain.len());
            for offset in 0..old_chain.len() {
                let old_row = old.route_commit_row_at(old_chain.start() + offset);
                assert_eq!(old_row, new.route_commit_row_at(new_chain.start() + offset));
                if let Some(LocalConflict::RouteArm { scope, .. }) = old_row.to_conflict() {
                    require_retained(scope);
                }
            }
        }
    }
    assert_eq!(expected_max_commit, new_fp.max_route_commit_count);
    let old_rows = Box::leak(Box::new(RoleImageRef::new(
        global,
        role,
        full_facts,
        full_columns,
        full_bytes,
    )));
    let new_rows = Box::leak(Box::new(RoleImageRef::new(
        global,
        role,
        new_facts,
        new_columns,
        new_bytes,
    )));
    crate::global::typestate::EventCursor::assert_filtered_projection_callback_equivalence(
        old_rows, new_rows,
    );
}

#[test]
fn filtered_projection_matches_full_reference_with_sparse_unsorted_route_ids() {
    use crate::eff::{EffAtom, EventOrigin};
    use crate::global::const_dsl::ReentryMark;
    let mut source = EffList::<512>::new_partitioned(4, 8, 0);
    for (from, to, label) in [(0, 1, 1), (0, 1, 2), (2, 3, 3), (2, 3, 4)] {
        source.push_event_mut(EffAtom {
            from,
            to,
            label,
            payload: crate::eff::EventPayload::Wire(0),
            origin: EventOrigin::User,
            lane: 0,
        });
    }
    source.push_route_scope_mut(ScopeId::route(7), 0, 1, 2, ReentryMark::SinglePass);
    source.push_route_scope_mut(ScopeId::route(2), 2, 3, 4, ReentryMark::Reentrant);
    let mut totals = [0; 4];
    case_source(&source, &mut totals);
    assert_eq!(totals[3], 4);
}
