use super::super::{
    CompiledProgramImage, PackedLocalEventRow, RoleLaneImage, RoleLaneScratch, ScopeEvent, ScopeId,
};
use crate::global::{
    compiled::images::{CompiledProgramRef, EventSemanticKind},
    const_dsl::ScopeKind,
    typestate::{
        LocalAtomFacts, LocalConflict, LocalNode, LocalNodeMeta, PackedEventConflict,
        RouteChoiceMark, StateIndex,
    },
};

impl PackedLocalEventRow {
    const FLAG_CHOICE_DETERMINANT: u8 = 1 << 0;
    const SCOPE_SLOT_KIND_SHIFT: u16 = 13;
    const SCOPE_SLOT_ORDINAL_MASK: u16 = (1 << Self::SCOPE_SLOT_KIND_SHIFT) - 1;
    const SCOPE_SLOT_NONE: u16 = u16::MAX;

    pub(crate) const EMPTY: Self = Self {
        eff_index: u16::MAX,
        dependency_row: u16::MAX,
        conflict_row: u16::MAX,
        scope_slot: Self::SCOPE_SLOT_NONE,
        frame_label: 0,
        flags: 0,
    };

    #[inline(always)]
    const fn encode_scope_slot(scope: ScopeId) -> u16 {
        if scope.is_none() {
            return Self::SCOPE_SLOT_NONE;
        }
        let ordinal = scope.local_ordinal();
        if ordinal > Self::SCOPE_SLOT_ORDINAL_MASK {
            panic!("local event scope ordinal overflow");
        }
        ((scope.kind() as u16) << Self::SCOPE_SLOT_KIND_SHIFT) | ordinal
    }

    #[inline(always)]
    const fn decode_scope_slot(slot: u16) -> ScopeId {
        if slot == Self::SCOPE_SLOT_NONE {
            return ScopeId::none();
        }
        let kind = match (slot >> Self::SCOPE_SLOT_KIND_SHIFT) as u8 {
            0 => ScopeKind::Plain,
            1 => ScopeKind::Route,
            2 => ScopeKind::Roll,
            3 => ScopeKind::Parallel,
            _ => panic!("local event scope kind overflow"),
        };
        ScopeId::new(kind, slot & Self::SCOPE_SLOT_ORDINAL_MASK)
    }

    #[inline(always)]
    pub(crate) const fn from_packed_parts(
        eff_index: u16,
        dependency_row: u16,
        conflict_row: u16,
        scope_slot: u16,
        frame_label: u8,
        flags: u8,
    ) -> Self {
        Self {
            eff_index,
            dependency_row,
            conflict_row,
            scope_slot,
            frame_label,
            flags,
        }
    }

    #[inline(always)]
    pub(crate) const fn packed_scope_slot(self) -> u16 {
        self.scope_slot
    }

    #[inline(always)]
    pub(crate) const fn new(
        eff_idx: usize,
        scope: ScopeId,
        frame_label: u8,
        choice: RouteChoiceMark,
    ) -> Self {
        if eff_idx > u16::MAX as usize {
            panic!("local event row eff index overflow");
        }
        let mut flags = 0u8;
        if choice.is_determinant() {
            flags |= Self::FLAG_CHOICE_DETERMINANT;
        }
        Self {
            eff_index: eff_idx as u16,
            dependency_row: u16::MAX,
            conflict_row: u16::MAX,
            scope_slot: Self::encode_scope_slot(scope),
            frame_label,
            flags,
        }
    }

    #[inline(always)]
    pub(crate) const fn with_dependency_row(mut self, row: usize) -> Self {
        if row > u16::MAX as usize {
            panic!("local event dependency row index overflow");
        }
        self.dependency_row = row as u16;
        self
    }

    #[inline(always)]
    pub(crate) const fn with_conflict_row(mut self, row: usize) -> Self {
        if row > u16::MAX as usize {
            panic!("local event conflict row index overflow");
        }
        self.conflict_row = row as u16;
        self
    }

    #[inline(always)]
    const fn is_empty(self) -> bool {
        self.eff_index == u16::MAX
    }

    #[inline(always)]
    const fn is_choice_determinant(self) -> bool {
        (self.flags & Self::FLAG_CHOICE_DETERMINANT) != 0
    }

    #[inline(always)]
    const fn choice_mark(self) -> RouteChoiceMark {
        if self.is_choice_determinant() {
            RouteChoiceMark::Determinant
        } else {
            RouteChoiceMark::Ordinary
        }
    }

