use super::*;
use crate::{Endpoint, global::typestate::EventCommitMeta};
use std::collections::BTreeSet;

/// Compare candidate elimination with the complete production admission check
/// across every completion bitmap of these small, publicly projected protocols.
fn compare(rows: &'static RoleImageRef) -> (usize, usize) {
    let mut removed = 0;
    let mut reopened = 0;
    with_rows(rows, |cursor| {
        let mut contracts = BTreeSet::from([(255, 255)]);
        for index in 0..cursor.local_steps_len() {
            if let Some(meta) = cursor.try_recv_meta_at(index) {
                contracts.insert((cursor.logical_role_at(index).unwrap(), meta.label));
            }
        }
        for bitmap in 0usize..1usize.checked_shl(cursor.local_steps_len() as u32).unwrap() {
            for index in 0..cursor.local_steps_len() {
                if bitmap & (1 << index) == 0 {
                    cursor.clear_local_event_done(index);
                } else {
                    cursor.mark_local_event_done(index);
                }
            }
            let before = (
                cursor.index(),
                cursor.lane_cursors().to_vec(),
                cursor.completed_event_words().to_vec(),
            );
            for &(role, label) in &contracts {
                let indexed: std::vec::Vec<_> = rows.receive_candidates(role, label).collect();
                let live: std::vec::Vec<_> = cursor.receive_candidates(role, label).collect();
                assert!(live.iter().all(|index| indexed.contains(index)));
                for index in indexed {
                    let done = bitmap & (1 << index) != 0;
                    if !live.contains(&index) {
                        removed += 1;
                        assert!(done);
                        let meta = EventCommitMeta::from(cursor.try_recv_meta_at(index).unwrap());
                        for arm in [None, Some(0), Some(1)] {
                            assert!(matches!(
                                cursor.event_enabled(index, meta, &mut |_, _| arm),
                                Err(EventAdmissionError::Invariant
                                    | EventAdmissionError::PredecessorPending)
                            ));
                        }
                    } else if done {
                        let meta = EventCommitMeta::from(cursor.try_recv_meta_at(index).unwrap());
                        for arm in [None, Some(0), Some(1)] {
                            if cursor.event_enabled(index, meta, &mut |_, _| arm).is_ok() {
                                reopened += 1;
                            }
                        }
                    }
                }
            }
            assert_eq!(
                (
                    cursor.index(),
                    cursor.lane_cursors().to_vec(),
                    cursor.completed_event_words().to_vec(),
                ),
                before,
                "candidate queries must not change progress"
            );
        }
    });
    (removed, reopened)
}

#[test]
fn completed_receive_elimination_agrees_with_full_admission() {
    type A = Msg<1, u32>;
    type B = Msg<2, u32>;
    type C = Msg<3, u32>;
    type D = Msg<4, u32>;
    let seq = g::seq(
        g::send::<0, 1, A>(),
        g::seq(g::send::<1, 0, B>(), g::send::<0, 1, A>()),
    );
    let par = g::par(
        g::seq(g::send::<0, 1, A>(), g::send::<1, 0, B>()),
        g::seq(g::send::<2, 1, C>(), g::send::<1, 2, D>()),
    );
    let route = g::route(
        g::seq(g::send::<0, 1, A>(), g::send::<1, 0, B>()),
        g::seq(g::send::<0, 1, C>(), g::send::<1, 0, B>()),
    );
    type Loan = Msg<9, Endpoint<'static, 10>>;
    let owned = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::send::<10, 2, A>(),
            g::seq(g::send::<2, 10, B>(), g::send::<1, 0, Loan>()),
        ),
    );
    for rows in [
        project::<0>(&seq).role_image_ref(),
        project::<1>(&seq).role_image_ref(),
        project::<1>(&par).role_image_ref(),
        project::<1>(&route).role_image_ref(),
        project::<0>(&owned).role_image_ref(),
        project::<1>(&owned).role_image_ref(),
    ] {
        let (removed, reopened) = compare(rows);
        assert!(removed > 0);
        assert_eq!(reopened, 0);
    }
}

#[test]
fn completed_rolled_receives_remain_candidates_for_a_fresh_visit() {
    type A = Msg<1, u32>;
    type B = Msg<2, u32>;
    let seq = g::seq(g::send::<0, 1, A>(), g::send::<1, 0, B>()).roll();
    let route = g::route(g::send::<0, 1, A>(), g::send::<0, 1, B>()).roll();
    for rows in [
        project::<1>(&seq).role_image_ref(),
        project::<1>(&route).role_image_ref(),
    ] {
        let (removed, reopened) = compare(rows);
        assert_eq!(removed, 0);
        assert!(reopened > 0);
    }
}
