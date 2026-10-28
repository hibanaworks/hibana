use super::*;
use crate::{
    g::{self, Msg},
    global::{
        event_program::LocalEventProgram,
        role_program::{PackedLaneRange, RoleImageRef, RoleLocalDirection},
    },
    runtime::program::project,
};
use std::{boxed::Box, panic::catch_unwind};

fn fixture() -> &'static RoleImageRef {
    let global = g::seq(
        g::route(
            g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
            g::route(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<4, ()>>()),
        )
        .roll(),
        g::route(g::send::<0, 1, Msg<5, ()>>(), g::send::<0, 1, Msg<6, ()>>()),
    );
    let program: crate::runtime::program::RoleProgram<1> = project(&global);
    program.role_image_ref()
}

fn machine(rows: &'static RoleImageRef) -> EventCursorMachine {
    EventCursorMachine {
        role: rows.role(),
        event_program: LocalEventProgram::from_rows(rows),
    }
}

fn rebuild(
    original: &'static RoleImageRef,
    edit: impl FnOnce(&mut [u8; 2048]),
) -> &'static RoleImageRef {
    let mut bytes = [0u8; 2048];
    assert!(original.columns().blob_len() <= bytes.len());
    for (offset, byte) in bytes
        .iter_mut()
        .enumerate()
        .take(original.columns().blob_len())
    {
        *byte = original.blob().byte_at(offset);
    }
    edit(&mut bytes);
    let immutable = Box::leak(Box::new(bytes));
    Box::leak(Box::new(RoleImageRef::new(
        original.program(),
        original.role(),
        original.facts(),
        original.columns(),
        immutable,
    )))
}

fn write_u16(bytes: &mut [u8; 2048], offset: usize, raw: u16) {
    bytes[offset..offset + 2].copy_from_slice(&raw.to_le_bytes());
}

fn raw_u16(rows: &RoleImageRef, offset: usize) -> u16 {
    u16::from_le_bytes([rows.blob().byte_at(offset), rows.blob().byte_at(offset + 1)])
}

fn raw_scope(rows: &RoleImageRef, slot: usize) -> ScopeId {
    ScopeId::decode_raw(raw_u16(
        rows,
        rows.columns().route_scopes.offset as usize + slot * 2,
    ))
    .unwrap()
}

fn raw_slot(rows: &RoleImageRef, query: ScopeId) -> Option<usize> {
    (0..rows.columns().route_scopes.len as usize).find(|&slot| raw_scope(rows, slot) == query)
}

fn raw_parent(rows: &RoleImageRef, child: usize) -> Option<(usize, u8)> {
    for slot in 0..rows.columns().route_scopes.len as usize {
        for arm in 0..2 {
            let offset = rows.columns().route_arms.offset as usize + (slot * 2 + arm) * 8;
            if usize::from(raw_u16(rows, offset + 4)) == child {
                return Some((slot, arm as u8));
            }
        }
    }
    None
}

// Decode the arm's bytes and scan its event rows. Neither the compiled
// first-operation table nor the production reverse-parent traversal is used.
fn raw_first_recv(rows: &RoleImageRef, slot: usize, arm: u8) -> Option<usize> {
    let at = rows.columns().route_arms.offset as usize + (slot * 2 + arm as usize) * 8;
    let range = PackedLaneRange::from_raw(
        u32::from(raw_u16(rows, at)) | (u32::from(raw_u16(rows, at + 2)) << 16),
    );
    if range.is_absent_or_zero_len() {
        return None;
    }
    (range.start()..range.end()).find(|&step| {
        rows.lanes().local_step_event(step).unwrap().direction() == RoleLocalDirection::Recv
    })
}