    #[inline(always)]
    pub(crate) const fn to_node(
        self,
        role: u8,
        action_ordinal: usize,
        program: &CompiledProgramRef,
        conflict: PackedEventConflict,
    ) -> Option<LocalNode> {
        if self.is_empty() {
            return None;
        }
        let eff_idx = self.eff_index as usize;
        let atom = program.node_at(eff_idx).atom_data();
        let scope = Self::decode_scope_slot(self.scope_slot);
        let resolver = match program.resident_resolver_at(eff_idx) {
            Some(resolver) => resolver.with_scope(scope),
            None => crate::global::const_dsl::RouteResolver::Intrinsic,
        };
        let semantic = EventSemanticKind::ProtocolEvent;
        let route_arm = match conflict.to_conflict() {
            Some(LocalConflict::RouteArm { arm, .. }) => Some(arm),
            Some(LocalConflict::Unconditional | LocalConflict::SharedRoute) | None => None,
        };
        let next = StateIndex::from_usize(action_ordinal + 1);
        let eff_index = crate::eff::EffIndex::from_dense_ordinal(eff_idx);
        let facts = LocalAtomFacts {
            eff_index,
            label: atom.label,
            frame_label: self.frame_label,
            resource: atom.resource,
            origin: atom.origin,
            resolver,
            lane: atom.lane,
        };
        let meta = LocalNodeMeta {
            semantic,
            next,
            scope,
            route_arm,
            choice: self.choice_mark(),
        };
        if atom.from == role && atom.to == role {
            Some(LocalNode::local(facts, meta))
        } else if atom.from == role {
            Some(LocalNode::send(atom.to, facts, meta))
        } else if atom.to == role {
            Some(LocalNode::recv(atom.from, facts, meta))
        } else {
            None
        }
    }
}

impl RoleLaneImage {
    #[inline(always)]
    pub(crate) const fn local_step_node(
        &self,
        step_idx: usize,
        role: u8,
        program: &CompiledProgramRef,
    ) -> Option<LocalNode> {
        match self.local_step_event(step_idx) {
            Some(event) => event.to_node(
                role,
                step_idx,
                program,
                self.event_conflict_for_index(step_idx),
            ),
            None => None,
        }
    }
}

impl RoleLaneScratch {
    #[inline(always)]
    const fn scope_at(program: &CompiledProgramImage, eff_idx: usize) -> ScopeId {
        let view = program.view();
        let markers = view.scope_markers();
        let mut best = ScopeId::none();
        let mut best_start = 0usize;
        let mut best_span = usize::MAX;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if marker.offset > eff_idx {
                break;
            }
            if matches!(marker.event, ScopeEvent::Enter) {
                let start = marker.offset;
                let end = Self::scope_segment_end(markers, idx, view.len());
                if eff_idx >= end {
                    idx += 1;
                    continue;
                }
                if best.is_none() || start > best_start {
                    best = marker.scope_id;
                    best_start = start;
                    best_span = usize::MAX;
                } else if start == best_start {
                    let span = end - start;
                    if span < best_span {
                        best = marker.scope_id;
                        best_start = start;
                        best_span = span;
                    }
                }
            }
            idx += 1;
        }
        best
    }

    #[inline(always)]
    const fn route_scope_and_arm_at(
        program: &CompiledProgramImage,
        eff_idx: usize,
    ) -> Option<(ScopeId, u8)> {
        match Self::route_conflict_for_eff(program.view().scope_markers(), eff_idx).to_conflict() {
            Some(crate::global::typestate::LocalConflict::RouteArm { scope, arm }) => {
                Some((scope, arm))
            }
            Some(
                crate::global::typestate::LocalConflict::Unconditional
                | crate::global::typestate::LocalConflict::SharedRoute,
            )
            | None => None,
        }
    }

    #[inline(always)]
    const fn first_recv_eff_for_route_arm<const ROLE: u8>(
        program: &CompiledProgramImage,
        route: ScopeId,
        arm: u8,
    ) -> Option<usize> {
        if arm >= 2 {
            return None;
        }
        let markers = program.view().scope_markers();
        let Some(ranges) = Self::route_arm_ranges(markers, route) else {
            return None;
        };
        let (start, end) = ranges[arm as usize];
        let view = program.view();
        let mut idx = start;
        while idx < end && idx < view.len() {
            if let Some(atom) = view.atom_at(idx)
                && atom.to == ROLE
                && atom.from != ROLE
            {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(super) const fn local_event_row_for_eff<const ROLE: u8>(
        program: &CompiledProgramImage,
        eff_idx: usize,
        frame_label: u8,
    ) -> PackedLocalEventRow {
        let scope = Self::scope_at(program, eff_idx);
        let route_scope_and_arm = Self::route_scope_and_arm_at(program, eff_idx);
        let choice = match route_scope_and_arm {
            Some((route_scope, arm)) => {
                match Self::first_recv_eff_for_route_arm::<ROLE>(program, route_scope, arm) {
                    Some(first) if first == eff_idx => RouteChoiceMark::Determinant,
                    Some(_) | None => RouteChoiceMark::Ordinary,
                }
            }
            None => RouteChoiceMark::Ordinary,
        };
        PackedLocalEventRow::new(eff_idx, scope, frame_label, choice)
    }
}
