use super::{OperationKernel, PublicActiveOp, SendError, SendResult, Transport};
use crate::{
    endpoint::carrier::OperationRegion,
    global::typestate::{EventArmView, EventCommitMeta},
};

impl<'r, T: Transport + 'r> OperationKernel<'_, 'r, T> {
    pub(in crate::endpoint::kernel) fn borrowed_reentry_scope(
        &self,
        index: usize,
    ) -> Option<crate::global::const_dsl::ScopeId> {
        let (scope, epoch) = self.region.reentry()?;
        let current =
            self.progress
                .cursor
                .borrowed_roll_reentry_is_current(scope, epoch, &mut |scope| {
                    self.selected_arm_for_scope(scope)
                });
        (current
            && self
                .progress
                .cursor
                .roll_scope_complete_for_entry(scope, index, &mut |scope| {
                    self.selected_arm_for_scope(scope)
                }))
        .then_some(scope)
    }

    pub(crate) fn event_enabled(
        &self,
        index: usize,
        mut event: EventCommitMeta,
        arms: &mut dyn FnMut(crate::global::const_dsl::ScopeId, EventArmView) -> Option<u8>,
    ) -> Result<
        crate::global::typestate::EnabledEventCommit,
        crate::global::typestate::EventAdmissionError,
    > {
        event.body_reentry = self
            .borrowed_reentry_scope(index)
            .unwrap_or_else(crate::global::const_dsl::ScopeId::none);
        self.progress.cursor.event_enabled(index, event, arms)
    }

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
        self.progress
            .cursor
            .event_row_at(index)
            .is_some_and(|row| self.region.contains(row.effect().dense_ordinal()))
    }

    pub(crate) fn owns_lane(&self, lane: usize) -> bool {
        if self.region.is_whole() {
            return true;
        }
        self.progress
            .cursor
            .parallel_borrow_owns_lane(self.region.mask(), lane)
    }

    fn arm_lane(&self, source: core::ops::Range<usize>) -> Option<usize> {
        (0..self.progress.cursor.local_steps_len()).find_map(|index| {
            let row = self.progress.cursor.event_row_at(index)?;
            (row.logical_role() == self.public_header.role()
                && source.contains(&row.effect().dense_ordinal()))
            .then_some(usize::from(row.atom().lane))
        })
    }

    pub(in crate::endpoint::kernel) fn enabled_view_event(&self, index: usize) -> Option<usize> {
        let (source, admission) = self.view_event_admission(index, &mut self.preview_queries())?;
        admission.is_ok().then_some(source)
    }

    pub(in crate::endpoint::kernel) fn view_event_admission(
        &self,
        index: usize,
        previews: &mut dyn FnMut(crate::global::const_dsl::ScopeId) -> Option<u8>,
    ) -> Option<(
        usize,
        Result<(), crate::global::typestate::EventAdmissionError>,
    )> {
        let row = self.progress.cursor.event_row_at(index)?;
        if row.logical_role() != self.public_header.role()
            || !self.region.contains(row.effect().dense_ordinal())
        {
            return None;
        }
        let event = row.commit_meta();
        let conflict = row.conflict();
        let admission = self.event_enabled(index, event, &mut |scope, view| match view {
            // A fresh preview can select another arm, but cannot
            // change which arm completed the preceding roll visit.
            EventArmView::Committed => self.selected_arm_for_scope(scope),
            EventArmView::Preview => {
                previews(scope).or_else(|| self.selected_arm_for_recv_event(conflict, scope, view))
            }
        });
        Some((event.eff_index.dense_ordinal(), admission.map(|_| ())))
    }

    fn parallel_region_covers_frontier(
        &self,
        source: core::ops::Range<usize>,
        reentry: Option<crate::global::const_dsl::ScopeId>,
        previews: &mut dyn FnMut(crate::global::const_dsl::ScopeId) -> Option<u8>,
    ) -> bool {
        let mut inside = false;
        // Repeating a completed group and taking its sequential continuation
        // are alternatives. Independent siblings still belong to the frontier.
        for index in 0..self.progress.cursor.local_steps_len() {
            let Some((event, admission)) = self.view_event_admission(index, previews) else {
                continue;
            };
            // A borrow performs no operation. This view can borrow its next
            // fragment while another held view supplies its prefix; the send
            // or receive still waits for that prefix through event_enabled.
            if !matches!(
                admission,
                Ok(())
                    | Err(
                        crate::global::typestate::EventAdmissionError::OtherViewPending
                            | crate::global::typestate::EventAdmissionError::PayloadPredecessorPending
                    )
            ) {
                continue;
            }
            if source.contains(&event) {
                inside = true;
            } else if !self.progress.cursor.completed_roll_alternative_to(
                index,
                source.start,
                &mut |scope| self.selected_arm_for_scope(scope),
            ) && !reentry
                .is_some_and(|scope| self.context.program.roll_precedes(scope, event))
                && !self
                    .progress
                    .cursor
                    .retained_roll_continuation_after(index, source.clone())
            {
                return false;
            }
        }
        inside
    }

    /// Flatten only associative parallel composition. Sequential prefixes and
    /// enclosing repetitions retain their own execution boundary.
    fn associative_parallel_at(&self, region: OperationRegion) -> Option<(usize, usize, usize)> {
        let bounds = region.bounds();
        let program = self.context.program;
        for marker in program.source_markers_at(bounds.start) {
            if let Some((_, start, end)) = program.source_roll_at(marker)
                && start == bounds.start
                && end == bounds.end
            {
                return None;
            }
        }
        for marker in program.source_markers_at(bounds.start) {
            let Some((start, split, end)) = program.source_parallel_at(marker) else {
                continue;
            };
            if start != bounds.start || end != bounds.end {
                continue;
            }
            return Some((start, split, end));
        }
        None
    }

    pub(crate) fn split_parallel(&mut self, regions: &mut [OperationRegion]) -> SendResult<()> {
        if let Some(fault) = self.session_fault() {
            return Err(SendError::SessionFault(fault));
        }
        if regions.is_empty()
            || !matches!(
                self.operation.public_active_op,
                PublicActiveOp::Idle | PublicActiveOp::RestoredRouteBranch
            )
            || self.has_other_active_operations()
        {
            return Err(SendError::PhaseInvariant);
        }
        let ((start, split, end), origin) = {
            let mut previews = self.preview_queries();
            let mut repeated = None;
            let current =
                (0..self.context.program.source_scope_marker_count()).find_map(|marker| {
                    let (start, split, end) = self.context.program.source_parallel_at(marker)?;
                    let parent = self.region.bounds();
                    if start < parent.start
                        || end > parent.end
                        || self.arm_lane(start..end).is_none()
                    {
                        return None;
                    }
                    let origin = self
                        .region
                        .reentry()
                        .filter(|&(scope, epoch)| {
                            self.progress.cursor.borrowed_roll_reentry_is_current(
                                scope,
                                epoch,
                                &mut |scope| self.selected_arm_for_scope(scope),
                            )
                        })
                        .or_else(|| {
                            self.progress.cursor.completed_parallel_reentry(
                                start,
                                end,
                                &mut |scope, view| match view {
                                    EventArmView::Committed => self.selected_arm_for_scope(scope),
                                    EventArmView::Preview => previews(scope)
                                        .or_else(|| self.selected_arm_for_scope(scope)),
                                },
                            )
                        });
                    let candidate = ((start, split, end), origin);
                    // Classify completed repetitions before comparing enabled groups.
                    if origin.is_some() && repeated.is_some() {
                        return None;
                    }
                    if !self.parallel_region_covers_frontier(
                        start..end,
                        origin.map(|(scope, _)| scope),
                        &mut previews,
                    ) {
                        return None;
                    }
                    if origin.is_none() {
                        Some(candidate)
                    } else {
                        repeated = Some(candidate);
                        None
                    }
                });
            current.or(repeated).ok_or(SendError::PhaseInvariant)?
        };

        // Preserve the current forward group before choosing an alternative
        // restart. Scope selection precedes width checks and borrow issuance.
        let left = self.arm_lane(start..split);
        let right = self.arm_lane(split..end);
        if left.is_some() && left == right {
            return Err(SendError::PhaseInvariant);
        }
        let mut count = 0;
        for (start, end, lane) in [(start, split, left), (split, end, right)] {
            if let Some(lane) = lane {
                let slot = regions.get_mut(count).ok_or(SendError::PhaseInvariant)?;
                *slot =
                    OperationRegion::parallel(start, end, lane).ok_or(SendError::PhaseInvariant)?;
                count += 1;
            }
        }
        if count == 0 {
            return Err(SendError::PhaseInvariant);
        }
        let mut index = 0;
        while index < count {
            let Some((start, split, end)) = self.associative_parallel_at(regions[index]) else {
                index += 1;
                continue;
            };
            match (self.arm_lane(start..split), self.arm_lane(split..end)) {
                (Some(left), Some(right)) => {
                    if count == regions.len() {
                        return Err(SendError::PhaseInvariant);
                    }
                    regions.copy_within(index + 1..count, index + 2);
                    regions[index] = OperationRegion::parallel(start, split, left)
                        .ok_or(SendError::PhaseInvariant)?;
                    regions[index + 1] = OperationRegion::parallel(split, end, right)
                        .ok_or(SendError::PhaseInvariant)?;
                    count += 1;
                }
                (Some(lane), None) => {
                    regions[index] = OperationRegion::parallel(start, split, lane)
                        .ok_or(SendError::PhaseInvariant)?;
                }
                (None, Some(lane)) => {
                    regions[index] = OperationRegion::parallel(split, end, lane)
                        .ok_or(SendError::PhaseInvariant)?;
                }
                (None, None) => return Err(SendError::PhaseInvariant),
            }
        }
        if count != regions.len() {
            return Err(SendError::PhaseInvariant);
        }
        for (index, region) in regions.iter().enumerate() {
            if regions[..index]
                .iter()
                .any(|previous| previous.lane(0) == region.lane(0))
            {
                return Err(SendError::PhaseInvariant);
            }
        }
        for region in regions.iter_mut() {
            let source = region.bounds();
            let mask = self
                .progress
                .cursor
                .parallel_borrow_mask(self.public_header.role(), source.start, source.end)
                .ok_or(SendError::PhaseInvariant)?;
            region.bind_mask(mask);
        }
        for region in regions.iter_mut() {
            region.bind_reentry(origin);
        }
        self.move_parked_branch_to_parallel_borrow(regions)?;
        Ok(())
    }

    /// A parked preview belongs to the fragment containing its exact first
    /// event, which need not be the parent's primary operation slot. Move its
    /// existing I/O owner; never clone a frame, choice or protocol cursor.
    fn move_parked_branch_to_parallel_borrow(
        &mut self,
        regions: &[OperationRegion],
    ) -> SendResult<()> {
        if self.operation.public_active_op != PublicActiveOp::RestoredRouteBranch {
            return Ok(());
        }
        let branch = self
            .operation
            .public_route_branch
            .as_ref()
            .ok_or(SendError::PhaseInvariant)?;
        let event = branch.branch_meta.eff_index.dense_ordinal();
        let region = regions
            .iter()
            .find(|region| region.contains(event))
            .ok_or(SendError::PhaseInvariant)?;
        let target =
            crate::invariant_some(self.operations.get(region.lane(0)).and_then(Option::as_ref));
        let target = target.state.get();
        if core::ptr::eq(target, self.operation) {
            return Ok(());
        }
        // SAFETY: the common cursor borrow excludes another kernel. Admission
        // rejected every other active operation in this parent region, and the
        // exclusive parent borrow excludes child futures and payload borrows.
        // The distinct target slot belongs to one of the validated fragments.
        let target = unsafe { &mut *target };
        if target.public_active_op != PublicActiveOp::Idle || target.public_route_branch.is_some() {
            return Err(SendError::PhaseInvariant);
        }
        core::mem::swap(self.operation, target);
        Ok(())
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
