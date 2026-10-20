use super::*;
use crate::{
    Endpoint,
    eff::EventPayload,
    global::typestate::{EventCommitMeta, MessageSelector},
};

fn compare(rows: &'static RoleImageRef) -> (usize, usize) {
    let mut removed = 0;
    let mut reopened = 0;
    with_rows(rows, |cursor| {
        let mut contracts = std::vec![MessageSelector::new(255, 255, EventPayload::Wire(0))];
        let mut events = std::vec::Vec::new();
        for index in 0..cursor.local_steps_len() {
            let operation = cursor
                .try_send_meta_at(index)
                .map(|meta| (EventCommitMeta::from(meta), meta.payload_kind))
                .or_else(|| {
                    cursor.try_local_meta_at(index).map(|meta| {
                        (
                            EventCommitMeta::new(
                                meta.eff_index,
                                meta.label,
                                meta.origin,
                                meta.scope,
                                meta.route_arm,
                                meta.lane,
                            ),
                            meta.payload_kind,
                        )
                    })
                });
            if let Some((event, payload)) = operation {
                let role = cursor.logical_role_at(index).unwrap();
                contracts.push(MessageSelector::new(role, event.label, payload));
                contracts.push(MessageSelector::new(
                    role.wrapping_add(1),
                    event.label,
                    payload,
                ));
                contracts.push(MessageSelector::new(
                    role,
                    event.label.wrapping_add(1),
                    payload,
                ));
                contracts.push(MessageSelector::new(
                    role,
                    event.label,
                    EventPayload::Wire(payload.encoded_value()),
                ));
                contracts.push(MessageSelector::new(
                    role,
                    event.label,
                    EventPayload::Wire(255),
                ));
                contracts.push(MessageSelector::new(
                    role,
                    event.label,
                    EventPayload::Endpoint(255),
                ));
                events.push((index, role, event, payload));
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
            for &selector in &contracts {
                let candidates: std::vec::Vec<_> = cursor.outbound_candidates(selector).collect();
                let mut expected = std::vec::Vec::new();
                for &(index, role, event, payload) in &events {
                    if role != selector.role
                        || event.label != selector.label
                        || payload != selector.payload_kind
                    {
                        continue;
                    }
                    let done = bitmap & (1 << index) != 0;
                    if !done || cursor.has_reentry_scopes() {
                        expected.push(index);
                        if done {
                            for arm in [None, Some(0), Some(1)] {
                                if cursor.event_enabled(index, event, &mut |_, _| arm).is_ok() {
                                    reopened += 1;
                                }
                            }
                        }
                    } else {
                        removed += 1;
                        for arm in [None, Some(0), Some(1)] {
                            assert!(matches!(
                                cursor.event_enabled(index, event, &mut |_, _| arm),
                                Err(EventAdmissionError::Invariant
                                    | EventAdmissionError::PredecessorPending)
                            ));
                        }
                    }
                }
                assert_eq!(candidates, expected);
            }
            assert_eq!(
                (
                    cursor.index(),
                    cursor.lane_cursors().to_vec(),
                    cursor.completed_event_words().to_vec(),
                ),
                before,
            );
        }
    });
    (removed, reopened)
}

#[test]
fn outbound_filter_matches_full_metadata_and_keeps_live_admission() {
    type A = Msg<1, u32>;
    type B = Msg<2, u32>;
    type C = Msg<3, u32>;
    type D = Msg<4, u32>;
    type Local = Msg<5, ()>;
    type Loan = Msg<9, Endpoint<'static, 10>>;
    let seq = g::seq(
        g::send::<0, 1, A>(),
        g::seq(g::send::<1, 0, B>(), g::send::<0, 1, A>()),
    );
    let local = g::seq(g::send::<0, 0, Local>(), g::send::<0, 1, A>());
    let par = g::par(
        g::seq(g::send::<0, 1, A>(), g::send::<1, 0, B>()),
        g::seq(g::send::<2, 1, C>(), g::send::<1, 2, D>()),
    );
    let route = g::route(
        g::seq(g::send::<0, 1, A>(), g::send::<1, 0, B>()),
        g::seq(g::send::<0, 1, C>(), g::send::<1, 0, B>()),
    );
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
        project::<0>(&local).role_image_ref(),
        project::<1>(&par).role_image_ref(),
        project::<0>(&route).role_image_ref(),
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
fn completed_outbound_steps_remain_available_for_roll_reentry() {
    type A = Msg<1, u32>;
    type B = Msg<2, u32>;
    let seq = g::seq(g::send::<0, 1, A>(), g::send::<1, 0, B>()).roll();
    let route = g::route(g::send::<0, 1, A>(), g::send::<0, 1, B>()).roll();
    for rows in [
        project::<0>(&seq).role_image_ref(),
        project::<0>(&route).role_image_ref(),
    ] {
        let (removed, reopened) = compare(rows);
        assert_eq!(removed, 0);
        assert!(reopened > 0);
    }
}
