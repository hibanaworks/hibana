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

mod operation_index;
mod route_commit;
mod route_topology;

#[test]
fn ordinary_roll_rows_must_match_the_complete_source_projection() {
    let global = g::seq(
        g::send::<0, 1, Msg<1, ()>>().roll(),
        g::send::<0, 1, Msg<2, ()>>().roll().roll(),
    );
    let rows = project::<0>(&global).role_image_ref();
    assert!(!rows.program().has_actor_bindings());
    reject(rows, |bytes, columns, _| {
        put(
            bytes,
            columns.roll_scopes.offset as usize,
            ScopeId::LOCAL_CAPACITY - 1,
        );
    });
    reject(rows, |_, columns, _| columns.roll_scopes.len -= 1);
    reject(rows, |bytes, columns, _| {
        let start = columns.roll_scopes.offset as usize;
        let stride = crate::global::role_program::ROLE_IMAGE_ROLL_SCOPE_STRIDE;
        for byte in 0..stride {
            bytes.swap(start + byte, start + stride + byte);
        }
    });
}

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
    let bytes = Box::new(bytes);
    assert!(
        catch_unwind(|| {
            // SAFETY: the constructor requires static storage, but this test
            // discards its result inside this closure. The Box stays alive
            // throughout validation and no reference escapes, even on panic.
            let temporary = unsafe { &*core::ptr::from_ref(bytes.as_ref()) };
            let _ = RoleImageRef::new(rows.program, rows.role, facts, columns, temporary);
        })
        .is_err()
    );
}

/// The mandatory constructor pass must prove every guard omitted by direct
/// arm-bound reads, independently of scope identifier ordering.
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
            if !lane_columns_are_coherent(&bytes, columns, rows.active_lane_row(), rows.footprint())
            {
                rejected += 1;
                continue;
            }
            accepted += 1;
            // SAFETY: the raw image is read only in this iteration. Its
            // stationary backing array remains live and no reference escapes.
            let bytes = unsafe { &*core::ptr::from_ref(&bytes) };
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
    reject(rows, |b, c, _| b[c.route_index().start] ^= 1);
    reject(rows, |b, c, _| b[c.route_index().start + 1] ^= 1);
    reject(rows, |_, c, _| c.route_index_len = 0);
    reject(rows, |b, _, _| b[event + 8] = 0x80); // event flags
    reject(rows, |b, _, _| b[event + 8] ^= 0b110); // receive changed into send
    reject(rows, |b, _, _| b[event + 8] &= 1); // wire receive changed into local action
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
        let raw = u16_at(b, at + crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE);
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
fn validated_metadata_rejects_omitted_route_ancestors_and_event_contexts() {
    let program: RoleProgram<1> = project(&g::route(
        g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::send::<0, 1, Msg<3, ()>>(),
    ));
    let rows = program.role_image_ref();
    assert_eq!(rows.columns.route_scopes.len, 2);
    reject(rows, |b, c, _| {
        // The inner route still lies inside the outer first arm, but its
        // parent link has been removed.
        put(b, c.route_scope_conflicts.offset as usize + 2, u16::MAX);
    });
    reject(rows, |b, c, _| {
        // The outer arm contains this event, but the event must also carry
        // the inner selection rather than silently losing that requirement.
        let first = c.events.offset as usize;
        let conflict = u16_at(b, first + 4) as usize;
        put(
            b,
            c.conflicts.offset as usize + conflict * 2,
            PackedEventConflict::route_arm(ScopeId::route(0), 0).raw(),
        );
    });
}

#[test]
fn validated_metadata_constructor_rejects_noncanonical_route_index() {
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
        let at = columns.events.offset as usize
            + idx * crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE
            + 6;
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
    // SAFETY: this test retains the stationary array through every query and
    // discards the rebuilt image before returning.
    let bytes = unsafe { &*core::ptr::from_ref(&bytes) };
    assert!(
        catch_unwind(|| { RoleImageRef::new(rows.program, rows.role, rows.facts, columns, bytes) })
            .is_err()
    );
}

#[test]
fn validated_metadata_derives_wire_identity_and_preserves_session_origin() {
    use crate::eff::EventOrigin;
    let projected: RoleProgram<1> = project(&g::send::<0, 1, Msg<1, ()>>());
    let original = projected.role_image_ref();
    let role_bytes = copy(original);
    // SAFETY: both images below borrow this stationary array only for queries
    // within this test. Neither image nor its references escape.
    let role_bytes = unsafe { &*core::ptr::from_ref(&role_bytes) };
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
            frame_label: 0,
            label: 1,
            origin: EventOrigin::User,
            ..
        }
    ));
    let mut program_bytes = [0u8; 256];
    let original_program = original.program;
    assert!(original_program.columns.blob_len() <= program_bytes.len());
    for (idx, byte) in program_bytes
        .iter_mut()
        .enumerate()
        .take(original_program.columns.blob_len())
    {
        *byte = original_program.blob.byte_at(idx);
    }
    // The canonical first atom starts at zero, origin is packed byte seven.
    program_bytes[7] = EventOrigin::Session.packed_bits();
    // SAFETY: the program and its backing array remain stationary until all
    // session-image queries finish. No reference leaves this test.
    let program_bytes = unsafe { &*core::ptr::from_ref(&program_bytes) };
    let session_program = CompiledProgramRef::compact(
        original_program.facts,
        original_program.columns,
        program_bytes,
    );
    // SAFETY: the borrowed program outlives the session image and every query.
    let session_program = unsafe { &*core::ptr::from_ref(&session_program) };
    let session = RoleImageRef::new(
        session_program,
        original.role,
        original.facts,
        original.columns,
        role_bytes,
    );
    assert!(matches!(
        session.local_step_node(0).unwrap().action(),
        LocalAction::Recv {
            origin: EventOrigin::Session,
            frame_label: 0,
            ..
        }
    ));
    crate::global::typestate::EventCursor::compare_validated_metadata_image(&user);
    crate::global::typestate::EventCursor::compare_validated_metadata_image(&session);
}
