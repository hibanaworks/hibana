//! Immutable ownership facts in the same compact program image.
//! Construction checks references and issuance rows once. These tables do not
//! authorize an operation independently of actor-local progress.
use super::CompiledProgramRef;
use crate::{
    eff::EventPayload,
    global::{
        compiled::images::image::columns::{
            OwnershipColumns, PROGRAM_IMAGE_ACTOR_BINDING_STRIDE,
            PROGRAM_IMAGE_ACTOR_DECISION_STRIDE, PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE,
            PROGRAM_IMAGE_SCOPE_MARKER_STRIDE,
        },
        const_dsl::{ScopeId, ScopeKind},
    },
};

#[cfg(any(kani, all(test, hibana_repo_tests)))]
mod fixtures;
#[cfg(kani)]
mod kani;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

const fn valid_reference(reference: u16, decisions: usize, max_role: u8) -> bool {
    if reference < 256 {
        reference <= max_role as u16
    } else {
        reference != u16::MAX && reference as usize - 256 < decisions
    }
}

impl CompiledProgramRef {
    pub(crate) const fn actor_decision_count(&self) -> usize {
        match self.ownership_columns() {
            Some(columns) => columns.decisions.len as usize,
            None => 0,
        }
    }

    const fn ownership_columns(&self) -> Option<OwnershipColumns> {
        if !self.columns.has_ownership() {
            return None;
        }
        let header = self.columns.ownership_header().offset as usize;
        match self.columns.ownership_columns(
            self.read_u16_at(header) as usize,
            self.read_u16_at(header + 2) as usize,
        ) {
            Some(columns) => Some(columns),
            None => crate::invariant(),
        }
    }

    pub(crate) const fn source_scope_marker_count(&self) -> usize {
        if self.columns.has_ownership() {
            self.read_u16_at(self.columns.ownership_header().offset as usize) as usize
        } else {
            self.columns.scope_marker_count()
        }
    }

    pub(crate) const fn source_parallel_at(&self, marker: usize) -> Option<(usize, usize, usize)> {
        let markers = self
            .columns
            .scope_markers_for(self.source_scope_marker_count());
        if marker >= markers.len as usize {
            return None;
        }
        let offset = markers.offset as usize + marker * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
        let scope = match ScopeId::decode_raw(self.read_u16_at(offset + 2)) {
            Some(scope) => scope,
            None => crate::invariant(),
        };
        if !matches!(scope.kind(), Some(ScopeKind::Parallel)) || self.byte_at(offset + 4) & 3 != 0 {
            return None;
        }
        let start = self.read_u16_at(offset) as usize;
        let mut split = None;
        let mut index = marker + 1;
        while index < markers.len as usize {
            let at = markers.offset as usize + index * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
            if self.read_u16_at(at + 2) == scope.raw() {
                match self.byte_at(at + 4) & 3 {
                    1 => split = Some(self.read_u16_at(at) as usize),
                    2 => {
                        let end = self.read_u16_at(at) as usize;
                        let Some(split) = split else {
                            crate::invariant();
                        };
                        if start >= split || split >= end || end > self.columns.atom_count() {
                            crate::invariant();
                        }
                        return Some((start, split, end));
                    }
                    _ => (),
                }
            }
            index += 1;
        }
        crate::invariant()
    }

