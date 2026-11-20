//! Read-only recovery admission over the projected resource receipt index.
use super::{EventAdmissionError, EventArmView, EventCursor, ScopeId};
use crate::global::{
    role_program::{PackedLaneRange, RoleLocalDirection},
    typestate::{EventCommitMeta, PackedEventConflict, state_index_to_usize},
};

impl EventCursor {
    /// An empty receive cannot use a previous visit if its FIFO has already
    /// delivered evidence of the current visit, whose required send must
    /// come from this very view. Leave queued frames to ordinary matching.
    pub(crate) fn retained_receive_waits_for_own_send(
        &self,
        index: usize,
        holds: impl Fn(usize) -> bool,
        committed: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let Some(target) = self.event_row_at(index) else {
            return false;
        };
        let target_atom = target.atom();
        let target_source = target.effect().dense_ordinal();
        let program = self.machine().event_program();
        self.pending_event_steps(0..index).any(|prior| {
            let prefix = crate::invariant_some(self.event_row_at(prior));
            let atom = prefix.atom();
            let source = prefix.effect().dense_ordinal();
            if prefix.direction() != RoleLocalDirection::Send
                || atom.from != target_atom.to
                || atom.to != target_atom.from
                || !holds(prior)
                || !self.program_ref().source_precedes(source, target_source)
                || self.event_actor_is_local(prior, committed) != Some(true)
                || !self.event_conflict_row_required_with_preview(
                    prefix.conflict(),
                    PackedEventConflict::none(),
                    committed,
                )
            {
                return false;
            }
            let Some(scope) = program.roll_ancestors(prior).next() else {
                return false;
            };
            let roll = crate::invariant_some(self.machine().roll_scope_row(scope));
            index >= roll.end()
                && self.roll_has_retained_completion(scope)
                && (roll.start()..prior).rev().any(|receipt| {
                    if !self.local_event_done(receipt) {
                        return false;
                    }
                    let row = crate::invariant_some(self.event_row_at(receipt));
                    let atom = row.atom();
                    row.direction() == RoleLocalDirection::Recv
                        && atom.from == target_atom.from
                        && atom.to == target_atom.to
                        && atom.lane == target_atom.lane
                        // A preserved child may still contain work from an
                        // older parent visit. Require a receipt in this body.
                        && program.roll_ancestors(receipt).next() == Some(scope)
                        && self.program_ref().source_precedes(row.effect().dense_ordinal(), source)
                        // The peer has already sent on a path that requires
                        // the missing request before it can send the result.
                        && self.event_conflict_row_required_with_preview(
                            prefix.conflict(),
                            row.conflict(),
                            &mut |_| None,
                        )
                })
        })
    }

