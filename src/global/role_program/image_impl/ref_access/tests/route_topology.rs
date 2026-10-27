use super::*;
use crate::{Endpoint, global::role_program::RoleLocalDirection};
use std::collections::BTreeSet;

fn compare(rows: &RoleImageRef) {
    let raw = rows.lanes();
    let count = rows.local_step_count();
    let view_at = |step| {
        let event = raw.local_step_event(step).unwrap();
        let atom = rows.program().event_atom_at(event.eff_index as usize);
        if event.direction() == RoleLocalDirection::Recv {
            atom.to
        } else {
            atom.from
        }
    };
    let mut views = BTreeSet::from([0, 254, 255]);
    let mut lanes = BTreeSet::from([0, 254, 255]);
    for step in 0..count {
        let lane = rows.local_step_lane(step).unwrap();
        let expected = (step + 1..count)
            .find(|&next| rows.local_step_lane(next) == Some(lane))
            .unwrap_or(count);
        assert_eq!(rows.next_lane_step(step), expected);
    }
    if rows.columns.route_scopes.len != 0 {
        for step in 0..count {
            views.insert(view_at(step));
            lanes.insert(
                raw.local_step_lane(step, rows.footprint().logical_lane_count)
                    .unwrap(),
            );
            let event = raw.local_step_event(step).unwrap();
            let expected = (0..step).rev().find(|&before| {
                view_at(before) == view_at(step)
                    && rows.program().source_precedes(
                        raw.local_step_event(before).unwrap().eff_index as usize,
                        event.eff_index as usize,
                    )
            });
            let actual = u16_at(
                &copy(rows),
                rows.columns.view_predecessors().start + step * 2,
            );
            assert_eq!(actual, expected.map_or(u16::MAX, |x| x as u16));
        }
    }
    for slot in 0..rows.columns.route_scopes.len as usize {
        for arm in 0..2 {
            let range = raw.route_arm_event_row_by_slot(slot, arm);
            let steps = if range.is_absent_or_zero_len() {
                0..0
            } else {
                range.start()..range.end()
            };
            for receive in [false, true] {
                let expected = steps.clone().find(|&step| {
                    (raw.local_step_event(step).unwrap().direction() == RoleLocalDirection::Recv)
                        == receive
                });
                assert_eq!(rows.route_arm_first_operation(slot, arm, receive), expected);
            }
            for &view in &views {
                for &lane in &lanes {
                    let expected = steps
                        .clone()
                        .find(|&step| {
                            view_at(step) == view
                                && raw.local_step_lane(step, rows.footprint().logical_lane_count)
                                    == Some(lane)
                        })
                        .filter(|&entry| {
                            !steps.clone().any(|before| {
                                view_at(before) == view
                                    && rows.program().source_precedes(
                                        raw.local_step_event(before).unwrap().eff_index as usize,
                                        raw.local_step_event(entry).unwrap().eff_index as usize,
                                    )
                            })
                        });
                    assert_eq!(
                        rows.route_arm_view_lane_entry(slot, arm, view, lane),
                        expected,
                        "slot={slot} arm={arm} view={view} lane={lane}"
                    );
                }
            }
        }
    }
}

#[test]
fn compiled_offer_topology_matches_exhaustive_source_queries() {
    let global = g::seq(
        g::send::<1, 0, Msg<9, ()>>(),
        g::route(
            g::seq(
                g::par(
                    g::send::<0, 1, Msg<1, ()>>(),
                    g::seq(g::send::<0, 1, Msg<2, ()>>(), g::send::<1, 0, Msg<3, ()>>()),
                ),
                g::send::<0, 1, Msg<4, ()>>(),
            )
            .roll(),
            g::seq(
                g::send::<0, 1, Msg<5, ()>>(),
                g::route(g::send::<1, 0, Msg<6, ()>>(), g::send::<1, 0, Msg<7, ()>>()),
            ),
        ),
    )
    .roll();
    compare(project::<0>(&global).role_image_ref());
    compare(project::<1>(&global).role_image_ref());
}

#[test]
fn compiled_offer_topology_separates_delegated_co_resident_views() {
    type Loan<'r> = Msg<1, Endpoint<'r, 2>>;
    let global = g::seq(
        g::send::<0, 1, Loan<'_>>(),
        g::seq(
            g::par(
                g::route(
                    g::seq(g::send::<2, 1, Msg<2, ()>>(), g::send::<1, 2, Msg<3, ()>>()),
                    g::seq(g::send::<2, 1, Msg<4, ()>>(), g::send::<1, 2, Msg<5, ()>>()),
                ),
                g::send::<1, 3, Msg<6, ()>>(),
            ),
            g::send::<1, 0, Loan<'_>>(),
        ),
    )
    .roll();
    compare(project::<0>(&global).role_image_ref());
    compare(project::<1>(&global).role_image_ref());
    compare(project::<3>(&global).role_image_ref());
}

#[test]
fn constructor_rejects_every_mutated_offer_topology_byte() {
    let global = g::route(
        g::seq(g::send::<0, 1, Msg<1, ()>>(), g::send::<1, 0, Msg<2, ()>>()),
        g::par(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<4, ()>>()),
    );
    let rows = project::<0>(&global).role_image_ref();
    for offset in rows.columns.route_entry_steps().start..rows.columns.blob_len() {
        for bit in 0..8 {
            reject(rows, |bytes, _, _| bytes[offset] ^= 1 << bit);
        }
    }
}

#[test]
fn lane_successors_cover_a_protocol_without_routes() {
    let global = g::seq(
        g::par(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 2, Msg<2, ()>>()),
        g::seq(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 2, Msg<4, ()>>()),
    );
    let rows = project::<0>(&global).role_image_ref();
    assert_eq!(rows.columns.route_scopes.len, 0);
    compare(rows);
    for offset in rows.columns.lane_successors() {
        for bit in 0..8 {
            reject(rows, |bytes, _, _| bytes[offset] ^= 1 << bit);
        }
    }
}