    /// Strict single-visit source order. Enumeration orders sequential terms,
    /// except for operations separated by parallel or alternative arms.
    /// Read the existing immutable markers; do not create a dependency ledger.
    pub(crate) const fn source_precedes(&self, before: usize, after: usize) -> bool {
        if before >= after || after >= self.columns.atom_count() {
            return false;
        }
        let markers = self
            .columns
            .scope_markers_for(self.source_scope_marker_count());
        let mut marker = 0;
        while marker < markers.len as usize {
            let offset = markers.offset as usize + marker * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
            let split = self.read_u16_at(offset) as usize;
            if before < split && split <= after {
                let raw_scope = self.read_u16_at(offset + 2);
                let Some(scope) = ScopeId::decode_raw(raw_scope) else {
                    crate::invariant();
                };
                let tag = self.byte_at(offset + 4) & 3;
                // Par has a Split marker. Route closes its left arm and
                // enters its right arm at the same offset instead.
                if matches!(
                    (scope.kind(), tag),
                    (Some(ScopeKind::Parallel), 1) | (Some(ScopeKind::Route), 2)
                ) {
                    let mut start = None;
                    let mut stop = None;
                    let mut bound = 0;
                    while bound < markers.len as usize {
                        let at =
                            markers.offset as usize + bound * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
                        if self.read_u16_at(at + 2) == raw_scope {
                            match self.byte_at(at + 4) & 3 {
                                0 if start.is_none() => start = Some(self.read_u16_at(at) as usize),
                                2 => stop = Some(self.read_u16_at(at) as usize),
                                _ => (),
                            }
                        }
                        bound += 1;
                    }
                    let (Some(start), Some(stop)) = (start, stop) else {
                        crate::invariant();
                    };
                    if start <= before && after < stop {
                        return false;
                    }
                }
            }
            marker += 1;
        }
        true
    }

    /// Read a roll enter at this source marker and its global atom interval.
    /// A local table cannot hide a source scope by omitting its own roll row.
    pub(crate) const fn source_roll_at(&self, marker: usize) -> Option<(ScopeId, usize, usize)> {
        let markers = self
            .columns
            .scope_markers_for(self.source_scope_marker_count());
        if marker >= markers.len as usize {
            return None;
        }
        let offset = markers.offset as usize + marker * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
        let scope = match ScopeId::decode_raw(self.read_u16_at(offset + 2)) {
            Some(scope) => scope,
            None => crate::invariant(),
        };
        if !matches!(scope.kind(), Some(ScopeKind::Roll)) || self.byte_at(offset + 4) & 3 != 0 {
            return None;
        }
        let start = self.read_u16_at(offset) as usize;
        let mut index = marker + 1;
        while index < markers.len as usize {
            let offset = markers.offset as usize + index * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
            if self.read_u16_at(offset + 2) == scope.raw() && self.byte_at(offset + 4) & 3 == 2 {
                let end = self.read_u16_at(offset) as usize;
                if start >= end || end > self.columns.atom_count() {
                    crate::invariant();
                }
                return Some((scope, start, end));
            }
            index += 1;
        }
        crate::invariant()
    }

    pub(crate) const fn actor_binding_at(&self, event: usize) -> Option<[u16; 2]> {
        let Some(columns) = self.ownership_columns() else {
            return None;
        };
        let Some(offset) =
            self.column_offset(columns.bindings, event, PROGRAM_IMAGE_ACTOR_BINDING_STRIDE)
        else {
            return None;
        };
        Some([self.read_u16_at(offset), self.read_u16_at(offset + 2)])
    }

    pub(crate) const fn actor_decision_at(&self, index: usize) -> Option<(ScopeId, [u16; 2])> {
        let Some(columns) = self.ownership_columns() else {
            return None;
        };
        let Some(offset) = self.column_offset(
            columns.decisions,
            index,
            PROGRAM_IMAGE_ACTOR_DECISION_STRIDE,
        ) else {
            return None;
        };
        let Some(scope) = ScopeId::decode_raw(self.read_u16_at(offset)) else {
            crate::invariant();
        };
        Some((
            scope,
            [self.read_u16_at(offset + 2), self.read_u16_at(offset + 4)],
        ))
    }

    /// Interpret a checked immutable reference under the existing route history.
    /// Decision edges point backwards, so traversal needs no stack or holder map.
    pub(crate) fn selected_actor(
        &self,
        mut reference: u16,
        selected_arm: &mut dyn FnMut(ScopeId) -> Option<u8>,
    ) -> Option<u8> {
        while reference >= 256 {
            let (scope, alternatives) =
                crate::invariant_some(self.actor_decision_at(usize::from(reference) - 256));
            reference = match selected_arm(scope)? {
                0 => alternatives[0],
                1 => alternatives[1],
                _ => crate::invariant(),
            };
        }
        Some(reference as u8)
    }

