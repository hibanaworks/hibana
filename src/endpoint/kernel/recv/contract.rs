use crate::{
    endpoint::kernel::core::CursorEndpoint,
    endpoint::{RecvError, RecvResult},
    global::{
        role_program::{
            LANE_SET_VIEW_WORDS, LaneSetView, LaneWord, lane_word_count, lane_word_index,
        },
        typestate::EventCommitMeta,
    },
    transport::Transport,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel::recv) fn live_recv_lanes<'lanes>(
        &self,
        target_label: u8,
        target_schema: u32,
        words: &'lanes mut [LaneWord; LANE_SET_VIEW_WORDS],
    ) -> RecvResult<LaneSetView<'lanes>> {
        words.fill(0);
        let mut expected_schema = None;
        let mut idx = 0usize;
        while idx < self.cursor.local_steps_len() {
            if let Some(meta) = self.cursor.try_recv_meta_at(idx)
                && meta.label == target_label
                && !meta.origin.is_session()
            {
                let preview_conflict = self.cursor.event_conflict_for_index(idx);
                let mut selected_arm =
                    |scope, view| self.selected_arm_for_recv_event(preview_conflict, scope, view);
                if self
                    .cursor
                    .event_enabled(idx, EventCommitMeta::from(meta), &mut selected_arm)
                    .is_ok()
                {
                    if meta.payload_schema == target_schema {
                        let lane = meta.lane as usize;
                        if lane >= self.cursor.logical_lane_count() {
                            return Err(RecvError::PhaseInvariant);
                        }
                        let (word, bit) = lane_word_index(lane);
                        words[word] |= bit;
                    } else {
                        expected_schema = Some(meta.payload_schema);
                    }
                }
            }
            idx += 1;
        }
        // The existing u8 lane domain bounds this temporary set to 32 bytes.
        // Every admitted occurrence remains available to the full-key unique
        // match; this set only avoids re-reading all rows for each empty lane.
        // SAFETY: The initialized words outlive the returned borrowed view;
        // the descriptor's lane count is bounded by the same wire domain.
        let lanes = unsafe {
            LaneSetView::from_parts(
                words.as_ptr(),
                lane_word_count(self.cursor.logical_lane_count()),
            )
        };
        if lanes.first_set(self.cursor.logical_lane_count()).is_some() {
            return Ok(lanes);
        }
        match expected_schema {
            Some(expected) => Err(RecvError::SchemaMismatch {
                expected,
                actual: target_schema,
            }),
            None => Err(RecvError::PhaseInvariant),
        }
    }
}
