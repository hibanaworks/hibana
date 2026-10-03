use super::*;
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::{ReentryMark, ScopeId},
};
#[path = "legacy_participant_validation.rs"]
mod legacy;

fn atom(from: u8, to: u8, label: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        label,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane: 0,
    }
}

fn assert_equivalent<const E: usize>(source: &EffList<E>) -> Option<u8> {
    let summary = CompiledProgramImage::scan_const(source);
    let actual = projection_error_all_roles(&summary, source).map(|e| e as u8);
    let expected = legacy::legacy_projection_error_all_roles(&summary, source).map(|e| e as u8);
    assert_eq!(actual, expected, "full ordered pipeline");
    assert_eq!(
        validate_route_projection_guarantees(&summary, source).map(|e| e as u8),
        legacy::legacy_route_projection_guarantees(&summary, source).map(|e| e as u8),
        "ordered route stages, independently of earlier validation errors"
    );
    let mask = participating_roles(source);
    for role in 0..=255usize {
        let marked = mask[role / 8] & (1u8 << (role % 8)) != 0;
        let occurs = (0..source.len()).any(|i| {
            let event = source.atom_at(i);
            event.from == role as u8 || event.to == role as u8
        });
        assert_eq!(marked, occurs, "role {role}");
    }
    actual
}

fn one_route(left: &[(u8, u8, u8)], right: &[(u8, u8, u8)], resolver: bool) -> EffList<40> {
    let mut source = EffList::new_partitioned(left.len() + right.len(), 4, usize::from(resolver));
    for &(from, to, label) in left.iter().chain(right) {
        source.push_event_mut(atom(from, to, label));
    }
    source.push_route_scope_mut(
        ScopeId::route(0),
        0,
        left.len(),
        left.len() + right.len(),
        ReentryMark::SinglePass,
    );
    if resolver {
        source.push_route_resolver_mut(ScopeId::route(0), 0);
    }
    source
}

#[test]
fn participant_mask_covers_receiver_only_255_and_sparse_holes() {
    let source = one_route(&[(0, 255, 1)], &[(0, 255, 2)], false);
    assert_eq!(assert_equivalent(&source), None);
    assert_eq!(
        CompiledProgramImage::scan_const(&source).compiled_program_role_count(),
        256
    );
    let mask = participating_roles(&source);
    assert_eq!(mask[0], 1);
    assert_eq!(mask[31], 128);
    assert!(mask[1..31].iter().all(|byte| *byte == 0));
    let invalid = one_route(&[(0, 255, 1)], &[(0, 1, 2)], false);
    assert_eq!(
        assert_equivalent(&invalid),
        Some(ProgramSourceError::ProjectionRouteUnprojectable as u8)
    );
}

#[test]
fn participant_mask_preserves_self_sends_duplicates_and_controller_errors() {
    for resolver in [false, true] {
        for role in [0, 1, 7, 8, 15, 16, 24, 25, 63, 64, 127, 128, 254, 255] {
            let source = one_route(
                &[(role, role, 3), (role, role, 3)],
                &[(role, role, 3), (role, role, 3)],
                resolver,
            );
            assert_equivalent(&source);
            let mismatch = one_route(
                &[(role, role, 1)],
                &[(role.wrapping_add(1), role, 2)],
                resolver,
            );
            assert_equivalent(&mismatch);
        }
    }
}

#[test]
fn participant_mask_dense_all_256_roles_and_byte_boundaries() {
    let mut source = EffList::<1530>::new_partitioned(510, 1020, 0);
    for role in 1..=255u8 {
        source.push_event_mut(atom(0, role, 1));
        source.push_event_mut(atom(0, role, 2));
    }
    for index in 0..255 {
        source.push_route_scope_mut(
            ScopeId::route(index as u16),
            index * 2,
            index * 2 + 1,
            index * 2 + 2,
            ReentryMark::SinglePass,
        );
    }
    assert_eq!(participating_roles(&source), [255u8; 32]);
    assert_eq!(assert_equivalent(&source), None);
}

#[test]
fn participant_mask_differential_generated_valid_and_malformed_graphs() {
    let roles = [0u8, 1, 7, 8, 24, 25, 63, 64, 127, 128, 254, 255];
    let mut seed = 0x4350_4c45u32;
    let mut accepted = 0;
    let mut rejected = 0;
    for sample in 0..2048usize {
        let mut endpoints = [(0u8, 0u8, 0u8); 8];
        for (index, event) in endpoints.iter_mut().enumerate() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let from = roles[((seed >> 16) as usize) % roles.len()];
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let to = roles[((seed >> 16) as usize) % roles.len()];
            *event = (from, to, if sample % 7 == 0 { 0 } else { index as u8 });
        }
        // Include guaranteed accepting sparse routes, then missing receivers,
        // receiver-led followups, controller mismatches, and selector duplicates.
        if sample % 4 == 0 {
            let from = roles[sample % roles.len()];
            let to = roles[(sample + 1) % roles.len()];
            for event in &mut endpoints {
                event.0 = from;
                event.1 = to;
            }
        }
        let source = one_route(&endpoints[..4], &endpoints[4..], sample % 2 == 0);
        if assert_equivalent(&source).is_none() {
            accepted += 1;
        } else {
            rejected += 1;
        }
    }
    assert!(
        accepted > 0 && rejected > 0,
        "nonvacuity: accepted={accepted} rejected={rejected}"
    );
}

#[test]
fn participant_mask_preserves_nested_route_and_earlier_stage_errors() {
    use crate::g::{Msg, Par, ProgramSourceData, Roll, Route, Send, Seq};
    type Missing =
        Route<Seq<Send<0, 255, Msg<1, ()>>, Send<255, 24, Msg<2, ()>>>, Send<0, 255, Msg<3, ()>>>;
    type Controller = Route<Send<0, 255, Msg<1, ()>>, Send<24, 255, Msg<2, ()>>>;
    type Nested = Route<
        Route<Send<24, 255, Msg<1, ()>>, Send<24, 255, Msg<2, ()>>>,
        Send<24, 255, Msg<3, ()>>,
    >;
    type Parallel = Par<Send<24, 255, Msg<1, ()>>, Send<24, 255, Msg<1, ()>>>;
    type Reentry = Roll<Route<Send<24, 255, Msg<1, ()>>, Send<24, 255, Msg<2, ()>>>>;
    assert_equivalent(ProgramSourceData::<40>::lower::<Missing>().eff_list());
    assert_equivalent(ProgramSourceData::<40>::lower::<Controller>().eff_list());
    assert_equivalent(ProgramSourceData::<40>::lower::<Nested>().eff_list());
    assert_equivalent(ProgramSourceData::<40>::lower::<Parallel>().eff_list());
    assert_equivalent(ProgramSourceData::<40>::lower::<Reentry>().eff_list());
}
