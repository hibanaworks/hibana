//! Re-derive entry obligations from checked global atoms and local membership.
//! Missing rows cannot disable the check. Nothing survives as runtime state.
use super::{RoleImageRef, invalid_resident_descriptor, validation::ValidationRows};

impl RoleImageRef {
    pub(super) const fn validate_roll_projection<const E: usize>(&self, rows: &ValidationRows<E>) {
        let owned = self.program.actor_binding_at(0).is_some();
        if !owned && self.columns.roll_entry_view_count != 0 {
            invalid_resident_descriptor();
        }
        let mut resources = [0u8; 32];
        let mut event = 0;
        while owned && event < self.program.columns.atom_count() {
            if let Some(role) = self.program.event_endpoint(event) {
                resources[role as usize / 8] |= 1 << (role % 8);
            }
            event += 1;
        }
        let mut expected = 0;
        let mut slot = 0;
        let mut marker = 0;
        let mut previous = None;
        while marker < self.program.source_scope_marker_count() {
            let Some((scope, start, end)) = self.program.source_roll_at(marker) else {
                marker += 1;
                continue;
            };
            marker += 1;
            let first = rows.bound(start);
            let finish = rows.bound(end);
            if first == finish {
                continue;
            }
            let row = match self.roll_scope_row(slot) {
                Some(row) => row,
                None => invalid_resident_descriptor(),
            };
            if !row.scope().same(scope)
                || row.event_row().start() != first
                || row.event_row().end() != finish
            {
                invalid_resident_descriptor();
            }
            if let Some(prior) = previous
                && prior >= scope.local_ordinal()
            {
                invalid_resident_descriptor();
            }
            previous = Some(scope.local_ordinal());
            slot += 1;
            if !owned {
                continue;
            }
            let range = self.roll_entry_range(slot - 1);
            if range.start() != expected {
                invalid_resident_descriptor();
            }
            let mut resource = 0;
            while resource <= self.program.facts.max_role as usize {
                if resources[resource / 8] & (1 << (resource % 8)) != 0
                    && let Some(owner) = self.source_entry_owner(rows, start, end, resource as u8)
                {
                    if expected >= range.end() {
                        invalid_resident_descriptor();
                    }
                    let (actual_resource, actual_owner) = self.roll_entry_view(expected);
                    if actual_resource != resource as u8 || actual_owner != owner {
                        invalid_resident_descriptor();
                    }
                    expected += 1;
                }
                resource += 1;
            }
            if expected != range.end() {
                invalid_resident_descriptor();
            }
        }
        if slot != self.columns.roll_scopes.len as usize
            || expected != self.columns.roll_entry_view_count as usize
        {
            invalid_resident_descriptor();
        }
    }

    const fn source_entry_owner<const E: usize>(
        &self,
        rows: &ValidationRows<E>,
        start: usize,
        end: usize,
        resource: u8,
    ) -> Option<u16> {
        let mut event = start;
        while event < end {
            let (from, to) = self.program.event_roles(event);
            let side = if from == resource
                || matches!(self.program.event_endpoint(event), Some(role) if role == resource)
            {
                0
            } else if to == resource {
                1
            } else {
                event += 1;
                continue;
            };
            let bindings = match self.program.actor_binding_at(event) {
                Some(bindings) => bindings,
                None => invalid_resident_descriptor(),
            };
            let direction = if from == to { 0 } else { side + 1 };
            let mut local = rows.bound(event);
            while local < rows.len && rows.effects[local] as usize == event {
                if rows.directions[local] as usize == direction {
                    return Some(bindings[side]);
                }
                local += 1;
            }
            return None;
        }
        None
    }
}
