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
            PROGRAM_IMAGE_SCOPE_MARKER_STRIDE, ProgramColumnRange,
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

const fn source_order_word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

impl CompiledProgramRef {
    #[inline(always)]
    pub(crate) const fn has_actor_bindings(&self) -> bool {
        self.columns.has_ownership()
    }

    #[inline(always)]
    pub(crate) const fn actor_decision_count(&self) -> usize {
        self.actor_decisions as usize
    }

    #[inline(always)]
    const fn ownership_columns(&self) -> Option<OwnershipColumns> {
        if !self.columns.has_ownership() {
            return None;
        }
        // Construction checked these contiguous ranges against the byte image.
        let decisions = self.actor_binding_start as usize
            + self.columns.atom_count() * PROGRAM_IMAGE_ACTOR_BINDING_STRIDE;
        let initial = decisions + self.actor_decision_count() * PROGRAM_IMAGE_ACTOR_DECISION_STRIDE;
        Some(OwnershipColumns {
            bindings: ProgramColumnRange {
                offset: self.actor_binding_start,
                len: self.columns.atom_count() as u16,
            },
            decisions: ProgramColumnRange {
                offset: decisions as u16,
                len: self.actor_decisions,
            },
            initial: ProgramColumnRange {
                offset: initial as u16,
                len: ((self.blob.len() - initial) / PROGRAM_IMAGE_INITIAL_ACTOR_STRIDE) as u16,
            },
        })
    }

    pub(super) const fn decode_ownership_columns(&self) -> Option<OwnershipColumns> {
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
        self.source_scope_bounds(scope)
    }

    pub(crate) const fn source_route_separates(
        &self,
        scope: ScopeId,
        left: usize,
        right: usize,
    ) -> bool {
        let Some((first, split, end)) = self.source_route_bounds(scope) else {
            return false;
        };
        (first <= left && left < split && split <= right && right < end)
            || (first <= right && right < split && split <= left && left < end)
    }

    /// The binary source scope separating two ordered effects, when it is a
    /// route. A parallel split and sequential terms do not create alternatives.
    pub(crate) const fn source_separating_route(
        &self,
        before: usize,
        after: usize,
    ) -> Option<(ScopeId, usize)> {
        if before >= after || after >= self.columns.atom_count() || self.source_scopes == 0 {
            return None;
        }
        let bytes = self.source_order.as_slice();
        let mut scope = source_order_word(bytes, self.source_scopes as usize * 9 + before * 2);
        while scope != u16::MAX {
            let row = scope as usize * 9;
            let split = source_order_word(bytes, row + 2) as usize;
            let end = source_order_word(bytes, row + 4) as usize;
            if before < split && split <= after && after < end {
                return if bytes[row + 8] == ScopeKind::Route as u8 {
                    Some((ScopeId::route(scope), split))
                } else {
                    None
                };
            }
            scope = source_order_word(bytes, row + 6);
        }
        None
    }

    pub(crate) const fn source_route_visible_from_range(
        &self,
        scope: ScopeId,
        start: usize,
        end: usize,
    ) -> bool {
        if start >= end || end > self.columns.atom_count() {
            return false;
        }
        let Some((first, split, boundary)) = self.source_route_bounds(scope) else {
            return false;
        };
        (first <= start && end <= split)
            || (split <= start && end <= boundary)
            || (start <= first && boundary <= end)
    }

    const fn source_route_bounds(&self, scope: ScopeId) -> Option<(usize, usize, usize)> {
        if !matches!(scope.kind(), Some(ScopeKind::Route)) {
            return None;
        }
        self.source_scope_bounds(scope)
    }

    #[inline]
    pub(crate) const fn source_route_arm_for_event(
        &self,
        scope: ScopeId,
        event: usize,
    ) -> Option<u8> {
        let Some((first, split, finish)) = self.source_route_bounds(scope) else {
            return None;
        };
        if first <= event && event < split {
            Some(0)
        } else if split <= event && event < finish {
            Some(1)
        } else {
            None
        }
    }

