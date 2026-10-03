use super::*;
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
        let image = RoleImageBytes::<N>::emit((source, scopes), facts, role, columns);
        (image.bytes.bytes, format!("{:?}", image.columns))
    });
    assert_eq!(old, new, "role {role}, columns {columns:?}");
    old.is_ok()
}

fn case<Shape: ProgramShape>(totals: &mut [usize; 3]) {
    let source = ProgramSourceData::<512>::lower::<Shape>();
    let source = source.eff_list();
    let scopes = projection::ScopeFacts::new(source);
    let compiled = CompiledProgramImage::scan_const(source);
    for role in 0..=compiled.max_role() {
        let counts = compiled.role_lowering_counts(source, role);
        let facts = RuntimeRoleFacts::from_counts(counts);
        let plan = RoleImagePlan::from_program((source, &scopes), facts, role);
        assert!(compare::<512, 4096>(
            source,
            &scopes,
            facts,
            role,
            plan.columns
        ));
        totals[0] += 1;
        for index in 0..source.len() {
            let label = source.frame_label_at(index);
            let old = outcome(|| {
                format!(
                    "{:?}",
                    original::local_event_row_for_eff(source, index, label, role)
                )
            });
            let new = outcome(|| {
                format!(
                    "{:?}",
                    projection::local_event_row_for_eff(source, index, label, role).0
                )
            });
            assert_eq!(old, new);
            let (_, reused) = projection::local_event_row_for_eff(source, index, label, role);
            assert_eq!(
                reused,
                projection::route_conflict_for_eff(source.scope_markers(), index)
            );
            totals[2] += 1;
        }
        if ![0, 1, 2, 3, 127, 128, 254, 255].contains(&role) {
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
        let old = outcome(|| original::local_event_row_for_eff(source, usize::MAX, 255, role));
        let new = outcome(|| projection::local_event_row_for_eff(source, usize::MAX, 255, role).0);
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
            payload_schema: 0,
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
                    original::local_event_row_for_eff(&source, index, 255, role)
                )
            });
            let new = outcome(|| {
                format!(
                    "{:?}",
                    projection::local_event_row_for_eff(&source, index, 255, role).0
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
    let mut totals = [0usize; 3];
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
        "projection-conflict-reuse cases=15 full_bytes={} exact_errors={} row_and_conflict={}",
        totals[0],
        totals[1],
        totals[2]
    );
}