fn raw_visits(rows: &RoleImageRef, root: usize) -> std::vec::Vec<(u8, usize)> {
    let mut visits = std::vec::Vec::new();
    for slot in 0..rows.columns().route_scopes.len as usize {
        let root_arm = if slot == root {
            None
        } else {
            let mut current = slot;
            let mut found = None;
            for _ in 0..rows.columns().route_scopes.len {
                let Some((parent, arm)) = raw_parent(rows, current) else {
                    break;
                };
                if parent == root {
                    found = Some(arm);
                    break;
                }
                current = parent;
            }
            let Some(arm) = found else {
                continue;
            };
            Some(arm)
        };
        for arm in 0..2 {
            if let Some(step) = raw_first_recv(rows, slot, arm) {
                visits.push((root_arm.unwrap_or(arm), step));
            }
        }
    }
    visits.sort_unstable();
    visits
}

fn assert_same_lookup(rows: &'static RoleImageRef, query: ScopeId) {
    let machine = machine(rows);
    let slot = raw_slot(rows, query);
    let expected_parent = slot
        .and_then(|slot| raw_parent(rows, slot))
        .map(|(slot, arm)| (raw_scope(rows, slot), arm));
    assert_eq!(machine.passive_child_parent_route(query), expected_parent);
    let mut actual = std::vec::Vec::new();
    let status = machine.visit_first_recv_dispatch(query, |arm, target| {
        actual.push((arm, state_index_to_usize(target)));
    });
    actual.sort_unstable();
    let expected = slot.map(|slot| raw_visits(rows, slot));
    assert_eq!(status.is_some(), expected.is_some());
    assert_eq!(actual.as_slice(), expected.as_deref().unwrap_or(&[]));
    let subtree_mask = expected
        .as_ref()
        .map(|visits| visits.iter().fold(0u8, |mask, &(arm, _)| mask | (1 << arm)));
    let range_mask = slot.map(|slot| {
        u8::from(raw_first_recv(rows, slot, 0).is_some())
            | (u8::from(raw_first_recv(rows, slot, 1).is_some()) << 1)
    });
    assert_eq!(range_mask, subtree_mask);
    assert_eq!(machine.first_recv_dispatch_arm_mask(query), range_mask);
}

fn assert_same_key_lookup(rows: &'static RoleImageRef, query: ScopeId) {
    let machine = machine(rows);
    let expected = raw_slot(rows, query).map(|slot| raw_visits(rows, slot));
    let mut keys = std::collections::BTreeSet::new();
    let events = rows.program().columns.atom_count();
    for effect in 0..events {
        let atom = rows.program().event_atom_at(effect);
        keys.extend([
            (atom.from, atom.lane, effect as u16),
            (atom.from.wrapping_add(1), atom.lane, effect as u16),
            (atom.from, atom.lane.wrapping_add(1), effect as u16),
            (atom.from, atom.lane, events as u16),
            (atom.from, atom.lane, u16::MAX),
        ]);
    }
    for (source, lane, label) in keys {
        let expected = expected
            .as_ref()
            .map_or(Err(CursorInvariantError::INVARIANT), |visits| {
                let mut matched = UniqueMatch::NONE;
                for &(arm, target) in visits {
                    let event = rows.lanes().local_step_event(target).unwrap();
                    let atom = rows.program().event_atom_at(usize::from(event.eff_index));
                    if event.direction() == RoleLocalDirection::Recv
                        && atom.from == source
                        && atom.lane == lane
                        && event.eff_index == label
                    {
                        matched = matched.add((arm, StateIndex::from_usize(target)));
                    }
                }
                matched
                    .finish_optional()
                    .map_err(|_| CursorInvariantError::INVARIANT)
            });
        assert_eq!(
            machine.first_recv_descendant_target_for_key(
                query,
                InboundFrameKey::new(source, lane, label),
            ),
            expected,
            "scope={} source={source} lane={lane} label={label}",
            query.raw(),
        );
    }
}