    /// Arm of an enclosing route containing a complete descendant route.
    /// Global arms are nonempty, so distinct nested routes cannot have equal
    /// bounds. The checked source intervals determine ancestry directly.
    #[inline]
    pub(crate) const fn source_route_ancestor_arm(
        &self,
        descendant: ScopeId,
        ancestor: ScopeId,
    ) -> Option<u8> {
        let Some((start, _, end)) = self.source_route_bounds(descendant) else {
            return None;
        };
        let Some((first, split, boundary)) = self.source_route_bounds(ancestor) else {
            return None;
        };
        if first <= start && end <= split {
            Some(0)
        } else if split <= start && end <= boundary {
            Some(1)
        } else {
            None
        }
    }

    #[inline]
    const fn source_scope_bounds(&self, scope: ScopeId) -> Option<(usize, usize, usize)> {
        let slot = scope.local_ordinal() as usize;
        if slot >= self.source_scopes as usize {
            return None;
        }
        let kind = match scope.kind() {
            Some(kind) => kind as u8,
            _ => return None,
        };
        let row = slot * 9;
        let bytes = self.source_order.as_slice();
        if bytes[row + 8] != kind {
            return None;
        }
        Some((
            source_order_word(bytes, row) as usize,
            source_order_word(bytes, row + 2) as usize,
            source_order_word(bytes, row + 4) as usize,
        ))
    }

