use super::*;
use crate::{
    g::{self, Msg},
    global::{
        const_dsl::{ReentryMark, ScopeId},
        role_program::{RoleProgram, project},
        typestate::LocalConflict,
    },
};
use std::{boxed::Box, panic::catch_unwind};

fn copy(rows: &RoleImageRef) -> [u8; 2048] {
    let mut bytes = [0; 2048];
    assert!(rows.columns.blob_len() <= bytes.len());
    for (i, byte) in bytes.iter_mut().enumerate().take(rows.columns.blob_len()) {
        *byte = rows.blob.byte_at(i);
    }
    bytes
}
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn put(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn reject(
    rows: &RoleImageRef,
    edit: impl FnOnce(&mut [u8; 2048], &mut RoleImageColumns, &mut RuntimeRoleFacts),
) {
    let mut bytes = copy(rows);
    let mut columns = rows.columns;
    let mut facts = rows.facts;
    edit(&mut bytes, &mut columns, &mut facts);
    // Constructor accepts immutable static storage. These tiny malformed
    // fixtures deliberately remain allocated, including after expected panic.
    let bytes = Box::leak(Box::new(bytes));
    assert!(
        catch_unwind(|| RoleImageRef::new(rows.program, rows.role, facts, columns, bytes)).is_err()
    );
}

/// The mandatory constructor pass must prove every guard omitted by direct
/// arm-bound reads, even without sorted/passive-parent certificates.
fn assert_lane_validation_dominates_raw_arm_reads(rows: &RoleImageRef) {
    use super::super::metadata::lane_columns_are_coherent;
    use crate::global::role_program::{BlobPtr, ROLE_IMAGE_ROUTE_ARM_STRIDE, RoleLaneImage};

    let mut accepted = 0;
    let mut rejected = 0;
    let columns = rows.columns;
    for offset in columns.route_arms.offset as usize
        ..columns.route_arms.end_offset(ROLE_IMAGE_ROUTE_ARM_STRIDE)
    {
        for value in [0, 1, 7, 0x7f, 0x80, 0xff] {
            let mut bytes = copy(rows);
            bytes[offset] = value;
            if !lane_columns_are_coherent(&bytes, columns, rows.active_lane_row, rows.footprint()) {
                rejected += 1;
                continue;
            }
            accepted += 1;
            let bytes = Box::leak(Box::new(bytes));
            let raw = RoleLaneImage::new(
                &rows.columns,
                BlobPtr::from_array(bytes, columns.blob_len()),
            );
            for slot in 0..columns.route_scopes.len as usize {
                for arm in 0..2 {
                    // A malformed prefix or bound must panic in the raw query.
                    // Acceptance by the mandatory pass must rule that out.
                    assert_eq!(
                        raw.route_arm_event_row_by_slot(slot, arm).raw(),
                        raw.certified_route_arm_event_row_by_slot(slot, arm).raw(),
                        "arm byte {offset} = {value}, slot {slot}, arm {arm}"
                    );
                }
            }
        }
    }
    assert!(accepted > 0 && rejected > 0);
}

#[test]
fn mandatory_lane_validation_proves_raw_arm_bounds() {
    let program: RoleProgram<1> = project(&g::route(
        g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::send::<0, 1, Msg<3, ()>>(),
    ));
    let rows = program.role_image_ref();
    assert!(rows.has_sorted_route_index());
    assert!(rows.has_passive_parent_index());
    assert_lane_validation_dominates_raw_arm_reads(rows);
}

#[test]
fn validated_metadata_constructor_rejects_every_skipped_metadata_class() {
    let program: RoleProgram<1> = project(&g::seq(
        g::route(
            g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
            g::send::<0, 1, Msg<3, ()>>(),
        ),
        g::send::<0, 1, Msg<4, ()>>(),
    ));
    let rows = program.role_image_ref();
    let event = rows.columns.events.offset as usize;
    reject(rows, |b, _, _| b[event + 9] = 0x80); // event flags
    reject(rows, |b, _, _| b[event + 9] ^= 0b110); // receive changed into send
    reject(rows, |b, _, _| b[event + 9] &= 1); // wire receive changed into local action
    reject(rows, |b, _, _| put(b, event + 6, 0x8000)); // event scope
    reject(rows, |b, _, _| put(b, event, 100)); // missing program atom
    reject(rows, |b, _, _| put(b, event + 2, 100)); // missing dependency
    reject(rows, |b, _, _| put(b, event + 4, 100)); // missing conflict
    reject(rows, |b, c, _| {
        put(b, c.conflicts.offset as usize, u16::MAX)
    }); // referenced empty conflict
    reject(rows, |b, _, _| put(b, event + 4, u16::MAX)); // absent conflict inside route envelope
    reject(rows, |b, c, _| {
        let at = c.conflicts.offset as usize;
        let raw = u16_at(b, at);
        put(b, at, raw ^ 1);
    }); // wrong first arm
    reject(rows, |b, c, _| {
        put(
            b,
            c.conflicts.offset as usize,
            PackedEventConflict::route_arm(ScopeId::route(123), 0).raw(),
        );
    }); // valid route ID absent from this image's authority table
    reject(rows, |b, c, _| {
        let at = c.route_scopes.offset as usize;
        let raw = u16_at(b, at);
        put(b, at + 2, raw);
    }); // duplicate authority
    reject(rows, |b, c, _| {
        put(b, c.route_scopes.offset as usize, 0x2000)
    }); // wrong scope kind
    reject(rows, |b, c, _| {
        put(b, c.route_scope_conflicts.offset as usize, 0x8000)
    }); // invalid reentry/conflict
    reject(rows, |_, c, _| c.route_scope_conflicts.len -= 1);
    reject(rows, |_, c, _| c.route_arms.len -= 1);
    reject(rows, |_, _, f| {
        f.words[RuntimeRoleFacts::LOCAL_STEP_COUNT] += 1
    });
    reject(rows, |_, _, f| {
        f.words[RuntimeRoleFacts::ROUTE_SCOPE_COUNT] += 1
    });
    let joined: RoleProgram<1> = project(&g::seq(
        g::par(
            g::send::<0, 1, Msg<11, ()>>(),
            g::send::<0, 1, Msg<12, ()>>(),
        ),
        g::send::<0, 1, Msg<13, ()>>(),
    ));
    let rows = joined.role_image_ref();
    assert!(rows.columns.dependencies.len > 0);
    reject(rows, |b, c, _| {
        // out-of-bounds referenced dependency extent
        put(b, c.dependencies.offset as usize + 2, 100);
    });
    reject(rows, |b, c, _| {
        // an explicitly referenced absent dependency row
        let start = c.dependencies.offset as usize;
        b[start..start + 8].fill(255);
    });
    let parallel: RoleProgram<1> = project(&g::par(
        g::send::<0, 1, Msg<5, ()>>(),
        g::send::<0, 1, Msg<6, ()>>(),
    ));
    let rows = parallel.role_image_ref();
    reject(rows, |b, c, _| {
        // directory/bitmap lanes stay valid, atom lane differs
        let at = c.events.offset as usize;
        let raw = u16_at(b, at + 10);
        put(b, at, raw);
    });
    let foreign: RoleProgram<2> = project(&g::seq(
        g::send::<0, 1, Msg<7, ()>>(),
        g::send::<0, 2, Msg<8, ()>>(),
    ));
    reject(foreign.role_image_ref(), |b, c, _| {
        put(b, c.events.offset as usize, 0)
    });
}

#[test]
fn validated_metadata_constructor_accepts_unique_unsorted_route_ids() {
    let program: RoleProgram<1> = project(&g::route(
        g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::send::<0, 1, Msg<3, ()>>(),
    ));
    let rows = program.role_image_ref();
    let columns = rows.columns;
    let mut bytes = copy(rows);
    let rename = |scope: ScopeId| {
        if scope.same(ScopeId::route(0)) {
            ScopeId::route(7)
        } else if scope.same(ScopeId::route(1)) {
            ScopeId::route(3)
        } else {
            scope
        }
    };
    for slot in 0..columns.route_scopes.len as usize {
        let at = columns.route_scopes.offset as usize + slot * 2;
        let scope = ScopeId::decode_raw(u16_at(&bytes, at)).unwrap();
        put(&mut bytes, at, rename(scope).raw());
    }
    for idx in 0..columns.events.len as usize {
        let at = columns.events.offset as usize + idx * 10 + 6;
        let scope = ScopeId::decode_raw(u16_at(&bytes, at)).unwrap();
        put(&mut bytes, at, rename(scope).raw());
    }
    for column in [
        columns.conflicts,
        columns.route_scope_conflicts,
        columns.route_commit_rows,
    ] {
        for idx in 0..column.len as usize {
            let at = column.offset as usize + idx * 2;
            let conflict = PackedEventConflict::from_raw(u16_at(&bytes, at));
            if let Some(LocalConflict::RouteArm { scope, arm }) = conflict.to_conflict() {
                let mark = if conflict.route_reentry() {
                    ReentryMark::Reentrant
                } else {
                    ReentryMark::SinglePass
                };
                put(
                    &mut bytes,
                    at,
                    PackedEventConflict::route_arm(rename(scope), arm)
                        .with_route_reentry(mark)
                        .raw(),
                );
            }
        }
    }
    let bytes = Box::leak(Box::new(bytes));
    let rebuilt = RoleImageRef::new(rows.program, rows.role, rows.facts, columns, bytes);
    assert!(!rebuilt.has_sorted_route_index());
    assert!(!rebuilt.has_passive_parent_index());
    assert_lane_validation_dominates_raw_arm_reads(&rebuilt);
    assert_eq!(rebuilt.route_scope_slot(ScopeId::route(7)), Some(0));
    assert_eq!(rebuilt.route_scope_slot(ScopeId::route(3)), Some(1));
    for idx in 0..rows.local_step_count() {
        assert_eq!(
            rebuilt.local_step_node(idx).unwrap().action(),
            rows.local_step_node(idx).unwrap().action()
        );
    }
    crate::global::typestate::EventCursor::compare_validated_metadata_image(Box::leak(Box::new(
        rebuilt,
    )));
}

#[test]
fn validated_metadata_preserves_wire_label_and_session_origin() {
    use crate::eff::EventOrigin;
    let projected: RoleProgram<1> = project(&g::send::<0, 1, Msg<1, ()>>());
    let original = projected.role_image_ref();
    let mut role_bytes = copy(original);
    role_bytes[original.columns.events.offset as usize + 8] = 199;
    let role_bytes = Box::leak(Box::new(role_bytes));
    let user = RoleImageRef::new(
        original.program,
        original.role,
        original.facts,
        original.columns,
        role_bytes,
    );
    assert!(matches!(
        user.local_step_node(0).unwrap().action(),
        LocalAction::Recv {
            frame_label: 199,
            label: 1,
            origin: EventOrigin::User,
            ..
        }
    ));
    let mut program_bytes = [0u8; 256];
    let program = original.program;
    assert!(program.columns.blob_len() <= program_bytes.len());
    for (idx, byte) in program_bytes
        .iter_mut()
        .enumerate()
        .take(program.columns.blob_len())
    {
        *byte = program.blob.byte_at(idx);
    }
    // The canonical first atom starts at zero, origin is packed byte seven.
    program_bytes[7] = EventOrigin::Session.packed_bits();
    let program_bytes = Box::leak(Box::new(program_bytes));
    let program = Box::leak(Box::new(CompiledProgramRef::compact(
        program.facts,
        program.columns,
        program_bytes,
    )));
    let session = RoleImageRef::new(
        program,
        original.role,
        original.facts,
        original.columns,
        role_bytes,
    );
    assert!(matches!(
        session.local_step_node(0).unwrap().action(),
        LocalAction::Recv {
            origin: EventOrigin::Session,
            frame_label: 199,
            ..
        }
    ));
    crate::global::typestate::EventCursor::compare_validated_metadata_image(Box::leak(Box::new(
        user,
    )));
    crate::global::typestate::EventCursor::compare_validated_metadata_image(Box::leak(Box::new(
        session,
    )));
}
