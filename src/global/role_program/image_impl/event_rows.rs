use super::super::{PackedLocalEventRow, RoleLaneImage, ScopeId};
use crate::global::{
    compiled::images::{CompiledProgramRef, EventSemanticKind},
    typestate::{
        LocalAtomFacts, LocalConflict, LocalNode, LocalNodeMeta, PackedEventConflict,
        RouteChoiceMark, StateIndex,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RoleLocalDirection {
    Local,
    Send,
    Recv,
}

#[inline(always)]
pub(super) const fn role_local_direction(role: u8, from: u8, to: u8) -> Option<RoleLocalDirection> {
    if from == role && to == role {
        Some(RoleLocalDirection::Local)
    } else if from == role {
        Some(RoleLocalDirection::Send)
    } else if to == role {
        Some(RoleLocalDirection::Recv)
    } else {
        None
    }
}

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

#[inline(always)]
pub(super) const fn encode_optional_event_fact_row(row: usize) -> Option<u16> {
    if row < u16::MAX as usize {
        Some(row as u16)
    } else {
        None
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests {
    use super::encode_optional_event_fact_row;

    #[test]
    fn optional_event_fact_row_preserves_the_reserved_absent_value() {
        assert_eq!(encode_optional_event_fact_row(0), Some(0));
        assert_eq!(
            encode_optional_event_fact_row(u16::MAX as usize - 1),
            Some(u16::MAX - 1)
        );
        assert_eq!(encode_optional_event_fact_row(u16::MAX as usize), None);
    }
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
        self.dependency_row = match encode_optional_event_fact_row(row) {
            Some(row) => row,
            None => panic!("local event dependency row index overflow"),
        };
        self
    }

    #[inline(always)]
    pub(crate) const fn with_conflict_row(mut self, row: usize) -> Self {
        self.conflict_row = match encode_optional_event_fact_row(row) {
            Some(row) => row,
            None => panic!("local event conflict row index overflow"),
        };
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
    ) -> LocalNode {
        if self.is_empty() {
            crate::invariant();
        }
        let eff_idx = self.eff_index as usize;
        let atom = program.event_atom_at(eff_idx);
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
        match role_local_direction(role, atom.from, atom.to) {
            Some(RoleLocalDirection::Local) => LocalNode::local(facts, meta),
            Some(RoleLocalDirection::Send) => LocalNode::send(atom.to, facts, meta),
            Some(RoleLocalDirection::Recv) => LocalNode::recv(atom.from, facts, meta),
            None => crate::invariant(),
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
            Some(event) => Some(event.to_node(
                role,
                step_idx,
                program,
                self.event_conflict_for_index(step_idx),
            )),
            None => None,
        }
    }
}
