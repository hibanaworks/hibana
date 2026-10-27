use super::*;

fn scanned_completion(
    cursor: &EventCursor,
    idx: usize,
    selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
) -> Option<usize> {
    let mut result = None;
    for slot in 0..cursor.machine().route_scope_slot_count() {
        if let Some(region) = cursor.machine().route_scope_rows_by_slot(slot)
            && region.start() <= idx
            && idx < region.end()
        {
            let scope = region.scope();
            if let Some(arm) = selected(scope)
                && cursor.selected_route_arm_completes_scope(scope, arm, selected)
            {
                result = Some(match result {
                    Some(current) => cursor.deeper_route_scope(current, scope),
                    None => scope,
                });
            }
        }
    }
    result.and_then(|scope| cursor.route_scope_end_by_id(scope))
}

fn scanned_controller(cursor: &EventCursor, idx: usize) -> Option<ScopeId> {
    let mut result = None;
    for slot in 0..cursor.machine().route_scope_slot_count() {
        if let Some(region) = cursor.machine().route_scope_rows_by_slot(slot)
            && region.start() <= idx
            && idx < region.end()
            && cursor.contains_route_controller(region.scope())
        {
            result = Some(match result {
                Some(current) => cursor.deeper_route_scope(current, region.scope()),
                None => region.scope(),
            });
        }
    }
    result
}

fn compare_enclosing_queries(rows: &'static RoleImageRef) {
    let len = rows.local_step_count();
    let count = rows.footprint().route_scope_count;
    assert!(len <= 8 && count <= 4);
    let scopes: std::vec::Vec<_> = (0..count)
        .map(|slot| rows.route_scope_by_slot(slot).unwrap())
        .collect();
    with_rows(rows, |cursor| {
        for idx in (0..=len).chain([len + 1, usize::MAX]) {
            let mut actual = std::vec::Vec::new();
            cursor.visit_route_arms_for_index(idx, |scope, arm| actual.push((scope, arm)));
            let mut expected = std::vec::Vec::new();
            for slot in 0..count {
                if let Some(region) = cursor.machine().route_scope_rows_by_slot(slot)
                    && region.start() <= idx
                    && idx < region.end()
                    && let Some(arm) = cursor.route_arm_for_index(region.scope(), idx)
                {
                    expected.push((region.scope(), arm));
                }
            }
            assert_eq!(actual, expected, "ancestry order at {idx}");
            assert_eq!(
                cursor.send_preview_controller_scope_at_for_decision(idx),
                scanned_controller(cursor, idx),
                "controller at {idx}"
            );
        }
    });
    for completed in 0..1usize << len {
        with_rows(rows, |cursor| {
            for step in 0..len {
                if completed & (1 << step) != 0 {
                    cursor.mark_local_event_done(step);
                }
            }
            for choices in 0..3usize.pow(count as u32) {
                let mut code = choices;
                let mut selected = std::vec::Vec::new();
                for &scope in &scopes {
                    selected.push((
                        scope,
                        match code % 3 {
                            0 => None,
                            1 => Some(0),
                            2 => Some(1),
                            _ => unreachable!(),
                        },
                    ));
                    code /= 3;
                }
                let mut lookup = |scope| {
                    selected
                        .iter()
                        .find(|(candidate, _)| *candidate == scope)
                        .and_then(|(_, arm)| *arm)
                };
                for idx in (0..=len).chain([len + 1, usize::MAX]) {
                    assert_eq!(
                        cursor.selected_enclosing_route_scope_end_at(idx, &mut lookup),
                        scanned_completion(cursor, idx, &mut lookup),
                        "completed={completed:b}, choices={choices}, index={idx}"
                    );
                }
            }
        });
    }
}

#[test]
fn enclosing_route_chains_match_full_scans_for_all_completion_and_choice_patterns() {
    let global = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, Msg<1, ()>>(),
                g::route(g::send::<0, 1, Msg<2, ()>>(), g::send::<0, 1, Msg<3, ()>>())
                    .resolve::<0>()
                    .roll(),
            ),
            g::par(g::send::<0, 1, Msg<4, ()>>(), g::send::<0, 1, Msg<5, ()>>()),
        ),
        g::route(g::send::<0, 1, Msg<6, ()>>(), g::send::<0, 1, Msg<7, ()>>()),
    );
    compare_enclosing_queries(project::<0>(&global).role_image_ref());
    compare_enclosing_queries(project::<1>(&global).role_image_ref());
}

#[test]
fn enclosing_route_chains_match_full_scans_in_delegated_parallel_views() {
    type Loan<'r> = Msg<9, Endpoint<'r, 3>>;
    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::par(
                g::route(
                    g::route(g::send::<3, 2, Msg<1, ()>>(), g::send::<3, 2, Msg<2, ()>>()),
                    g::send::<3, 2, Msg<3, ()>>(),
                )
                .roll(),
                g::route(g::send::<1, 2, Msg<4, ()>>(), g::send::<1, 2, Msg<5, ()>>()),
            ),
            g::send::<1, 0, Loan<'_>>(),
        ),
    );
    compare_enclosing_queries(project::<0>(&global).role_image_ref());
    compare_enclosing_queries(project::<1>(&global).role_image_ref());
    compare_enclosing_queries(project::<2>(&global).role_image_ref());
}
