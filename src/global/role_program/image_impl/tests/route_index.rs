use super::super::metadata::route_scopes_are_sorted;
use super::*;

#[test]
fn sorted_route_certificate_checks_complete_raw_rows_and_bounds() {
    let dense = [0u8, 0, 1, 0, 2, 0];
    assert!(route_scopes_are_sorted(&dense, ColumnRange::new(0, 3, 2)));
    assert!(route_scopes_are_sorted(
        &[1u8, 0, 3, 0, 7, 0],
        ColumnRange::new(0, 3, 2)
    ));
    assert!(route_scopes_are_sorted(&[], ColumnRange::new(0, 0, 2)));
    assert!(!route_scopes_are_sorted(&dense, ColumnRange::new(0, 4, 2)));
    assert!(!route_scopes_are_sorted(&dense, ColumnRange::new(1, 3, 2)));
    for bad in [
        [0, 0, 0, 0, 2, 0],     // duplicate
        [0, 0, 2, 0, 1, 0],     // permutation
        [0, 0, 1, 0x20, 2, 0],  // same local ordinal, wrong kind
        [0, 0, 1, 0x80, 2, 0],  // reserved bit
        [0, 0, 255, 255, 2, 0], // absent sentinel
    ] {
        assert!(!route_scopes_are_sorted(&bad, ColumnRange::new(0, 3, 2)));
    }
    let mut limit = [0u8; (ScopeId::LOCAL_CAPACITY as usize + 1) * 2];
    for slot in 0..=ScopeId::LOCAL_CAPACITY as usize {
        limit[slot * 2..slot * 2 + 2].copy_from_slice(&(slot as u16).to_le_bytes());
    }
    assert!(route_scopes_are_sorted(
        &limit,
        ColumnRange::new(0, ScopeId::LOCAL_CAPACITY as usize, 2),
    ));
    assert!(!route_scopes_are_sorted(
        &limit,
        ColumnRange::new(0, ScopeId::LOCAL_CAPACITY as usize + 1, 2),
    ));
}

#[test]
fn projected_route_index_matches_scan_for_every_decodable_scope_id() {
    let global = g::seq(
        g::route(
            g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
            g::send::<0, 1, Msg<3, ()>>(),
        )
        .roll(),
        g::route(g::send::<0, 1, Msg<4, ()>>(), g::send::<0, 1, Msg<5, ()>>()),
    );
    let program: RoleProgram<0> = project(&global);
    let rows = program.role_image_ref();
    assert!(rows.route_scopes_sorted);
    assert_eq!(rows.columns.route_scopes.len, 3);
    for raw in 0..=u16::MAX {
        if let Some(scope) = ScopeId::decode_raw(raw) {
            assert_eq!(
                rows.route_scope_slot(scope),
                rows.lanes().route_scope_slot(scope)
            );
        }
    }
}

#[test]
fn constructor_keeps_malformed_route_tables_on_checked_fallback() {
    let global = g::route(
        g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::send::<0, 1, Msg<3, ()>>(),
    );
    let program: RoleProgram<0> = project(&global);
    let original = program.role_image_ref();
    assert!(original.route_scopes_sorted);
    assert!(original.columns.blob_len() <= 256);
    let mut bytes = [0u8; 256];
    for (offset, byte) in bytes
        .iter_mut()
        .enumerate()
        .take(original.columns.blob_len())
    {
        *byte = original.blob.byte_at(offset);
    }
    for second in [0u16, 8192, u16::MAX] {
        let mut corrupted = bytes;
        let offset = original.columns.route_scopes.offset as usize + 2;
        corrupted[offset..offset + 2].copy_from_slice(&second.to_le_bytes());
        let immutable = std::boxed::Box::leak(std::boxed::Box::new(corrupted));
        let rows = RoleImageRef::new(
            original.program,
            original.role,
            original.facts,
            original.columns,
            immutable,
        );
        assert!(!rows.route_scopes_sorted);
        assert_invariant(|| {
            let _ = rows.route_scope_slot(ScopeId::route(0));
        });
    }
}

#[test]
fn constructed_route_index_matches_scan_on_arbitrary_small_raw_tables() {
    let global = g::route(
        g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::send::<0, 1, Msg<3, ()>>(),
    );
    let program: RoleProgram<0> = project(&global);
    let original = program.role_image_ref();
    let mut bytes = [0u8; 256];
    assert!(original.columns.blob_len() <= bytes.len());
    for (offset, byte) in bytes
        .iter_mut()
        .enumerate()
        .take(original.columns.blob_len())
    {
        *byte = original.blob.byte_at(offset);
    }
    let values = [0u16, 1, 7, 8191, 8192, 32768, u16::MAX];
    let queries = [
        ScopeId::none(),
        ScopeId::route(0),
        ScopeId::route(1),
        ScopeId::route(7),
        ScopeId::route(8191),
        ScopeId::roll_scope(0),
        ScopeId::new(crate::global::const_dsl::ScopeKind::Parallel, 0),
    ];
    for left in values {
        for right in values {
            let mut modified = bytes;
            let start = original.columns.route_scopes.offset as usize;
            modified[start..start + 2].copy_from_slice(&left.to_le_bytes());
            modified[start + 2..start + 4].copy_from_slice(&right.to_le_bytes());
            let immutable = std::boxed::Box::leak(std::boxed::Box::new(modified));
            let rows = RoleImageRef::new(
                original.program,
                original.role,
                original.facts,
                original.columns,
                immutable,
            );
            for query in queries {
                let new = std::panic::catch_unwind(|| rows.route_scope_slot(query));
                let old = std::panic::catch_unwind(|| rows.lanes().route_scope_slot(query));
                assert_eq!(new.is_err(), old.is_err());
                assert_eq!(new.ok(), old.ok());
            }
        }
    }
}
