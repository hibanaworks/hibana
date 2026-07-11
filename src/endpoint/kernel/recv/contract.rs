use crate::{
    endpoint::kernel::core::CursorEndpoint,
    endpoint::{RecvError, RecvResult},
    global::typestate::EventCommitMeta,
    transport::Transport,
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(in crate::endpoint::kernel::recv) fn live_recv_contract_on_lane(
        &self,
        lane_idx: usize,
        target_label: u8,
        target_schema: u32,
    ) -> bool {
        let mut idx = 0usize;
        while idx < self.cursor.local_steps_len() {
            if let Some(meta) = self.cursor.try_recv_meta_at(idx)
                && meta.label == target_label
                && meta.payload_schema == target_schema
                && meta.lane as usize == lane_idx
                && !meta.origin.is_session()
                && {
                    let preview_conflict = self.cursor.event_conflict_for_index(idx);
                    let mut selected_arm =
                        |scope| self.selected_arm_for_recv_event(preview_conflict, scope);
                    self.cursor
                        .event_enabled(idx, EventCommitMeta::from(meta), &mut selected_arm)
                        .is_ok()
                }
            {
                return true;
            }
            idx += 1;
        }
        false
    }

    pub(in crate::endpoint::kernel::recv) fn ensure_live_recv_contract(
        &self,
        target_label: u8,
        target_schema: u32,
    ) -> RecvResult<()> {
        let mut expected_schema = None;
        let mut idx = 0usize;
        while idx < self.cursor.local_steps_len() {
            if let Some(meta) = self.cursor.try_recv_meta_at(idx)
                && meta.label == target_label
                && !meta.origin.is_session()
            {
                let preview_conflict = self.cursor.event_conflict_for_index(idx);
                let mut selected_arm =
                    |scope| self.selected_arm_for_recv_event(preview_conflict, scope);
                if self
                    .cursor
                    .event_enabled(idx, EventCommitMeta::from(meta), &mut selected_arm)
                    .is_ok()
                {
                    if meta.payload_schema == target_schema {
                        return Ok(());
                    }
                    expected_schema = Some(meta.payload_schema);
                }
            }
            idx += 1;
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
