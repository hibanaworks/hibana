use super::super::metadata::route_scopes_are_sorted;
use super::*;

#[test]
fn sparse_route_index_matches_linear_membership_across_every_scope_id() {
    use super::super::metadata::{route_index_is_coherent, route_slot};
    const INPUTS: [&[usize]; 7] = [
        &[],
        &[0],
        &[31],
        &[32],
        &[0, 31, 32, 63, 64, 8191],
        &[7, 65, 999],
        &[999, 7, 65],
    ];
    const STORAGE: usize = ScopeId::LOCAL_CAPACITY as usize * 4;
    const fn fixture(routes: &[usize]) -> (RoleImageColumns, [u8; STORAGE]) {
        let mut columns = empty_columns();
        columns.route_scopes = ColumnRange::new(0, routes.len(), 2);
        columns.roll_scopes = ColumnRange::new(routes.len() * 2, 0, 6);
        let mut bytes = [255u8; STORAGE];
        let mut slot = 0;
        let start = columns.operation_index().end_offset(2);
        while slot < routes.len() {
            let raw = routes[slot];
            if raw + 1 > columns.route_index_len as usize {
                columns.route_index_len = (raw + 1) as u16;
            }
            bytes[slot * 2] = raw as u8;
            bytes[slot * 2 + 1] = (raw >> 8) as u8;
            bytes[start + raw * 2] = slot as u8;
            bytes[start + raw * 2 + 1] = (slot >> 8) as u8;
            slot += 1;
        }
        (columns, bytes)
    }
    static CASES: [(RoleImageColumns, [u8; STORAGE]); 7] = [
        fixture(INPUTS[0]),
        fixture(INPUTS[1]),
        fixture(INPUTS[2]),
        fixture(INPUTS[3]),
        fixture(INPUTS[4]),
        fixture(INPUTS[5]),
        fixture(INPUTS[6]),
    ];
    for (routes, (columns, bytes)) in INPUTS.into_iter().zip(CASES.iter()) {
        let columns = *columns;
        let index = columns.route_index();
        assert!(route_index_is_coherent(bytes, columns));
        for offset in index.clone() {
            let mut corrupt = *bytes;
            corrupt[offset] ^= 1;
            assert!(
                !route_index_is_coherent(&corrupt, columns),
                "index byte {offset}"
            );
        }
        let blob = BlobPtr::from_array(bytes, columns.blob_len());
        for raw in 0..=u16::MAX {
            if let Some(scope) = ScopeId::decode_raw(raw) {
                let expected = routes
                    .iter()
                    .position(|&candidate| candidate == raw as usize);
                assert_eq!(
                    route_slot(blob, index.clone(), scope),
                    expected,
                    "scope {raw}"
                );
            }
        }
    }
}

