use super::super::projection::ActorEventProjection;
use super::{PackedLaneRange, RoleImageRef, invalid_resident_descriptor};
use crate::global::{
    const_dsl::ReentryMark,
    typestate::{LocalConflict, PackedEventConflict},
};

const fn same_conflict(left: PackedEventConflict, right: PackedEventConflict) -> bool {
    left.with_route_reentry(ReentryMark::SinglePass).raw()
        == right.with_route_reentry(ReentryMark::SinglePass).raw()
}

impl RoleImageRef {
    pub(super) const fn validate_route_commit_chains(&self) {
        let lanes = self.lanes();
        let mut slot = 0;
        while slot < self.footprint().route_scope_count {
            let Some(scope) = lanes.route_scope_by_slot(slot) else {
                invalid_resident_descriptor();
            };
            let mut arm = 0;
            while arm < 2 {
                let row = lanes.certified_route_commit_range_by_slot(slot, arm);
                let mut expected = PackedEventConflict::route_arm(scope, arm);
                let mut pos = row.end();
                while pos > row.start() {
                    pos -= 1;
                    let current = lanes.route_commit_row_at(pos);
                    if !super::super::lane_image::route_commit_decisions_match(current, expected) {
                        invalid_resident_descriptor();
                    }
                    let Some(LocalConflict::RouteArm { scope, .. }) = current.to_conflict() else {
                        invalid_resident_descriptor();
                    };
                    let Some(parent_slot) = self.route_scope_slot(scope) else {
                        invalid_resident_descriptor();
                    };
                    let parent = lanes.route_scope_conflict_by_slot(parent_slot);
                    let parent = match parent.to_conflict() {
                        Some(LocalConflict::RouteArm { scope: parent, .. })
                            if parent.same(scope) =>
                        {
                            PackedEventConflict::none()
                        }
                        _ => parent,
                    };
                    if (pos == row.start()) != parent.is_none() {
                        invalid_resident_descriptor();
                    }
                    expected = parent;
                }
                arm += 1;
            }
            slot += 1;
        }
    }

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
        self.validate_roll_projection();
        self.validate_owned_receipts();
        self.validate_roll_entry_receipts();
        self.validate_operation_index();
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
        // bounds instead of re-scanning all preceding packed rows.
        // Route rows occur in source preorder. Walk their nested intervals once
        // to verify every parent and event context, including omitted ancestors.
        let mut current = None;
        let mut next_scope = 0;
        let mut idx = 0;
        while idx < count {
            // Includes send/local rows, which try_recv_meta decodes before
            // rejecting their direction. This checks program atom existence,
            // event header, role direction, conflict and atom/resident lane.
            let _ = self.local_step_node(idx);
            // event_row_at, reached by receive route authority, checks this
            // dependency even when the operation will not match a carried key.
            let _ = lanes.dependency_for_index(idx);
            while let Some(slot) = current {
                let Some(extent) = self.route_extent(slot) else {
                    invalid_resident_descriptor();
                };
                if idx < extent.end() {
                    break;
                }
                current = match lanes.route_scope_conflict_by_slot(slot).to_conflict() {
                    Some(LocalConflict::RouteArm { scope, .. }) => {
                        match self.route_scope_slot(scope) {
                            Some(parent) => Some(parent),
                            None => invalid_resident_descriptor(),
                        }
                    }
                    _ => None,
                };
            }
            while next_scope < scopes {
                let Some(extent) = self.route_extent(next_scope) else {
                    next_scope += 1;
                    continue;
                };
                if extent.start() > idx {
                    break;
                }
                if extent.start() < idx {
                    invalid_resident_descriptor();
                }
                let expected = self.interval_conflict(current, extent.start(), extent.end());
                let actual = lanes.route_scope_conflict_by_slot(next_scope);
                if !same_conflict(actual, expected) {
                    invalid_resident_descriptor();
                }
                current = Some(next_scope);
                next_scope += 1;
            }
            let expected = self.interval_conflict(current, idx, idx + 1);
            if !same_conflict(self.event_conflict_for_index(idx), expected) {
                invalid_resident_descriptor();
            }
            idx += 1;
        }
        while next_scope < scopes {
            if self.route_extent(next_scope).is_some() {
                invalid_resident_descriptor();
            }
            next_scope += 1;
        }
    }

    const fn route_extent(&self, slot: usize) -> Option<PackedLaneRange> {
        let lanes = self.lanes();
        let first = lanes.certified_route_arm_event_row_by_slot(slot, 0);
        let second = lanes.certified_route_arm_event_row_by_slot(slot, 1);
        if first.is_zero_len() {
            return if second.is_zero_len() {
                None
            } else {
                Some(second)
            };
        }
        if second.is_zero_len() {
            return Some(first);
        }
        if first.end() != second.start() {
            invalid_resident_descriptor();
        }
        Some(PackedLaneRange::new(
            first.start(),
            second.end() - first.start(),
        ))
    }

    const fn interval_conflict(
        &self,
        parent: Option<usize>,
        start: usize,
        end: usize,
    ) -> PackedEventConflict {
        let Some(slot) = parent else {
            return PackedEventConflict::none();
        };
        let lanes = self.lanes();
        let scope = match lanes.route_scope_by_slot(slot) {
            Some(scope) => scope,
            None => invalid_resident_descriptor(),
        };
        let mut arm = 0;
        while arm < 2 {
            let row = lanes.certified_route_arm_event_row_by_slot(slot, arm);
            if !row.is_zero_len() && row.start() <= start && end <= row.end() {
                return PackedEventConflict::route_arm(scope, arm);
            }
            arm += 1;
        }
        invalid_resident_descriptor()
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

    const fn validate_roll_entry_receipts(&self) {
        // Resources, owners and roll ranges already match the source. Validate
        // every entry's slice against the independently checked receipt catalog.
        let mut index = 0;
        while index < self.columns.roll_entry_view_count as usize {
            let (role, _) = self.roll_entry_view(index);
            let receipts = self.roll_entry_receipts(index);
            let mut receipt = receipts.start();
            let mut step = 0;
            while step < self.local_step_count() {
                let node = match self.local_step_node(step) {
                    Some(node) => node,
                    None => invalid_resident_descriptor(),
                };
                if let crate::global::typestate::LocalAction::Recv { eff_index, .. } = node.action()
                    && matches!(self.program.event_atom_at(eff_index.dense_ordinal()).payload,
                        crate::eff::EventPayload::Endpoint(key) if key == role)
                {
                    if receipt == receipts.end() || self.owned_receipt(receipt) as usize != step {
                        invalid_resident_descriptor();
                    }
                    receipt += 1;
                }
                step += 1;
            }
            if receipt != receipts.end() {
                invalid_resident_descriptor();
            }
            index += 1;
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
