use super::super::RoleLocalDirection;
use super::{PackedLaneRange, RoleImageRef, invalid_resident_descriptor};
use crate::global::compiled::images::PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY;
use crate::global::{
    const_dsl::ReentryMark,
    role_program::ROLE_IMAGE_ROUTE_SCOPE_CAPACITY,
    typestate::{LocalConflict, PackedEventConflict},
};

#[cfg(kani)]
mod kani;

/// Source-checked fields reused by the remaining immutable image checks.
pub(super) struct ValidationRows<const E: usize> {
    pub(super) effects: [u16; E],
    pub(super) keys: [u16; E],
    pub(super) operation_keys: [u32; E],
    pub(super) directions: [RoleLocalDirection; E],
    pub(super) len: usize,
    source_len: usize,
    source_bounds: [u16; PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY + 1],
}

impl<const E: usize> ValidationRows<E> {
    pub(super) const fn new(image: &RoleImageRef) -> Self {
        let count = image.local_step_count();
        if count > E {
            invalid_resident_descriptor();
        }
        let mut rows = Self {
            effects: [0; E],
            keys: [0; E],
            operation_keys: [0; E],
            directions: [RoleLocalDirection::Local; E],
            len: count,
            source_len: image.program.columns.atom_count(),
            source_bounds: [0; PROGRAM_IMAGE_ATOM_ONLY_EVENT_CAPACITY + 1],
        };
        image.validate_actor_event_projection(&mut rows);
        let mut source = 0;
        let mut step = 0;
        while source <= rows.source_len {
            while step < count && (rows.effects[step] as usize) < source {
                step += 1;
            }
            rows.source_bounds[source] = step as u16;
            source += 1;
        }
        rows
    }

    pub(super) const fn bound(&self, source: usize) -> usize {
        if source >= self.source_len {
            return self.len;
        }
        self.source_bounds[source] as usize
    }
}

const fn same_conflict(left: PackedEventConflict, right: PackedEventConflict) -> bool {
    left.with_route_reentry(ReentryMark::SinglePass).raw()
        == right.with_route_reentry(ReentryMark::SinglePass).raw()
}

#[derive(Clone, Copy)]
struct RouteContext {
    scope: crate::global::const_dsl::ScopeId,
    arms: [PackedLaneRange; 2],
    extent: PackedLaneRange,
    parent: PackedEventConflict,
}

impl RouteContext {
    const fn conflict(&self, start: usize, end: usize) -> PackedEventConflict {
        let mut arm = 0;
        while arm < 2 {
            let row = self.arms[arm];
            if !row.is_zero_len() && row.start() <= start && end <= row.end() {
                return PackedEventConflict::route_arm(self.scope, arm as u8);
            }
            arm += 1;
        }
        invalid_resident_descriptor()
    }
}

