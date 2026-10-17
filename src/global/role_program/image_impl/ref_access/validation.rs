use super::super::projection::ActorEventProjection;
use super::{RoleImageRef, invalid_resident_descriptor};
use crate::global::typestate::LocalConflict;

impl RoleImageRef {
    /// Establish all immutable checks that receive metadata performs before
    /// inspecting live progress. Raw lane views remain checked independently;
    /// only this opaque, fully constructed image permits direct authority reads.
    pub(super) const fn validate_resident_events(&self) {
        let count = self.local_step_count();
        let scopes = self.footprint().route_scope_count;
        if self.columns.events.len as usize != count
            || self.columns.lanes.len as usize != count
            || self.columns.route_scopes.len as usize != scopes
            || self.columns.route_scope_conflicts.len as usize != scopes
            || self.columns.route_arms.len as usize != scopes * 2
            || self.columns.route_commit_ranges.len as usize != scopes * 2
        {
            invalid_resident_descriptor();
        }
        self.validate_actor_event_projection();
        self.validate_owned_receipts();
        self.validate_roll_entry_views();
        let lanes = self.lanes();
        let mut slot = 0;
        while slot < scopes {
            let Some(scope) = lanes.route_scope_by_slot(slot) else {
                invalid_resident_descriptor();
            };
            // Reuse the constructor's existing sorted-ID proof when available.
            // Unsorted images retain the same checked uniqueness scan.
            match self.route_scope_slot(scope) {
                Some(found) if found == slot => {}
                _ => invalid_resident_descriptor(),
            }
            let _ = lanes.route_scope_reentry_by_slot(slot);
            slot += 1;
        }
        // The mandatory lane_columns_are_coherent pass in new already validates
        // every arm encoding, event bound, zero/step relation and cumulative
        // lane-step prefix, including unsorted images. Reuse those proved row
        // bounds instead of re-scanning all preceding packed rows. Scope
        // authority and exact event membership checks remain below.
        let mut idx = 0;
        while idx < count {
            // Includes send/local rows, which try_recv_meta decodes before
            // rejecting their direction. This checks program atom existence,
            // event header, role direction, conflict and atom/resident lane.
            let _ = self.local_step_node(idx);
            // event_row_at, reached by receive route authority, checks this
            // dependency even when the operation will not match a carried key.
            let _ = self.dependency_for_index(idx);
            let conflict = self.event_conflict_for_index(idx);
            match conflict.to_conflict() {
                Some(LocalConflict::RouteArm { scope, arm }) => {
                    let Some(slot) = self.route_scope_slot(scope) else {
                        invalid_resident_descriptor();
                    };
                    let first = lanes.certified_route_arm_event_row_by_slot(slot, 0);
                    let second = lanes.certified_route_arm_event_row_by_slot(slot, 1);
                    // Preserve the legacy arm0-first membership rule, including
                    // any accepted overlapping representation.
                    let actual = if first.start() <= idx && idx < first.end() {
                        0
                    } else if second.start() <= idx && idx < second.end() {
                        1
                    } else {
                        invalid_resident_descriptor();
                    };
                    if actual != arm {
                        invalid_resident_descriptor();
                    }
                    // Membership in an arm also proves membership in the
                    // nonempty union envelope used by enclosing_route_scope.
                }
                Some(LocalConflict::Unconditional | LocalConflict::SharedRoute) | None => {
                    let mut slot = 0;
                    while slot < scopes {
                        let first = lanes.certified_route_arm_event_row_by_slot(slot, 0);
                        let second = lanes.certified_route_arm_event_row_by_slot(slot, 1);
                        let (start, end) = if first.is_zero_len() {
                            (second.start(), second.end())
                        } else if second.is_zero_len() {
                            (first.start(), first.end())
                        } else {
                            (
                                if first.start() < second.start() {
                                    first.start()
                                } else {
                                    second.start()
                                },
                                if first.end() > second.end() {
                                    first.end()
                                } else {
                                    second.end()
                                },
                            )
                        };
                        if start <= idx && idx < end {
                            invalid_resident_descriptor();
                        }
                        slot += 1;
                    }
                }
            }
            idx += 1;
        }
    }