    pub(super) fn reentry_view_wait(
        &self,
        index: usize,
        event: EventCommitMeta,
        borrowed: Option<&dyn Fn(u8) -> bool>,
        arms: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> EventAdmissionError {
        let Some(scope) = (!event.body_reentry.is_none())
            .then_some(event.body_reentry)
            .or_else(|| {
                self.complete_roll_body_scope_for_index(index, &mut |scope| {
                    arms(scope, EventArmView::Committed)
                })
            })
            .or_else(|| {
                // A completed entry can wait for the current visit on another
                // view. This only classifies the wait: the ordinary completion
                // check still owns permission to reset or start any I/O.
                self.local_event_done(index)
                    .then(|| {
                        self.machine()
                            .event_program()
                            .roll_ancestors(index)
                            .find(|&scope| {
                                self.roll_scope_head_allows_index(
                                    scope,
                                    index,
                                    event.lane,
                                    &mut |scope| arms(scope, EventArmView::Preview),
                                )
                            })
                    })
                    .flatten()
            })
        else {
            return EventAdmissionError::Invariant;
        };
        let (source, direction) =
            crate::invariant_some(self.machine().event_program().operation_at(index));
        let (role, sent) =
            direction.required_views(self.program_ref().event_atom_at(source.dense_ordinal()));
        let Some(row) = self.machine().roll_scope_row(scope) else {
            crate::invariant();
        };
        let conflict = self.machine().event_conflict_for_index(index);
        let mut selected = |scope| arms(scope, EventArmView::Preview);
        if self.retired_receipt_blocks(index, scope, &mut selected) {
            return EventAdmissionError::PredecessorPending;
        }
        let mut wait = EventAdmissionError::OtherViewPending;
        // Completion uses committed choices, not a prospective next visit's
        // choices. Preserve its independent child credits and actor binding.
        let mut committed = |scope| arms(scope, EventArmView::Committed);
        for pending in self.pending_event_steps(row.start()..row.end()) {
            if self.event_lane_at(pending).is_none()
                || self.event_actor_is_local(pending, &mut committed) == Some(false)
                || !self.event_conflict_row_allows(
                    self.machine().event_conflict_for_index(pending),
                    ScopeId::none(),
                    None,
                    &mut committed,
                )
                || self
                    .roll_reentry_preserved_child(scope, pending, Some(index))
                    .is_some()
            {
                continue;
            }
            let (source, direction) =
                crate::invariant_some(self.machine().event_program().operation_at(pending));
            let (carrier, payload) =
                direction.required_views(self.program_ref().event_atom_at(source.dense_ordinal()));
            let uses_view = |role| carrier == role || payload == Some(role);
            if uses_view(role) {
                return EventAdmissionError::PredecessorPending;
            }
            if sent.is_some_and(uses_view)
                || borrowed
                    .is_some_and(|borrowed| borrowed(carrier) || payload.is_some_and(borrowed))
            {
                wait = EventAdmissionError::HeldPredecessorPending;
            }
        }
        let mut selected = |scope| arms(scope, EventArmView::Preview);
        let end = index.min(row.end());
        let mut prior = row.start();
        while prior < end {
            let (before, direction) = self
                .machine()
                .event_program()
                .operation_at(prior)
                .unwrap_or_else(|| crate::invariant());
            let (carrier, payload) =
                direction.required_views(self.program_ref().event_atom_at(before.dense_ordinal()));
            let uses_view = |role| carrier == role || payload == Some(role);
            let uses_caller = uses_view(role);
            if !(uses_caller
                || sent.is_some_and(uses_view)
                || borrowed
                    .is_some_and(|borrowed| borrowed(carrier) || payload.is_some_and(borrowed)))
            {
                prior += 1;
                continue;
            }
            // A fresh visit clears preceding actions on every lane. Waiting
            // cannot release this view to perform its own causal prefix.
            if (self.event_lane_at(prior) == Some(event.lane)
                || self
                    .program_ref()
                    .source_precedes(before.dense_ordinal(), event.eff_index.dense_ordinal()))
                && self.event_actor_is_local(prior, &mut selected) != Some(false)
            {
                match self.event_conflict_row_admission_with_preview(
                    self.machine().event_conflict_for_index(prior),
                    conflict,
                    &mut selected,
                ) {
                    super::super::ConflictAdmission::Allowed => {
                        if uses_caller {
                            return EventAdmissionError::PredecessorPending;
                        }
                        // The carrier check above was false, so the matched
                        // prerequisite is held by this operation's payload or
                        // another exclusively borrowed view.
                        wait = EventAdmissionError::HeldPredecessorPending;
                    }
                    super::super::ConflictAdmission::Excluded { scope, arm } => {
                        prior = self.excluded_arm_end(prior, scope, arm).min(end);
                        continue;
                    }
                    super::super::ConflictAdmission::Invalid => (),
                }
            }
            prior += 1;
        }
        wait
    }

    /// Current pending prefixes and fresh visits need the same receipt
    /// feasibility check. A fresh visit also needs already-completed receipts
    /// inside that roll again. No completion or branch knowledge is changed.
    pub(super) fn retired_receipt_blocks(
        &self,
        index: usize,
        reentry: ScopeId,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let program = self.machine().event_program();
        let start = if reentry.is_none() {
            0
        } else {
            crate::invariant_some(self.machine().roll_scope_row(reentry)).start()
        };
        let mut receipts = (0..program.owned_receipt_count()).filter_map(|receipt| {
            let (step, retirement) = program.owned_receipt(receipt);
            let witness = retirement?;
            let step = step as usize;
            if step < start
                || step >= index
                || (reentry.is_none() && self.local_event_done(step))
                || !self.local_event_done(witness as usize)
            {
                None
            } else {
                Some(step)
            }
        });
        let Some(first) = receipts.next() else {
            return false;
        };
        // Read the target's identity and guard only when an observed receipt
        // can matter, once for the entire query rather than per receipt.
        let (after, _) = crate::invariant_some(program.operation_at(index));
        let conflict = self.machine().event_conflict_for_index(index);
        for step in core::iter::once(first).chain(receipts) {
            let prior = state_index_to_usize(crate::invariant_some(
                self.machine().state_for_step_index(step),
            ));
            let (before, _) = crate::invariant_some(program.operation_at(prior));
            if self
                .program_ref()
                .source_precedes(before.dense_ordinal(), after.dense_ordinal())
                && self.event_actor_is_local(prior, selected) == Some(true)
                && self.event_conflict_row_required_with_preview(
                    self.machine().event_conflict_for_index(prior),
                    conflict,
                    selected,
                )
            {
                return true;
            }
        }
        false
    }

    fn pending_resource_receipt(
        &self,
        receipt: usize,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<(usize, EventCommitMeta)> {
        let step = self.machine().event_program().owned_receipt(receipt).0 as usize;
        let index = state_index_to_usize(crate::invariant_some(
            self.machine().state_for_step_index(step),
        ));
        let meta = crate::invariant_some(self.try_recv_meta_at(index));
        let progress = crate::invariant_ok(
            self.relocatable_resident_lane_step_at_index(index, meta.lane as usize),
        );
        if self.relocatable_step_done(progress)
            || self.event_actor_is_local(index, selected) == Some(false)
        {
            return None;
        }
        let conflict = self.machine().event_conflict_for_index(index);
        if !self.event_conflict_row_allows_with_preview(conflict, conflict, selected) {
            return None;
        }
        let meta = EventCommitMeta::from(meta);
        if matches!(
            self.event_enabled(index, meta, &mut |scope, _view: EventArmView| selected(
                scope
            )),
            Err(EventAdmissionError::Invariant)
        ) {
            return None;
        }
        Some((index, meta))
    }

    /// A later receipt is not needed to restore this resource if another
    /// pending receipt must already return it to this actor first. Source
    /// causality, not catalog order, establishes that boundary.
    fn receipt_has_prior_return(
        &self,
        receipts: PackedLaneRange,
        event: EventCommitMeta,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        for receipt in receipts.start()..receipts.end() {
            let Some((index, prior)) = self.pending_resource_receipt(receipt, selected) else {
                continue;
            };
            if self.event_actor_is_local(index, selected) == Some(true)
                && self.program_ref().source_precedes(
                    prior.eff_index.dense_ordinal(),
                    event.eff_index.dense_ordinal(),
                )
            {
                return true;
            }
        }
        false
    }

    pub(crate) fn resource_receipt_wait(
        &self,
        receipts: PackedLaneRange,
        blocked: (u8, Option<u8>),
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> EventAdmissionError {
        let mut verdict = EventAdmissionError::EntryViewUnavailable;
        for receipt in receipts.start()..receipts.end() {
            if let (_, Some(witness)) = self.machine().event_program().owned_receipt(receipt)
                && self.local_event_done(witness as usize)
            {
                return EventAdmissionError::PredecessorPending;
            }
            let Some((index, meta)) = self.pending_resource_receipt(receipt, selected) else {
                continue;
            };
            let conflict = self.machine().event_conflict_for_index(index);
            let role = crate::invariant_some(self.logical_role_at(index));
            if !self.receipt_has_prior_return(receipts, meta, selected) {
                let wait = if role == blocked.0 {
                    EventAdmissionError::PredecessorPending
                } else {
                    let wait =
                        self.source_predecessor_wait(index, blocked, None, conflict, selected);
                    if wait == EventAdmissionError::OtherViewPending && blocked.1 == Some(role) {
                        EventAdmissionError::HeldPredecessorPending
                    } else {
                        wait
                    }
                };
                if wait == EventAdmissionError::PredecessorPending {
                    return wait;
                }
                if wait == EventAdmissionError::HeldPredecessorPending {
                    verdict = wait;
                    continue;
                }
            }
            // An unknown branch can still require this borrowed view. Do not
            // let an earlier independent alternative hide that self-wait.
            if verdict == EventAdmissionError::EntryViewUnavailable {
                verdict = EventAdmissionError::OtherViewPending;
            }
        }
        verdict
    }
}