#[test]
fn sealed_dispatch_matches_raw_ranges_and_subtree_for_every_decodable_scope() {
    let rows = fixture();
    for raw in 0..=u16::MAX {
        if let Some(query) = ScopeId::decode_raw(raw) {
            assert_same_lookup(rows, query);
        }
    }
}

#[test]
fn invalid_unused_owner_is_rejected_before_dispatch_publication() {
    let original = fixture();
    let last = original.columns().route_scopes.len as usize - 1;
    assert!(
        catch_unwind(|| rebuild(original, |bytes| {
            write_u16(
                bytes,
                original.columns().route_scope_conflicts.offset as usize + last * 2,
                0x8000,
            );
        }))
        .is_err()
    );
}

#[test]
fn backward_edges_and_duplicate_targets_are_rejected_at_construction() {
    let original = fixture();
    for child in [0u16, 1] {
        assert!(
            catch_unwind(|| rebuild(original, |bytes| {
                // Parent arm0 remains valid. Arm1 is backward or duplicates arm0.
                write_u16(
                    bytes,
                    original.columns().route_arms.offset as usize + 8 + 4,
                    child,
                );
            }))
            .is_err()
        );
    }
}

#[test]
fn a_parent_arm_that_does_not_contain_the_child_is_rejected() {
    let original = fixture();
    assert!(
        catch_unwind(|| rebuild(original, |bytes| {
            let offset = original.columns().route_scope_conflicts.offset as usize + 2;
            let raw = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
            write_u16(bytes, offset, raw ^ 1);
        }))
        .is_err()
    );
}

#[test]
fn a_parent_cycle_with_an_outside_child_is_rejected() {
    let original = fixture();
    let child = original.route_scope_by_slot(1).unwrap();
    assert!(
        catch_unwind(|| rebuild(original, |bytes| {
            write_u16(
                bytes,
                original.columns().route_arms.offset as usize + 2 * 8 + 4,
                0,
            );
            write_u16(
                bytes,
                original.columns().route_scope_conflicts.offset as usize,
                child.raw() << 1,
            );
        }))
        .is_err()
    );
}

#[test]
fn unused_parent_rows_must_still_contain_their_children() {
    let original = fixture();
    let last = original.columns().route_scopes.len as usize - 1;
    let root = original.route_scope_by_slot(0).unwrap();
    let orphan = original.route_scope_by_slot(last).unwrap();
    assert!(
        catch_unwind(|| rebuild(original, |bytes| {
            let start = original.columns().route_scope_conflicts.offset as usize;
            write_u16(bytes, start, orphan.raw() << 1);
            write_u16(bytes, start + last * 2, root.raw() << 1);
        }))
        .is_err()
    );
}

#[test]
fn compiled_receive_dispatch_matches_raw_nested_parallel_rolled_and_delegated_images() {
    let parallel = g::route(
        g::par(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::par(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<4, ()>>()),
    );
    type Loan<'r> = Msg<2, crate::Endpoint<'r, 3>>;
    let delegated = g::seq(
        g::route(g::send::<0, 1, Msg<0, ()>>(), g::send::<0, 3, Msg<1, ()>>()),
        g::send::<1, 2, Loan<'_>>(),
    );
    for rows in [
        fixture(),
        project::<0>(&parallel).role_image_ref(),
        project::<1>(&parallel).role_image_ref(),
        project::<0>(&delegated).role_image_ref(),
        project::<1>(&delegated).role_image_ref(),
        project::<3>(&delegated).role_image_ref(),
        project::<2>(&delegated).role_image_ref(),
    ] {
        for slot in 0..rows.columns().route_scopes.len as usize {
            assert_same_lookup(rows, raw_scope(rows, slot));
            assert_same_key_lookup(rows, raw_scope(rows, slot));
        }
        for query in [
            ScopeId::none(),
            ScopeId::route(8191),
            ScopeId::roll_scope(0),
        ] {
            assert_same_lookup(rows, query);
            assert_same_key_lookup(rows, query);
        }
    }
}