#[test]
fn route_index_preserves_the_complete_compact_protocol_byte_span() {
    use super::super::metadata::{route_index_is_coherent, route_slot};
    const CAPACITY: usize = super::super::super::ROLE_IMAGE_STORAGE_BYTE_CAPACITY;
    let mut columns = empty_columns();
    columns.route_scopes = ColumnRange::new(0, 1, 2);
    columns.roll_scopes = ColumnRange::new(u16::MAX as usize, 0, 6);
    columns.route_index_len = ScopeId::LOCAL_CAPACITY;
    columns.validate_blob_bound();
    assert_eq!(columns.operation_index().end_offset(2), u16::MAX as usize);
    assert_eq!(
        columns.route_index().end,
        u16::MAX as usize + ScopeId::LOCAL_CAPACITY as usize * 2
    );
    assert!(columns.blob_len() <= CAPACITY);
    let mut overlapping = columns;
    overlapping.dependencies = ColumnRange {
        offset: u16::MAX - 1,
        len: 1,
    };
    assert!(overlapping.dependencies.end_offset(8) < columns.blob_len());
    assert!(std::panic::catch_unwind(|| overlapping.validate_blob_bound()).is_err());
    static BYTES: [u8; CAPACITY] = {
        let mut bytes = [255; CAPACITY];
        let raw = ScopeId::MAX_LOCAL_ORDINAL;
        bytes[0] = raw as u8;
        bytes[1] = (raw >> 8) as u8;
        let at = u16::MAX as usize + ScopeId::LOCAL_CAPACITY as usize * 2 - 2;
        bytes[at] = 0;
        bytes[at + 1] = 0;
        bytes
    };
    assert!(route_index_is_coherent(&BYTES, columns));
    let blob = BlobPtr::from_array(&BYTES, CAPACITY);
    assert_eq!(
        route_slot(blob, columns.route_index(), ScopeId::route(8191)),
        Some(0)
    );
    assert_eq!(
        route_slot(blob, columns.route_index(), ScopeId::route(0)),
        None
    );
    assert_eq!(
        route_slot(blob, columns.route_index(), ScopeId::roll_scope(0)),
        None
    );
}

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
    assert_eq!(rows.columns().route_scopes.len, 3);
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
fn constructor_rejects_malformed_route_tables_before_publication() {
    let global = g::route(
        g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::send::<0, 1, Msg<3, ()>>(),
    );
    let program: RoleProgram<0> = project(&global);
    let original = program.role_image_ref();
    assert!(original.columns().blob_len() <= 256);
    let mut bytes = [0u8; 256];
    for (offset, byte) in bytes
        .iter_mut()
        .enumerate()
        .take(original.columns().blob_len())
    {
        *byte = original.blob().byte_at(offset);
    }
    for second in [0u16, 8192, u16::MAX] {
        let mut corrupted = bytes;
        let offset = original.columns().route_scopes.offset as usize + 2;
        corrupted[offset..offset + 2].copy_from_slice(&second.to_le_bytes());
        let immutable = std::boxed::Box::leak(std::boxed::Box::new(corrupted));
        let raw = RoleLaneImage::new(
            original.lanes().columns,
            BlobPtr::from_array(immutable, original.columns().blob_len()),
        );
        assert_invariant(|| {
            let _ = raw.route_scope_slot(ScopeId::route(0));
        });
        assert_invariant(|| {
            let _ = RoleImageRef::new(
                original.program(),
                original.role(),
                original.facts(),
                original.columns(),
                immutable,
            );
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
    assert!(original.columns().blob_len() <= bytes.len());
    for (offset, byte) in bytes
        .iter_mut()
        .enumerate()
        .take(original.columns().blob_len())
    {
        *byte = original.blob().byte_at(offset);
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
            let start = original.columns().route_scopes.offset as usize;
            modified[start..start + 2].copy_from_slice(&left.to_le_bytes());
            modified[start + 2..start + 4].copy_from_slice(&right.to_le_bytes());
            let immutable = std::boxed::Box::leak(std::boxed::Box::new(modified));
            let raw = RoleLaneImage::new(
                original.lanes().columns,
                BlobPtr::from_array(immutable, original.columns().blob_len()),
            );
            let constructed = std::panic::catch_unwind(|| {
                RoleImageRef::new(
                    original.program(),
                    original.role(),
                    original.facts(),
                    original.columns(),
                    immutable,
                )
            });
            for query in queries {
                let checked = std::panic::catch_unwind(|| raw.route_scope_slot(query));
                let valid = left < ScopeId::LOCAL_CAPACITY && right < ScopeId::LOCAL_CAPACITY;
                let duplicate_query = left == right && query.raw() == left;
                assert_eq!(checked.is_err(), !valid || duplicate_query);
                if let Ok(rows) = &constructed {
                    assert_eq!(Some(rows.route_scope_slot(query)), checked.ok());
                }
            }
            if !matches!((left, right), (0, 1)) {
                // Merely changing route IDs leaves immutable event/conflict
                // references incoherent, even when the raw table is unique.
                assert!(constructed.is_err());
            } else {
                assert!(constructed.is_ok());
            }
        }
    }
}
