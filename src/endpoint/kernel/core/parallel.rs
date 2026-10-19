use super::{OperationKernel, PublicActiveOp, SendError, SendResult, Transport};
use crate::{
    endpoint::carrier::OperationRegion,
    global::typestate::{EventCommitMeta, LocalAction},
};

impl<'r, T: Transport + 'r> OperationKernel<'_, 'r, T> {
    pub(crate) fn has_other_active_operations(&self) -> bool {
        // This admission check runs before a fresh borrow starts, rather than
        // in the transport poll loop. Forgotten children cannot be overwritten
        // when Rust permits the parent borrow to end without running Drop.
        self.operations.iter().enumerate().any(|(lane, slot)| {
            let operation = crate::invariant_some(slot.as_ref());
            !core::ptr::eq(operation.state.get(), self.operation)
                // SAFETY: the common cursor borrow excludes another mutable
                // kernel, and the current operation cell was excluded above.
                && unsafe { (*operation.state.get()).public_active_op } != PublicActiveOp::Idle
                && self.owns_lane(lane)
        })
    }

    pub(crate) fn permits_event(&self, index: usize) -> bool {
        match self.progress.cursor.action_at(index) {
            LocalAction::Send { eff_index, .. }
            | LocalAction::Recv { eff_index, .. }
            | LocalAction::Local { eff_index, .. } => {
                self.region.contains(eff_index.dense_ordinal())
            }
            LocalAction::Terminate => false,
        }
    }

    pub(crate) fn owns_lane(&self, lane: usize) -> bool {
        if self.region.is_whole() {
            return true;
        }
        (0..self.progress.cursor.local_steps_len()).any(|index| {
            self.progress.cursor.logical_role_at(index) == Some(self.public_header.role())
                && self.permits_event(index)
                && match self.progress.cursor.action_at(index) {
                    LocalAction::Send { lane: actual, .. }
                    | LocalAction::Recv { lane: actual, .. }
                    | LocalAction::Local { lane: actual, .. } => usize::from(actual) == lane,
                    LocalAction::Terminate => false,
                }
        })
    }

    fn arm_lane(&self, source: core::ops::Range<usize>) -> Option<usize> {
        (0..self.progress.cursor.local_steps_len()).find_map(|index| {
            let (eff, lane) = match self.progress.cursor.action_at(index) {
                LocalAction::Send {
                    eff_index, lane, ..
                }
                | LocalAction::Recv {
                    eff_index, lane, ..
                }
                | LocalAction::Local {
                    eff_index, lane, ..
                } => (eff_index, lane),
                LocalAction::Terminate => return None,
            };
            (self.progress.cursor.logical_role_at(index) == Some(self.public_header.role())
                && source.contains(&eff.dense_ordinal()))
            .then_some(usize::from(lane))
        })
    }

    fn region_has_enabled_event(&self, source: core::ops::Range<usize>) -> bool {
        (0..self.progress.cursor.local_steps_len()).any(|index| {
            if self.progress.cursor.logical_role_at(index) != Some(self.public_header.role())
                || !self.permits_event(index)
            {
                return false;
            }
            let event: EventCommitMeta =
                if let Some(meta) = self.progress.cursor.try_send_meta_at(index) {
                    meta.into()
                } else if let Some(meta) = self.progress.cursor.try_recv_meta_at(index) {
                    meta.into()
                } else if let Some(meta) = self.progress.cursor.try_local_meta_at(index) {
                    EventCommitMeta::new(
                        meta.eff_index,
                        meta.label,
                        meta.origin,
                        meta.scope,
                        meta.route_arm,
                        meta.lane,
                    )
                } else {
                    return false;
                };
            if !source.contains(&event.eff_index.dense_ordinal()) {
                return false;
            }
            let conflict = self.progress.cursor.event_conflict_for_index(index);
            self.progress
                .cursor
                .event_enabled(index, event, &mut |scope, view| {
                    self.selected_arm_for_recv_event(conflict, scope, view)
                })
                .is_ok()
        })
    }

    pub(crate) fn split_parallel(&mut self) -> SendResult<[OperationRegion; 2]> {
        if let Some(fault) = self.session_fault() {
            return Err(SendError::SessionFault(fault));
        }
        if self.operation.public_active_op != PublicActiveOp::Idle
            || self.has_other_active_operations()
        {
            return Err(SendError::PhaseInvariant);
        }
        for marker in 0..self.context.program.source_scope_marker_count() {
            let Some((start, split, end)) = self.context.program.source_parallel_at(marker) else {
                continue;
            };
            let parent = self.region.bounds();
            if start < parent.start
                || end > parent.end
                || !self.region_has_enabled_event(start..end)
            {
                continue;
            }
            let (Some(left), Some(right)) =
                (self.arm_lane(start..split), self.arm_lane(split..end))
            else {
                continue;
            };
            if left == right {
                return Err(SendError::PhaseInvariant);
            }
            let left =
                OperationRegion::parallel(start, split, left).ok_or(SendError::PhaseInvariant)?;
            let right =
                OperationRegion::parallel(split, end, right).ok_or(SendError::PhaseInvariant)?;
            return Ok([left, right]);
        }
        Err(SendError::PhaseInvariant)
    }

    pub(crate) fn finish_parallel(
        &mut self,
        waiters: &mut crate::endpoint::carrier::WaiterTransfer,
    ) {
        if self.region.is_whole() {
            crate::invariant();
        }
        self.clear_endpoint_waiter(waiters);
        if !self.projection_complete() || self.operation.public_active_op != PublicActiveOp::Idle {
            self.poison_session(crate::rendezvous::SessionFaultKind::EndpointDropped);
        }
        self.finish_operation_io();
    }
}
