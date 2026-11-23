//! Read-only recovery admission over the projected resource receipt index.
use super::{EventAdmissionError, EventArmView, EventCursor, OperationBorrow, ScopeId};
use crate::global::{
    role_program::{PackedLaneRange, RoleLocalDirection},
    typestate::{EventCommitMeta, PackedEventConflict, state_index_to_usize},
};

impl EventCursor {
    /// An alternative at the head of a future visit may be waiting on another
    /// view to finish the current visit. This query cannot admit or reset it.
    pub(super) fn alternate_entry_waits_for_view(
        &self,
        index: usize,
        event: EventCommitMeta,
        borrowed: OperationBorrow<'_>,
        committed: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        let conflict = self.event_conflict_for_index(index);
        let super::super::ConflictAdmission::Excluded { scope: route, .. } =
            self.event_conflict_row_admission(conflict, ScopeId::none(), None, committed)
        else {
            return false;
        };
        self.machine()
            .event_program()
            .roll_ancestors(index)
            .any(|body| {
                if !self.route_scope_contained_in_roll_scope(route, body)
                    || self.roll_scope_events_complete_at(body, Some(index), committed)
                {
                    return false;
                }
                let mut choices = |scope, view| {
                    if matches!(view, EventArmView::Preview)
                        && self.route_scope_contained_in_roll_scope(scope, body)
                        && !self.roll_reentry_preserves_route(body, scope, index)
                    {
                        self.preview_conflict_arm(conflict, scope)
                    } else {
                        committed(scope)
                    }
                };
                let mut fresh = |scope| choices(scope, EventArmView::Preview);
                self.event_conflict_row_allows_with_preview(conflict, conflict, &mut fresh)
                    && self.event_actor_is_local(index, &mut fresh) == Some(true)
                    && self.roll_scope_head_allows_index(body, index, event.lane, &mut fresh)
                    && self.reentry_view_wait(
                        index,
                        EventCommitMeta {
                            body_reentry: body,
                            ..event
                        },
                        borrowed,
                        &mut choices,
                    ) == EventAdmissionError::OtherViewPending
            })
    }

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
        borrowed: OperationBorrow<'_>,
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
        let conflict = self.machine().event_conflict_for_index(index);
        super::wait_across_choices(self.program_ref().source_scope_count(), &mut |unknown| {
            self.reentry_wait_on_path(index, event, scope, borrowed, &mut |route, view| {
                let preview = matches!(view, EventArmView::Preview);
                arms(route, view).or_else(|| {
                    let renewed = self.route_scope_contained_in_roll_scope(route, scope)
                        && !self.roll_reentry_preserves_route(scope, route, index);
                    // The candidate implies its own guard only in its visit.
                    // An enclosing or preserved choice is shared by both.
                    if (preview || !renewed)
                        && let Some(arm) = self.preview_conflict_arm(conflict, route)
                    {
                        return Some(arm);
                    }
                    unknown(route, preview && renewed)
                })
            })
        })
    }

    /// A hypothetical visit reads enclosing choices first. An excluded
    /// outer arm then excludes its whole subtree without assigning any of
    /// its inner choices. Only guards from the sealed operation row are used.
    fn visit_excludes_row(
        &self,
        guard: PackedEventConflict,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        if let Some(chain) = self.route_commit_range_for_conflict(guard) {
            return self.route_commit_rows(chain).any(|row| {
                let Some(crate::global::typestate::LocalConflict::RouteArm { scope, arm }) =
                    row.to_conflict()
                else {
                    crate::invariant()
                };
                selected(scope).is_some_and(|choice| choice != arm)
            });
        }
        !self.event_conflict_row_allows(guard, ScopeId::none(), None, selected)
    }

    fn reentry_wait_on_path(
        &self,
        index: usize,
        event: EventCommitMeta,
        scope: ScopeId,
        borrowed: OperationBorrow<'_>,
        arms: &mut dyn FnMut(ScopeId, EventArmView) -> Option<u8>,
    ) -> EventAdmissionError {
        let (source, direction) =
            crate::invariant_some(self.machine().event_program().operation_at(index));
        let (role, sent) =
            direction.required_views(self.program_ref().event_atom_at(source.dense_ordinal()));
        let Some(row) = self.machine().roll_scope_row(scope) else {
            crate::invariant();
        };
        let conflict = self.machine().event_conflict_for_index(index);
        let mut selected = |scope| arms(scope, EventArmView::Preview);
        if !self.event_conflict_row_allows_with_preview(conflict, conflict, &mut selected)
            || self.event_actor_is_local(index, &mut selected) == Some(false)
            || self.retired_receipt_blocks(index, scope, &mut selected)
        {
            return EventAdmissionError::PredecessorPending;
        }
        let mut wait = EventAdmissionError::OtherViewPending;
        // Completion uses committed choices, not a prospective next visit's
        // choices. Preserve its independent child credits and actor binding.
        let mut committed = |scope| arms(scope, EventArmView::Committed);
        for pending in self.pending_event_steps(row.start()..row.end()) {
            let (source, direction) =
                crate::invariant_some(self.machine().event_program().operation_at(pending));
            let (carrier, payload) =
                direction.required_views(self.program_ref().event_atom_at(source.dense_ordinal()));
            let uses_view = |role| carrier == role || payload == Some(role);
            let own = payload == Some(role)
                || (carrier == role
                    && borrowed.holds_carrier(self.program_ref(), source.dense_ordinal()));
            let held = sent.is_some_and(uses_view)
                || borrowed
                    .peers
                    .is_some_and(|borrowed| borrowed(carrier) || payload.is_some_and(borrowed));
            if !own && (wait == EventAdmissionError::HeldPredecessorPending || !held) {
                continue;
            }
            if self.event_lane_at(pending).is_none()
                || self.visit_excludes_row(
                    self.machine().event_conflict_for_index(pending),
                    &mut committed,
                )
                || self.event_actor_is_local(pending, &mut committed) == Some(false)
                || self
                    .roll_reentry_preserved_child(scope, pending, Some(index))
                    .is_some()
            {
                continue;
            }
            if own {
                return EventAdmissionError::PredecessorPending;
            }
            wait = EventAdmissionError::HeldPredecessorPending;
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
            let uses_caller = payload == Some(role)
                || (carrier == role
                    && borrowed.holds_carrier(self.program_ref(), before.dense_ordinal()));
            if !uses_caller
                && (wait == EventAdmissionError::HeldPredecessorPending
                    || !(sent.is_some_and(uses_view)
                        || borrowed.peers.is_some_and(|borrowed| {
                            borrowed(carrier) || payload.is_some_and(borrowed)
                        })))
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
                && !self.visit_excludes_row(
                    self.machine().event_conflict_for_index(prior),
                    &mut |scope| {
                        selected(scope).or_else(|| self.preview_conflict_arm(conflict, scope))
                    },
                )
                && self.event_actor_is_local(prior, &mut selected) != Some(false)
            {
                if uses_caller {
                    return EventAdmissionError::PredecessorPending;
                }
                wait = EventAdmissionError::HeldPredecessorPending;
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
        current: usize,
        event: EventCommitMeta,
        selected: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> bool {
        for receipt in receipts.start()..receipts.end() {
            if receipt == current {
                continue;
            }
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
        borrowed: OperationBorrow<'_>,
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
            if !self.receipt_has_prior_return(receipts, receipt, meta, selected) {
                let wait = if role == blocked.0
                    && borrowed.holds_carrier(self.program_ref(), meta.eff_index.dense_ordinal())
                {
                    EventAdmissionError::PredecessorPending
                } else {
                    let wait =
                        self.source_predecessor_wait(index, blocked, borrowed, conflict, selected);
                    if wait == EventAdmissionError::OtherViewPending
                        && (blocked.1 == Some(role)
                            || borrowed.peers.is_some_and(|held| held(role)))
                    {
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

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn excluded_parent_prunes_unvisited_child_for_every_partial_choice() {
    use crate::global::typestate::{LocalConflict, cursor::validated_metadata_tests::with_rows};
    use crate::{g, runtime::program::project};

    let global = g::route(
        g::route(
            g::send::<0, 1, g::Msg<1, ()>>(),
            g::send::<0, 1, g::Msg<2, ()>>(),
        ),
        g::send::<0, 1, g::Msg<3, ()>>(),
    )
    .roll();
    with_rows(project::<0>(&global).role_image_ref(), |cursor| {
        let row = cursor.event_conflict_for_index(0);
        let chain = cursor.route_commit_range_for_conflict(row).unwrap();
        let mut guards = cursor.route_commit_rows(chain);
        let Some(LocalConflict::RouteArm {
            scope: parent,
            arm: parent_arm,
        }) = guards.next().unwrap().to_conflict()
        else {
            panic!("missing outer guard")
        };
        let Some(LocalConflict::RouteArm { scope: child, .. }) =
            guards.next().unwrap().to_conflict()
        else {
            panic!("missing inner guard")
        };
        assert_ne!(parent, child);
        assert!(guards.next().is_none());
        for outer in [None, Some(0), Some(1)] {
            for inner in [None, Some(0), Some(1)] {
                let mut reads = std::vec::Vec::new();
                let mut selected = |scope| if scope == parent { outer } else { inner };
                let excluded = cursor.visit_excludes_row(row, &mut |scope| {
                    reads.push(scope);
                    selected(scope)
                });
                assert_eq!(
                    excluded,
                    !cursor.event_conflict_row_allows(row, ScopeId::none(), None, &mut selected,)
                );
                if outer.is_some_and(|arm| arm != parent_arm) {
                    assert_eq!(reads, [parent]);
                } else {
                    assert_eq!(reads, [parent, child]);
                }
            }
        }
    });
}
