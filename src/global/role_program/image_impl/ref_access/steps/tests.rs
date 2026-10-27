use super::*;
use crate::{
    Endpoint, g,
    global::{
        event_program::{LocalEventProgram, LocalEventRow},
        typestate::EventCommitMeta,
    },
    runtime::program::project,
};
use std::{boxed::Box, panic::catch_unwind};

fn compare(rows: &'static RoleImageRef) {
    let events = LocalEventProgram::from_rows(rows);
    let raw = rows.lanes();
    for index in 0..rows.local_step_count() {
        let packed = raw.local_step_event(index).unwrap();
        let atom = rows.program().event_atom_at(packed.eff_index as usize);
        let conflict = raw.event_conflict_for_index(index);
        let row = events.event_row_at(index).unwrap();
        assert_eq!(row.effect().dense_ordinal(), packed.eff_index as usize);
        assert_eq!(row.direction(), packed.direction());
        assert_eq!(row.scope(), packed.scope());
        assert_eq!(row.choice(), packed.choice_mark());
        assert_eq!(row.next().as_usize(), index + 1);
        assert_eq!(row.conflict(), conflict);
        assert_eq!(row.dependency(), raw.dependency_for_index(index));
        assert_eq!(rows.local_step_lane(index), Some(atom.lane));
        assert_eq!(
            row.logical_role(),
            if packed.direction() == RoleLocalDirection::Recv {
                atom.to
            } else {
                atom.from
            }
        );
        let actual = row.atom();
        assert_eq!(actual.from, atom.from);
        assert_eq!(actual.to, atom.to);
        assert_eq!(actual.label, atom.label);
        assert_eq!(actual.payload, atom.payload);
        assert_eq!(actual.origin, atom.origin);
        assert_eq!(actual.lane, atom.lane);
        let route_arm = conflict.to_conflict().and_then(|conflict| match conflict {
            crate::global::typestate::LocalConflict::RouteArm { arm, .. } => Some(arm),
            crate::global::typestate::LocalConflict::Unconditional
            | crate::global::typestate::LocalConflict::SharedRoute => None,
        });
        let expected = EventCommitMeta::new(
            crate::eff::EffIndex::from_dense_ordinal(packed.eff_index as usize),
            atom.label,
            atom.origin,
            packed.scope(),
            route_arm,
            atom.lane,
        );
        assert_eq!(row.commit_meta(), expected);
        assert!(row.matches_commit(expected));
        assert_eq!(
            events.operation_at(index),
            Some((row.effect(), row.direction()))
        );

        let mut wrong = expected;
        wrong.label = wrong.label.wrapping_add(1);
        assert!(!row.matches_commit(wrong));
        wrong = expected;
        wrong.lane = wrong.lane.wrapping_add(1);
        assert!(!row.matches_commit(wrong));
        wrong = expected;
        wrong.route_arm = match wrong.route_arm {
            None => Some(0),
            Some(arm) => Some(arm ^ 1),
        };
        assert!(!row.matches_commit(wrong));
    }
    let terminal = rows.local_step_count();
    for effect in (0..=rows.program().columns.atom_count()).chain([u16::MAX as usize, usize::MAX]) {
        let expected: std::vec::Vec<_> = (0..terminal)
            .filter(|&index| raw.local_step_event(index).unwrap().eff_index as usize == effect)
            .collect();
        assert_eq!(
            events
                .source_effect_candidates(effect)
                .collect::<std::vec::Vec<_>>(),
            expected
        );
    }
    assert!(events.event_row_at(terminal).is_none());
    assert_eq!(events.operation_at(terminal), None);
    assert_eq!(rows.local_step_lane(terminal), None);
}

#[test]
fn sealed_step_fields_match_raw_rows_for_routes_parallelism_rolls_and_empty_images() {
    type A = g::Msg<7, u32>;
    type B = g::Msg<7, bool>;
    type C = g::Msg<255, ()>;
    let global = g::seq(
        g::par(g::send::<2, 1, A>(), g::send::<3, 1, B>()),
        g::seq(
            g::send::<1, 0, C>(),
            g::route(g::send::<0, 1, A>(), g::send::<0, 1, C>()),
        ),
    )
    .roll();
    compare(project::<0>(&global).role_image_ref());
    compare(project::<1>(&global).role_image_ref());
    let distant = g::send::<0, 255, C>();
    compare(project::<254>(&distant).role_image_ref());
    let local = g::send::<0, 0, g::Msg<0, ()>>();
    compare(project::<0>(&local).role_image_ref());
}

#[test]
fn sealed_step_fields_preserve_owned_payloads_and_distinct_co_resident_facets() {
    type Loan<'r> = g::Msg<1, Endpoint<'r, 2>>;
    type Read = g::Msg<2, u32>;
    type Value = g::Msg<3, u32>;
    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::send::<2, 1, Read>(),
            g::seq(g::send::<1, 2, Value>(), g::send::<1, 0, Loan<'_>>()),
        ),
    );
    compare(project::<0>(&global).role_image_ref());
    compare(project::<1>(&global).role_image_ref());
}

fn rejects_mutation(rows: &RoleImageRef, mutate: impl FnOnce(&mut [u8])) {
    let mut bytes = [0; 2048];
    let len = rows.columns().blob_len();
    assert!(len <= bytes.len());
    for (offset, byte) in bytes.iter_mut().enumerate().take(len) {
        *byte = rows.blob().byte_at(offset);
    }
    mutate(&mut bytes[..len]);
    let bytes = Box::leak(Box::new(bytes));
    assert!(
        catch_unwind(|| {
            RoleImageRef::new(
                rows.program(),
                rows.role(),
                rows.facts(),
                rows.columns(),
                bytes,
            )
        })
        .is_err()
    );
}

#[test]
fn raw_constructor_rejects_event_and_dependency_corruption_before_sealed_reads() {
    let global = g::seq(
        g::par(
            g::send::<0, 1, g::Msg<1, ()>>(),
            g::send::<0, 2, g::Msg<2, ()>>(),
        ),
        g::send::<0, 3, g::Msg<3, ()>>(),
    );
    let rows = project::<0>(&global).role_image_ref();
    rejects_mutation(rows, |bytes| {
        bytes[rows.columns().events.offset as usize + 8] |= 0x80;
    });
    rejects_mutation(rows, |bytes| {
        let offset = rows.columns().events.offset as usize + 2;
        bytes[offset..offset + 2].copy_from_slice(&(u16::MAX - 1).to_le_bytes());
    });
    assert!(rows.columns().dependencies.len != 0);
    rejects_mutation(rows, |bytes| {
        bytes[rows.columns().dependencies.offset as usize + 7] |= 0x80;
    });
}

#[test]
fn a_local_event_row_is_only_an_image_reference_and_compact_ordinal() {
    assert!(core::mem::size_of::<LocalEventRow>() <= 2 * core::mem::size_of::<usize>());
}
