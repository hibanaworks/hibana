use super::super::{PackedLocalEventRow, RoleLaneImage, ScopeId};
use crate::global::{
    compiled::images::{CompiledProgramRef, EventSemanticKind},
    typestate::{
        LocalAtomFacts, LocalConflict, LocalNode, LocalNodeMeta, PackedEventConflict,
        RouteChoiceMark, StateIndex,
    },
};

pub(super) const fn decode_resident_event_header(
    eff_index: u16,
    scope_raw: u16,
    flags: u8,
) -> Option<ScopeId> {
    if eff_index as usize >= crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY
        || (flags & !PackedLocalEventRow::FLAG_CHOICE_DETERMINANT) != 0
    {
        return None;
    }
    let scope = match ScopeId::decode_raw(scope_raw) {
        Some(scope) => scope,
        None => return None,
    };
    Some(scope)
}

impl PackedLocalEventRow {
    const FLAG_CHOICE_DETERMINANT: u8 = 1 << 0;

    #[inline(always)]
    pub(crate) const fn from_packed_parts(
        eff_index: u16,
        dependency_row: u16,
        conflict_row: u16,
        scope_raw: u16,
        frame_label: u8,
        flags: u8,
    ) -> Self {
        let scope = match decode_resident_event_header(eff_index, scope_raw, flags) {
            Some(scope) => scope,
            None => crate::invariant(),
        };
        Self {
            eff_index,
            dependency_row,
            conflict_row,
            scope,
            frame_label,
            flags,
        }
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }

    #[inline(always)]
    pub(crate) const fn new(
        eff_idx: usize,
        scope: ScopeId,
        frame_label: u8,
        choice: RouteChoiceMark,
    ) -> Self {
        if eff_idx >= crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY {
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
            scope,
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
        let scope = self.scope;
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
            origin: atom.origin,
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

impl<'a> RoleLaneImage<'a> {
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
