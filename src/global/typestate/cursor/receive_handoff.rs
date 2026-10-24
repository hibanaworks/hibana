use super::{EventCursor, InboundFrameKey, ScopeId, state_index_to_usize};
use crate::eff::EventPayload;

impl EventCursor {
    /// A frame for a later holder stays in the logical receive direction.
    /// The source identity and an unfinished outgoing transfer establish that
    /// this possession cannot consume it. This grants no local receive step.
    pub(crate) fn receive_follows_pending_transfer(
        &self,
        role: u8,
        key: InboundFrameKey,
        committed: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let source = usize::from(key.frame_label);
        if source == crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY {
            return false;
        }
        let program = self.program_ref();
        if !program.has_actor_bindings() {
            return false;
        }
        let Some(atom) = program.atom_at(source) else {
            return false;
        };
        if atom.origin.is_session()
            || atom.from == atom.to
            || atom.from != key.source_role
            || atom.to != role
            || atom.lane != key.lane
        {
            return false;
        }
        for step in self.pending_event_steps(0..self.local_steps_len()) {
            let index = state_index_to_usize(crate::invariant_some(
                self.machine().state_for_step_index(step),
            ));
            let Some(transfer) = self.try_send_meta_at(index) else {
                continue;
            };
            if transfer.payload_kind != EventPayload::Endpoint(role)
                || !program.source_precedes(transfer.eff_index.dense_ordinal(), source)
            {
                continue;
            }
            let conflict = self.machine().event_conflict_for_index(index);
            let mut selected =
                |scope| self.selected_arm_for_reentry_preview_conflict(scope, conflict, committed);
            if self.event_actor_is_local(index, &mut selected) != Some(false)
                && self.event_conflict_row_allows_with_preview(conflict, conflict, &mut selected)
            {
                return true;
            }
        }
        false
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::*;
    use crate::{g, runtime::program::project};

    #[test]
    fn only_the_exact_future_receive_can_wait_for_an_unfinished_transfer() {
        type Loan<'r> = g::Msg<2, crate::Endpoint<'r, 3>>;
        let global = g::seq(
            g::route(
                g::send::<0, 1, g::Msg<0, ()>>(),
                g::send::<0, 3, g::Msg<1, ()>>(),
            ),
            g::seq(
                g::send::<1, 2, Loan<'_>>(),
                g::send::<0, 3, g::Msg<3, ()>>(),
            ),
        );
        let program = project::<1>(&global);
        super::super::validated_metadata_tests::with_rows(program.role_image_ref(), |cursor| {
            assert!(matches!(
                cursor.try_send_meta_at(2).unwrap().payload_kind,
                EventPayload::Endpoint(3)
            ));
            for completed in [false, true] {
                if completed {
                    cursor.mark_local_event_done(2);
                }
                for frame_label in 0..=u16::MAX {
                    for source in [0, 1, 2, 3, 255] {
                        for lane in [0, 1, 255] {
                            let key = InboundFrameKey::new(source, lane, frame_label);
                            assert_eq!(
                                cursor.receive_follows_pending_transfer(3, key, &mut |_| None),
                                !completed && frame_label == 3 && source == 0 && lane == 0,
                                "completed={completed} key={key:?}"
                            );
                        }
                    }
                }
            }
        });
    }
}
