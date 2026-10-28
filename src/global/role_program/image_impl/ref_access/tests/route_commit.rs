use super::*;

fn compare_all_chains(rows: &RoleImageRef) {
    for slot in 0..rows.columns.route_scopes.len as usize {
        for arm in 0..2 {
            let chain = rows.route_commit_range_by_slot(slot, arm);
            assert_eq!(
                chain.raw(),
                rows.lanes().route_commit_range_by_slot(slot, arm).raw(),
                "slot {slot}, arm {arm}"
            );
            let borrowed: std::vec::Vec<_> = rows
                .route_commit_bytes(chain)
                .as_chunks::<2>()
                .0
                .iter()
                .map(|bytes| PackedEventConflict::from_raw(u16::from_le_bytes(*bytes)))
                .collect();
            let checked: std::vec::Vec<_> = (chain.start()..chain.end())
                .map(|index| rows.lanes().route_commit_row_at(index))
                .collect();
            assert_eq!(borrowed, checked);
        }
    }
}

#[test]
fn constructed_route_commit_chains_match_checked_reads_in_parallel_rolled_and_owned_images() {
    let work = g::par(
        g::route(
            g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
            g::send::<0, 1, Msg<3, ()>>(),
        )
        .roll(),
        g::route(g::send::<0, 1, Msg<4, ()>>(), g::send::<0, 1, Msg<5, ()>>()),
    );
    compare_all_chains(project::<0>(&work).role_image_ref());
    compare_all_chains(project::<1>(&work).role_image_ref());

    type Loan<'a> = Msg<6, crate::Endpoint<'a, 2>>;
    let owned = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::route(
                g::seq(g::send::<2, 3, Msg<7, ()>>(), g::send::<3, 2, Msg<8, ()>>()),
                g::seq(
                    g::send::<2, 3, Msg<9, ()>>(),
                    g::send::<3, 2, Msg<10, ()>>(),
                ),
            ),
            g::send::<1, 0, Loan<'_>>(),
        ),
    )
    .roll();
    compare_all_chains(project::<0>(&owned).role_image_ref());
    compare_all_chains(project::<1>(&owned).role_image_ref());
    compare_all_chains(project::<2>(&owned).role_image_ref());
    compare_all_chains(project::<3>(&owned).role_image_ref());
}

#[test]
fn route_commit_constructor_validation_dominates_every_raw_chain_check() {
    let projected = project::<1>(&g::seq(
        g::route(
            g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
            g::send::<0, 1, Msg<3, ()>>(),
        )
        .roll(),
        g::route(g::send::<0, 1, Msg<4, ()>>(), g::send::<0, 1, Msg<5, ()>>()),
    ));
    let rows = projected.role_image_ref();
    let mut accepted = 0;
    let mut raw_rejected = 0;
    for (column, stride) in [
        (rows.columns.route_commit_ranges, 4),
        (rows.columns.route_commit_rows, 2),
        (rows.columns.route_scope_conflicts, 2),
    ] {
        for offset in column.offset as usize..column.end_offset(stride) {
            for value in [0, 1, 7, 0x7f, 0x80, 0xff] {
                let mut bytes = copy(rows);
                bytes[offset] = value;
                // SAFETY: neither the raw nor the constructed image escapes
                // this iteration; the stationary backing array outlives both.
                let bytes = unsafe { &*core::ptr::from_ref(&bytes) };
                let raw_valid = catch_unwind(|| {
                    let raw = RoleLaneImage::new(
                        &rows.columns,
                        BlobPtr::from_array(bytes, rows.columns.blob_len()),
                    );
                    for slot in 0..rows.columns.route_scopes.len as usize {
                        for arm in 0..2 {
                            let _ = raw.route_commit_range_by_slot(slot, arm);
                        }
                    }
                })
                .is_ok();
                raw_rejected += usize::from(!raw_valid);
                let checked = catch_unwind(|| {
                    RoleImageRef::new(rows.program, rows.role, rows.facts, rows.columns, bytes)
                });
                if let Ok(checked) = checked {
                    assert!(raw_valid, "accepted invalid chain: byte {offset} = {value}");
                    compare_all_chains(&checked);
                    accepted += 1;
                }
            }
        }
    }
    assert!(accepted > 0 && raw_rejected > 0);
}

#[test]
fn an_empty_post_join_peer_context_is_valid_but_cannot_be_its_own_parent() {
    type Loan = Msg<9, crate::Endpoint<'static, 10>>;
    type Notice = Msg<8, ()>;
    let source = g::seq(
        g::route(
            g::seq(g::send::<0, 1, Loan>(), g::send::<0, 2, Notice>()),
            g::seq(g::send::<0, 1, Notice>(), g::send::<0, 2, Loan>()),
        )
        .resolve::<701>(),
        g::seq(
            g::send::<10, 3, Msg<1, ()>>(),
            g::send::<3, 10, Msg<2, ()>>(),
        ),
    );
    // Actor 3 has no branch operation. Its peer after the join still depends
    // on this route, so projection must retain the empty source context.
    let rows = project::<3>(&source).role_image_ref();
    assert_eq!(rows.footprint().route_scope_count, 1);
    assert_eq!(rows.local_step_count(), 2);
    let scope = rows.route_scope_by_slot(0).unwrap();
    assert!(rows.route_scope_conflict_by_slot(0).is_none());
    for arm in 0..2 {
        assert!(rows.route_arm_event_row_by_slot(0, arm).is_zero_len());
        let chain = rows.route_commit_range_by_slot(0, arm);
        assert_eq!(chain.len(), 1);
        assert_eq!(
            rows.route_commit_row_at(chain.start()).to_conflict(),
            Some(LocalConflict::RouteArm { scope, arm }),
        );
    }
    for arm in 0..2 {
        reject(rows, |bytes, columns, _| {
            put(
                bytes,
                columns.route_scope_conflicts.offset as usize,
                PackedEventConflict::route_arm(scope, arm).raw(),
            );
        });
    }
}