    const fn initial_actor_at(&self, row: usize) -> Option<(u8, u8)> {
        let Some(columns) = self.ownership_columns() else {
            return None;
        };
        let Some(offset) =
            self.column_offset(columns.initial, row, PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE)
        else {
            return None;
        };
        Some((self.byte_at(offset), self.byte_at(offset + 1)))
    }

    /// Cold attachment lookup. Rows are sorted and validated at construction.
    pub(crate) const fn initial_actor_for(&self, logical_role: u8) -> Option<u8> {
        let Some(columns) = self.ownership_columns() else {
            return None;
        };
        let mut low = 0;
        let mut high = columns.initial.len as usize;
        while low < high {
            let mid = low + (high - low) / 2;
            let Some((role, actor)) = self.initial_actor_at(mid) else {
                crate::invariant();
            };
            if role < logical_role {
                low = mid + 1;
            } else if role > logical_role {
                high = mid;
            } else {
                return Some(actor);
            }
        }
        None
    }

    pub(super) const fn validate_actor_rows(&self) {
        let Some(columns) = self.ownership_columns() else {
            let mut event = 0;
            while event < self.columns.atom_count() {
                if matches!(self.event_atom_at(event).payload, EventPayload::Endpoint(_)) {
                    crate::invariant();
                }
                event += 1;
            }
            return;
        };
        let bindings = columns.bindings.len as usize;
        let decisions = columns.decisions.len as usize;
        let initial_actors = columns.initial.len as usize;
        if bindings != 0 && bindings != self.columns.atom_count() {
            crate::invariant();
        }
        if initial_actors > 256 || decisions > u16::MAX as usize - 256 {
            crate::invariant();
        }
        let mut row = 0;
        let mut previous = None;
        while row < initial_actors {
            let Some((role, actor)) = self.initial_actor_at(row) else {
                crate::invariant();
            };
            if role > self.facts.max_role || actor > self.facts.max_role {
                crate::invariant();
            }
            if let Some(previous) = previous
                && role <= previous
            {
                crate::invariant();
            }
            previous = Some(role);
            let mut event = 0;
            while event < self.columns.atom_count() {
                if let EventPayload::Endpoint(payload) = self.event_atom_at(event).payload
                    && payload == role
                {
                    break;
                }
                event += 1;
            }
            if event == self.columns.atom_count() {
                crate::invariant();
            }
            let Some([sender, _]) = self.actor_binding_at(event) else {
                crate::invariant();
            };
            if sender != actor as u16 {
                crate::invariant();
            }
            row += 1;
        }

        let mut has_owned_payload = false;
        let mut event = 0;
        while event < self.columns.atom_count() {
            if let EventPayload::Endpoint(role) = self.event_atom_at(event).payload {
                has_owned_payload = true;
                if self.initial_actor_for(role).is_none() {
                    crate::invariant();
                }
            }
            if bindings != 0 {
                let Some([sender, receiver]) = self.actor_binding_at(event) else {
                    crate::invariant();
                };
                if !valid_reference(sender, decisions, self.facts.max_role)
                    || !valid_reference(receiver, decisions, self.facts.max_role)
                {
                    crate::invariant();
                }
            }
            event += 1;
        }
        if has_owned_payload {
            if bindings == 0 || initial_actors == 0 {
                crate::invariant();
            }
        } else if bindings != 0 || decisions != 0 || initial_actors != 0 {
            crate::invariant();
        }

        row = 0;
        while row < decisions {
            let Some((scope, [left, right])) = self.actor_decision_at(row) else {
                crate::invariant();
            };
            // Decisions may name only actors or strictly earlier decisions.
            if !valid_reference(left, row, self.facts.max_role)
                || !valid_reference(right, row, self.facts.max_role)
                || left == right
                || !matches!(scope.kind(), Some(ScopeKind::Route))
            {
                crate::invariant();
            }
            let mut route = 0;
            while route < self.columns.route_resolver_count()
                && self.route_scope_raw_at(route) != scope.raw()
            {
                route += 1;
            }
            if route == self.columns.route_resolver_count() {
                crate::invariant();
            }
            row += 1;
        }
    }
}