    /// Enclosing repetitions, from the innermost source scope to the root.
    /// Dedicated repetition links skip binary scopes. The image constructor
    /// checks the links and event heads once.
    pub(crate) fn source_roll_ancestors(&self, event: usize) -> impl Iterator<Item = ScopeId> + '_ {
        let offset = self.source_scopes as usize * 9 + self.columns.atom_count() * 2;
        let mut scope = if self.source_scopes == 0
            || event >= self.columns.atom_count()
            || offset == self.source_order.len()
        {
            u16::MAX
        } else {
            self.source_order_u16(offset + event * 2)
        };
        core::iter::from_fn(move || {
            if scope == u16::MAX {
                return None;
            }
            let ordinal = scope;
            scope = self.source_order_u16(ordinal as usize * 9 + 6);
            Some(ScopeId::roll_scope(ordinal))
        })
    }

    pub(crate) const fn source_roll_has_parallel_entry(
        &self,
        child: ScopeId,
        event: usize,
    ) -> bool {
        if !matches!(child.kind(), Some(ScopeKind::Roll)) || event >= self.columns.atom_count() {
            return false;
        }
        let Some((start, _, end)) = self.source_scope_bounds(child) else {
            return false;
        };
        let mut parent = self.source_order_u16(self.source_scopes as usize * 9 + start * 2);
        while parent != u16::MAX {
            let row = parent as usize * 9;
            if self.source_order.byte_at(row + 8) == ScopeKind::Parallel as u8 {
                let a = self.source_order_u16(row) as usize;
                let split = self.source_order_u16(row + 2) as usize;
                let b = self.source_order_u16(row + 4) as usize;
                if (a <= start && end <= split && split <= event && event < b)
                    || (a <= event && event < split && split <= start && end <= b)
                {
                    return true;
                }
            }
            parent = self.source_order_u16(row + 6);
        }
        false
    }

    pub(crate) const fn roll_precedes(&self, scope: ScopeId, after: usize) -> bool {
        if !matches!(scope.kind(), Some(ScopeKind::Roll)) {
            return false;
        }
        let Some((_, _, end)) = self.source_scope_bounds(scope) else {
            return false;
        };
        self.source_precedes(end - 1, after)
    }

    const fn source_marker_lower_bound(&self, offset: usize) -> usize {
        let markers = self
            .columns
            .scope_markers_for(self.source_scope_marker_count());
        let mut lower = 0;
        let mut upper = markers.len as usize;
        while lower < upper {
            let middle = lower + (upper - lower) / 2;
            let at = markers.offset as usize + middle * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
            if (self.read_u16_at(at) as usize) < offset {
                lower = middle + 1;
            } else {
                upper = middle;
            }
        }
        lower
    }

    pub(crate) const fn source_markers_at(&self, offset: usize) -> core::ops::Range<usize> {
        let first = self.source_marker_lower_bound(offset);
        let end = if offset == usize::MAX {
            first
        } else {
            self.source_marker_lower_bound(offset + 1)
        };
        first..end
    }

    /// Strict single-visit source order. Enumeration orders sequential terms,
    /// except for operations separated by parallel or alternative arms.
    /// Follow precomputed enclosing scopes, without rescanning source markers.
    #[inline]
    pub(crate) const fn source_precedes(&self, before: usize, after: usize) -> bool {
        if before >= after || after >= self.columns.atom_count() {
            return false;
        }
        if self.source_scopes == 0 {
            return true;
        }
        let mut scope = self.source_order_u16(self.source_scopes as usize * 9 + before * 2);
        while scope != u16::MAX {
            let row = scope as usize * 9;
            let split = self.source_order_u16(row + 2) as usize;
            let end = self.source_order_u16(row + 4) as usize;
            if before < split && split <= after && after < end {
                return false;
            }
            scope = self.source_order_u16(row + 6);
        }
        true
    }

    #[cfg(any(kani, all(test, hibana_repo_tests)))]
    pub(crate) const fn source_precedes_scan(&self, before: usize, after: usize) -> bool {
        if before >= after || after >= self.columns.atom_count() {
            return false;
        }
        let markers = self
            .columns
            .scope_markers_for(self.source_scope_marker_count());
        // Only a split strictly after `before` and no later than `after`
        // can separate the events. Source markers are emitted in offset order.
        let first = self.source_marker_lower_bound(before + 1);
        let last = self.source_marker_lower_bound(after + 1);
        let mut marker = first;
        let mut scope_limit = ScopeId::LOCAL_CAPACITY;
        while marker < last {
            let offset = markers.offset as usize + marker * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
            let split = self.read_u16_at(offset) as usize;
            let raw_scope = self.read_u16_at(offset + 2);
            let Some(scope) = ScopeId::decode_raw(raw_scope) else {
                crate::invariant();
            };
            let ordinal = scope.local_ordinal();
            if ordinal >= scope_limit {
                marker += 1;
                continue;
            }
            let tag = self.byte_at(offset + 4) & 3;
            // Ordinals follow source preorder. A scope entered inside this
            // window, and every later scope, cannot enclose `before`.
            // A route's second enter follows its left exit, checked below.
            if tag == 0 {
                scope_limit = ordinal;
                marker += 1;
                continue;
            }
            // Par has a Split marker. Route closes its left arm and
            // enters its right arm at the same offset instead.
            if matches!(
                (scope.kind(), tag),
                (Some(ScopeKind::Parallel), 1) | (Some(ScopeKind::Route), 2)
            ) {
                // No enter for this scope occurred in the window. Its split
                // separates the events unless its final exit is here too.
                let mut separates = matches!(scope.kind(), Some(ScopeKind::Parallel));
                let mut bound = marker + 1;
                while bound < last {
                    let at = markers.offset as usize + bound * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
                    if self.read_u16_at(at + 2) == raw_scope {
                        match self.byte_at(at + 4) & 3 {
                            0 => separates = self.read_u16_at(at) as usize == split,
                            2 => {
                                separates = false;
                                break;
                            }
                            _ => (),
                        }
                    }
                    bound += 1;
                }
                if separates {
                    return false;
                }
                // This enclosing scope ends by `after`. Any later ordinal
                // that could enclose `before` is nested inside it and ends
                // here too; its split cannot separate this pair either.
                scope_limit = ordinal;
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
        let Some((start, _, end)) = self.source_scope_bounds(scope) else {
            crate::invariant();
        };
        Some((scope, start, end))
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(super) const fn source_roll_at_scan(
        &self,
        marker: usize,
    ) -> Option<(ScopeId, usize, usize)> {
        let markers = self
            .columns
            .scope_markers_for(self.source_scope_marker_count());
        if marker >= markers.len as usize {
            return None;
        }
        let at = markers.offset as usize + marker * PROGRAM_IMAGE_SCOPE_MARKER_STRIDE;
        let Some(scope) = ScopeId::decode_raw(self.read_u16_at(at + 2)) else {
            crate::invariant();
        };
        if !matches!(scope.kind(), Some(ScopeKind::Roll)) || self.byte_at(at + 4) & 3 != 0 {
            return None;
        }
        let start = self.read_u16_at(at) as usize;
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
            if self.route_resolver_slot(scope).is_none() {
                crate::invariant();
            }
            row += 1;
        }
    }
}
