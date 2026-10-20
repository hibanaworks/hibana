use super::*;
use crate::{Endpoint, global::typestate::LocalAction};
use std::collections::BTreeSet;

fn compare(rows: &RoleImageRef) {
    let mut roles = BTreeSet::from([0, 254, 255]);
    let mut labels = BTreeSet::from([0, 254, 255]);
    for step in 0..rows.local_step_count() {
        let node = rows.local_step_node(step).unwrap();
        if let LocalAction::Recv {
            eff_index, label, ..
        } = node.action()
        {
            roles.insert(rows.program().event_atom_at(eff_index.dense_ordinal()).to);
            labels.insert(label);
        }
    }
    for role in roles {
        for &label in &labels {
            let expected: std::vec::Vec<_> = (0..rows.local_step_count())
                .filter(|&step| {
                    let node = rows.local_step_node(step).unwrap();
                    match node.action() {
                        LocalAction::Recv {
                            eff_index,
                            label: candidate,
                            ..
                        } => {
                            candidate == label
                                && rows.program().event_atom_at(eff_index.dense_ordinal()).to
                                    == role
                        }
                        LocalAction::Send { .. }
                        | LocalAction::Local { .. }
                        | LocalAction::Terminate => false,
                    }
                })
                .collect();
            assert_eq!(
                rows.receive_candidates(role, label)
                    .collect::<std::vec::Vec<_>>(),
                expected
            );
        }
    }
}

#[test]
fn indexed_receives_match_decoded_rows_with_duplicates_parallelism_and_choices() {
    type A = Msg<7, u32>;
    type B = Msg<7, bool>;
    type C = Msg<255, ()>;
    let global = g::seq(
        g::par(g::send::<2, 1, A>(), g::send::<3, 1, B>()),
        g::seq(
            g::send::<1, 0, C>(),
            g::route(g::send::<0, 1, A>(), g::send::<0, 1, C>()),
        ),
    );
    compare(project::<0>(&global).role_image_ref());
    compare(project::<1>(&global).role_image_ref());
    let local = g::send::<0, 0, Msg<0, ()>>();
    compare(project::<0>(&local).role_image_ref());
    let edge = g::seq(g::send::<255, 0, C>(), g::send::<0, 255, C>());
    compare(project::<0>(&edge).role_image_ref());
    compare(project::<255>(&edge).role_image_ref());
    let absent = project::<254>(&edge);
    assert_eq!(absent.role_image_ref().local_step_count(), 0);
    compare(absent.role_image_ref());
}

#[test]
fn actor_receive_index_preserves_logical_roles_and_rolled_rows() {
    type Loan = Msg<1, Endpoint<'static, 10>>;
    type Read = Msg<3, u32>;
    type Value = Msg<4, u32>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::par(
                g::seq(g::send::<10, 2, Read>(), g::send::<2, 10, Value>()).roll(),
                g::send::<3, 4, Read>(),
            ),
            g::send::<1, 0, Loan>(),
        ),
    );
    compare(project::<0>(&global).role_image_ref());
    compare(project::<1>(&global).role_image_ref());
    compare(project::<10>(&global).role_image_ref());
}

#[test]
fn malformed_receive_index_cannot_publish_an_image() {
    let global = g::seq(
        g::send::<0, 1, Msg<9, ()>>(),
        g::seq(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<9, ()>>()),
    );
    let program = project::<1>(&global);
    let rows = program.role_image_ref();
    let offset = rows.columns.receive_index().offset as usize;
    reject(rows, |bytes, _, _| put(bytes, offset, u16::MAX));
    reject(rows, |bytes, _, _| {
        let first = u16_at(bytes, offset);
        put(bytes, offset + 2, first);
    });
    reject(rows, |bytes, _, _| {
        let first = u16_at(bytes, offset);
        let last = u16_at(bytes, offset + 4);
        put(bytes, offset, last);
        put(bytes, offset + 4, first);
    });
}