const fn interval_conflict(
    parent: Option<RouteContext>,
    start: usize,
    end: usize,
) -> PackedEventConflict {
    match parent {
        Some(route) => route.conflict(start, end),
        None => PackedEventConflict::none(),
    }
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
                    // Every chain must actually terminate, including routes
                    // retained only for a post-join actor binding. A self-parent
                    // is malformed metadata, not an absent source ancestor.
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

    /// Check the declared row counts before decoding resident events.
    pub(super) const fn validate_resident_arity(&self) {
        let count = self.local_step_count();
        let scopes = self.footprint().route_scope_count;
        // validate_blob_bound bounds the packed arm span; resident arity
        // below gives two eight-byte rows per local route.
        if scopes > ROLE_IMAGE_ROUTE_SCOPE_CAPACITY
            || self.columns.events.len as usize != count
            || self.columns.lanes.len as usize != count
            || self.columns.route_scopes.len as usize != scopes
            || self.columns.route_scope_conflicts.len as usize != scopes
            || self.columns.route_arms.len as usize != scopes * 2
            || self.columns.route_commit_ranges.len as usize != scopes * 2
        {
            invalid_resident_descriptor();
        }
    }

    pub(super) const fn validate_resident_events<const E: usize>(&self, rows: &ValidationRows<E>) {
        let count = self.local_step_count();
        let scopes = self.footprint().route_scope_count;
        self.validate_roll_projection(rows);
        self.validate_operation_index(rows);
        let lanes = self.lanes();
        let mut slot = 0;
        while slot < scopes {
            let Some(scope) = lanes.route_scope_by_slot(slot) else {
                invalid_resident_descriptor();
            };
            // The exact scope index proves uniqueness without requiring the
            // descriptor's scope identifiers to be sorted by slot.
            match self.route_scope_slot(scope) {
                Some(found) if found == slot => {}
                _ => invalid_resident_descriptor(),
            }
            let _ = lanes.route_scope_reentry_by_slot(slot);
            slot += 1;
        }
        // The mandatory lane_columns_are_coherent pass in new already validates
        // every arm encoding, event bound, zero/step relation and cumulative
        // lane-step prefix. Reuse those proved row
        // bounds instead of re-scanning all preceding packed rows.
        // Route rows occur in source preorder. Walk their nested intervals once
        // to verify every parent and event context, including omitted ancestors.
        let mut current: Option<RouteContext> = None;
        let mut next_scope = 0;
        let mut pending = self.route_context(next_scope);
        let mut idx = 0;
        while idx < count {
            while let Some(route) = current {
                if idx < route.extent.end() {
                    break;
                }
                current = match route.parent.to_conflict() {
                    Some(LocalConflict::RouteArm { scope, .. }) => {
                        let parent = match self.route_scope_slot(scope) {
                            Some(parent) => parent,
                            None => invalid_resident_descriptor(),
                        };
                        match self.route_context(parent) {
                            Some(context) => Some(context),
                            None => invalid_resident_descriptor(),
                        }
                    }
                    _ => None,
                };
            }
            while next_scope < scopes {
                let Some(route) = pending else {
                    next_scope += 1;
                    pending = self.route_context(next_scope);
                    continue;
                };
                if route.extent.start() > idx {
                    break;
                }
                if route.extent.start() < idx {
                    invalid_resident_descriptor();
                }
                let expected = interval_conflict(current, route.extent.start(), route.extent.end());
                if !same_conflict(route.parent, expected) {
                    invalid_resident_descriptor();
                }
                current = Some(route);
                next_scope += 1;
                pending = self.route_context(next_scope);
            }
            let expected = interval_conflict(current, idx, idx + 1);
            if !same_conflict(self.event_conflict_for_index(idx), expected) {
                invalid_resident_descriptor();
            }
            idx += 1;
        }
        while next_scope < scopes {
            if pending.is_some() {
                invalid_resident_descriptor();
            }
            next_scope += 1;
            pending = self.route_context(next_scope);
        }
        let receipts = self.validate_owned_receipts();
        self.validate_roll_entry_receipts(&receipts);
    }

    const fn route_context(&self, slot: usize) -> Option<RouteContext> {
        if slot >= self.footprint().route_scope_count {
            return None;
        }
        let lanes = self.lanes();
        let first = lanes.certified_route_arm_event_row_by_slot(slot, 0);
        let second = lanes.certified_route_arm_event_row_by_slot(slot, 1);
        let extent = if first.is_zero_len() {
            if second.is_zero_len() {
                return None;
            }
            second
        } else if second.is_zero_len() {
            first
        } else {
            if first.end() != second.start() {
                invalid_resident_descriptor();
            }
            PackedLaneRange::new(first.start(), second.end() - first.start())
        };
        let scope = match lanes.route_scope_by_slot(slot) {
            Some(scope) => scope,
            None => invalid_resident_descriptor(),
        };
        Some(RouteContext {
            scope,
            arms: [first, second],
            extent,
            parent: lanes.route_scope_conflict_by_slot(slot),
        })
    }

    const fn validate_owned_receipts(&self) -> [PackedLaneRange; 256] {
        let count = self.columns.owned_receipt_count as usize;
        if self.columns.roll_entry_view_count == 0 && count != 0 {
            invalid_resident_descriptor();
        }
        let mut previous = None;
        let mut counts = [0u16; 256];
        let mut index = 0;
        while index < count {
            let step = self.owned_receipt(index) as usize;
            if step >= self.local_step_count()
                || !matches!(
                    self.local_step_direction(step),
                    super::super::RoleLocalDirection::Recv
                )
            {
                invalid_resident_descriptor();
            }
            let Some(role) = self
                .program
                .event_endpoint(self.local_step_effect(step).dense_ordinal())
            else {
                invalid_resident_descriptor();
            };
            if let Some((prior_role, prior_step)) = previous
                && (role < prior_role || (role == prior_role && step <= prior_step))
            {
                invalid_resident_descriptor();
            }
            previous = Some((role, step));
            counts[role as usize] += 1;
            index += 1;
        }
        if self.columns.roll_entry_view_count != 0 {
            let mut expected = 0;
            let mut step = 0;
            while step < self.local_step_count() {
                if matches!(
                    self.local_step_direction(step),
                    super::super::RoleLocalDirection::Recv
                ) && self
                    .program
                    .event_endpoint(self.local_step_effect(step).dense_ordinal())
                    .is_some()
                {
                    expected += 1;
                }
                step += 1;
            }
            if expected != count {
                invalid_resident_descriptor();
            }
        }
        let mut ranges = [PackedLaneRange::new(0, 0); 256];
        let mut start = 0;
        let mut role = 0;
        while role < ranges.len() {
            ranges[role] = PackedLaneRange::new(start, counts[role] as usize);
            start += counts[role] as usize;
            role += 1;
        }
        if start != count {
            invalid_resident_descriptor();
        }
        ranges
    }

    const fn validate_roll_entry_receipts(&self, catalog: &[PackedLaneRange; 256]) {
        // Resources, owners and roll ranges already match the source. Validate
        // every entry's slice against the independently checked receipt catalog.
        let mut index = 0;
        while index < self.columns.roll_entry_view_count as usize {
            let (role, _) = self.roll_entry_view(index);
            let receipts = self.roll_entry_receipts(index);
            let expected = catalog[role as usize];
            if receipts.start() != expected.start() || receipts.end() != expected.end() {
                invalid_resident_descriptor();
            }
            index += 1;
        }
    }

    const fn validate_actor_event_projection<const E: usize>(&self, rows: &mut ValidationRows<E>) {
        if self.program.actor_decision_count() == 0 {
            // Ordinary identity bindings and unconditional loans need no
            // decision scratch. This specialization is construction-only.
            self.validate_actor_events(rows, &mut []);
        } else {
            const BYTES: usize =
                crate::global::compiled::images::ACTOR_DECISION_CAPACITY.div_ceil(8);
            self.validate_actor_events(rows, &mut [0; BYTES]);
        }
    }

    const fn validate_actor_events<const E: usize>(
        &self,
        rows: &mut ValidationRows<E>,
        scratch: &mut [u8],
    ) {
        let decisions = self.program.actor_decision_count();
        if scratch.len() < decisions.div_ceil(8) {
            invalid_resident_descriptor();
        }
        let mut decision = 0;
        while decision < decisions {
            let Some((_, children)) = self.program.actor_decision_at(decision) else {
                invalid_resident_descriptor();
            };
            let member = actor_reference_is_member(children[0], self.role, scratch, decision)
                || actor_reference_is_member(children[1], self.role, scratch, decision);
            let bit = 1 << (decision % 8);
            scratch[decision / 8] = (scratch[decision / 8] & !bit) | if member { bit } else { 0 };
            decision += 1;
        }
        let mut index = 0;
        let lanes = self.lanes();
        let mut event = 0;
        while event < self.program.columns.atom_count() {
            let (from, to, label) = self.program.event_message(event);
            let [sender, receiver] = match self.program.actor_binding_at(event) {
                Some(binding) => binding,
                None => [from as u16, to as u16],
            };
            if from == to && sender != receiver {
                invalid_resident_descriptor();
            }
            let mut required = actor_operation_mask(
                from,
                to,
                actor_reference_is_member(sender, self.role, scratch, decisions),
                actor_reference_is_member(receiver, self.role, scratch, decisions),
            );
            while required != 0 {
                let direction = match required.trailing_zeros() {
                    0 => RoleLocalDirection::Local,
                    1 => RoleLocalDirection::Send,
                    2 => RoleLocalDirection::Recv,
                    _ => invalid_resident_descriptor(),
                };
                required &= required - 1;
                let Some(actual) = lanes.local_step_event(index) else {
                    invalid_resident_descriptor();
                };
                if actual.eff_index as usize != event || actual.direction() as u8 != direction as u8
                {
                    invalid_resident_descriptor();
                }
                let Some(lane) = self.local_step_lane(index) else {
                    invalid_resident_descriptor();
                };
                if lane != self.program.event_lane(event) {
                    invalid_resident_descriptor();
                }
                let _ = lanes.event_conflict_for_index(index);
                let _ = lanes.dependency_for_index(index);
                let view = if matches!(direction, RoleLocalDirection::Recv) {
                    to
                } else {
                    from
                };
                rows.effects[index] = actual.eff_index;
                rows.keys[index] = (view as u16) << 8 | lane as u16;
                rows.operation_keys[index] = direction.operation_index_key(from, to, label);
                rows.directions[index] = direction;
                index += 1;
            }
            event += 1;
        }
        if index != self.local_step_count() {
            invalid_resident_descriptor();
        }
    }
}

// Decisions have already been checked as a backwards-pointing DAG. The
// constructor derives membership independently of the projection iterator.
const fn actor_reference_is_member(
    reference: u16,
    actor: u8,
    membership: &[u8],
    checked: usize,
) -> bool {
    if reference < 256 {
        reference == actor as u16
    } else {
        let decision = reference as usize - 256;
        if decision >= checked || decision / 8 >= membership.len() {
            invalid_resident_descriptor();
        }
        membership[decision / 8] & (1 << (decision % 8)) != 0
    }
}

const fn actor_operation_mask(from: u8, to: u8, sender: bool, receiver: bool) -> u8 {
    if from == to {
        sender as u8
    } else {
        ((sender as u8) << 1) | ((receiver as u8) << 2)
    }
}