    const fn validate_owned_receipts(&self) {
        let count = self.columns.owned_receipt_count as usize;
        if self.columns.roll_entry_view_count == 0 && count != 0 {
            invalid_resident_descriptor();
        }
        let mut previous = None;
        let mut index = 0;
        while index < count {
            let step = self.owned_receipt(index) as usize;
            let node = match self.local_step_node(step) {
                Some(node) => node,
                None => invalid_resident_descriptor(),
            };
            let crate::global::typestate::LocalAction::Recv { eff_index, .. } = node.action()
            else {
                invalid_resident_descriptor();
            };
            let crate::eff::EventPayload::Endpoint(role) = self
                .program
                .event_atom_at(eff_index.dense_ordinal())
                .payload
            else {
                invalid_resident_descriptor();
            };
            if let Some((prior_role, prior_step)) = previous
                && (role < prior_role || (role == prior_role && step <= prior_step))
            {
                invalid_resident_descriptor();
            }
            previous = Some((role, step));
            index += 1;
        }
        if self.columns.roll_entry_view_count != 0 {
            let mut expected = 0;
            let mut step = 0;
            while step < self.local_step_count() {
                let node = match self.local_step_node(step) {
                    Some(node) => node,
                    None => invalid_resident_descriptor(),
                };
                if let crate::global::typestate::LocalAction::Recv { eff_index, .. } = node.action()
                    && matches!(
                        self.program
                            .event_atom_at(eff_index.dense_ordinal())
                            .payload,
                        crate::eff::EventPayload::Endpoint(_)
                    )
                {
                    expected += 1;
                }
                step += 1;
            }
            if expected != count {
                invalid_resident_descriptor();
            }
        }
    }

    const fn validate_roll_entry_views(&self) {
        let columns = self.columns;
        if columns.roll_entry_view_count == 0 {
            return;
        }
        if columns.roll_entry_ranges().len != columns.roll_scopes.len {
            invalid_resident_descriptor();
        }
        let mut expected = 0;
        let mut slot = 0;
        while slot < columns.roll_scopes.len as usize {
            let range = self.roll_entry_range(slot);
            if range.start() != expected {
                invalid_resident_descriptor();
            }
            let mut previous = None;
            let mut index = range.start();
            while index < range.end() {
                let (role, owner) = self.roll_entry_view(index);
                if role > self.program.facts.max_role {
                    invalid_resident_descriptor();
                }
                if let Some(prior) = previous
                    && role <= prior
                {
                    invalid_resident_descriptor();
                }
                if (owner < 256 && owner > self.program.facts.max_role as u16)
                    || (owner >= 256
                        && self
                            .program
                            .actor_decision_at(owner as usize - 256)
                            .is_none())
                {
                    invalid_resident_descriptor();
                }
                let receipts = self.roll_entry_receipts(index);
                let mut receipt = receipts.start();
                let mut step = 0;
                while step < self.local_step_count() {
                    let node = match self.local_step_node(step) {
                        Some(node) => node,
                        None => invalid_resident_descriptor(),
                    };
                    if let crate::global::typestate::LocalAction::Recv { eff_index, .. } =
                        node.action()
                        && matches!(self.program.event_atom_at(eff_index.dense_ordinal()).payload,
                            crate::eff::EventPayload::Endpoint(key) if key == role)
                    {
                        if receipt == receipts.end() || self.owned_receipt(receipt) as usize != step
                        {
                            invalid_resident_descriptor();
                        }
                        receipt += 1;
                    }
                    step += 1;
                }
                if receipt != receipts.end() {
                    invalid_resident_descriptor();
                }
                let mut found = false;
                let mut event = 0;
                while event < self.program.columns.atom_count() {
                    if let crate::eff::EventPayload::Endpoint(key) =
                        self.program.event_atom_at(event).payload
                        && key == role
                    {
                        found = true;
                        break;
                    }
                    event += 1;
                }
                if !found {
                    invalid_resident_descriptor();
                }
                previous = Some(role);
                index += 1;
            }
            expected = range.end();
            slot += 1;
        }
        if expected != columns.roll_entry_view_count as usize {
            invalid_resident_descriptor();
        }
    }

    const fn validate_actor_event_projection(&self) {
        if self.program.actor_decision_count() == 0 {
            // Ordinary identity bindings and unconditional loans need no
            // decision scratch. This specialization is construction-only.
            self.validate_actor_events(&mut []);
        } else {
            const BYTES: usize =
                crate::global::compiled::images::ACTOR_DECISION_CAPACITY.div_ceil(8);
            self.validate_actor_events(&mut [0; BYTES]);
        }
    }

    const fn validate_actor_events(&self, scratch: &mut [u8]) {
        let Some(mut projected) = ActorEventProjection::new(self.program, self.role, scratch)
        else {
            invalid_resident_descriptor();
        };
        let mut index = 0;
        while let Some(expected) = projected.next() {
            let Some(actual) = self.lanes().local_step_event(index) else {
                invalid_resident_descriptor();
            };
            if actual.eff_index as usize != expected.event
                || actual.direction() as u8 != expected.direction as u8
            {
                invalid_resident_descriptor();
            }
            index += 1;
        }
        if index != self.local_step_count() {
            invalid_resident_descriptor();
        }
    }
}
