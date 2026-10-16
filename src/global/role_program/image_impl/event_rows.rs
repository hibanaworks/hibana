use super::super::{PackedLocalEventRow, RoleLaneImage, ScopeId};
use crate::global::{
    compiled::images::{CompiledProgramRef, EventSemanticKind},
    typestate::{
        LocalAtomFacts, LocalConflict, LocalNode, LocalNodeMeta, PackedEventConflict,
        RouteChoiceMark, StateIndex,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RoleLocalDirection {
    Local,
    Send,
    Recv,
}

impl RoleLocalDirection {
    const fn packed_bits(self) -> u8 {
        match self {
            Self::Local => 0,
            Self::Send => 2,
            Self::Recv => 4,
        }
    }

    const fn decode(flags: u8) -> Option<Self> {
        match flags & !PackedLocalEventRow::FLAG_CHOICE_DETERMINANT {
            0 => Some(Self::Local),
            2 => Some(Self::Send),
            4 => Some(Self::Recv),
            _ => None,
        }
    }
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
        || RoleLocalDirection::decode(flags).is_none()
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
    use super::*;
    use crate::{
        Endpoint, g,
        global::{
            compiled::{
                images::{ProgramImageBytes, ProgramImagePlan},
                lowering::CompiledProgramImage,
            },
            typestate::LocalAction,
        },
    };

    #[test]
    fn optional_event_fact_row_preserves_the_reserved_absent_value() {
        assert_eq!(encode_optional_event_fact_row(0), Some(0));
        assert_eq!(
            encode_optional_event_fact_row(u16::MAX as usize - 1),
            Some(u16::MAX - 1)
        );
        assert_eq!(encode_optional_event_fact_row(u16::MAX as usize), None);
    }

    #[test]
    fn distinct_facets_at_one_actor_keep_both_payload_operations() {
        type Loan = g::Msg<1, Endpoint<'static, 2>>;
        type Data = g::Msg<2, u32>;
        type Steps = g::Seq<g::Send<0, 1, Loan>, g::Send<2, 1, Data>>;
        const SOURCE: g::ProgramSourceData<8> = g::ProgramSourceData::lower::<Steps>();
        const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
        const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
        static BYTES: ProgramImageBytes<64> =
            match ProgramImageBytes::from_source_if_fits(&SOURCE, PLAN.columns()) {
                Some(bytes) => bytes,
                None => panic!("owned program fixture"),
            };
        let program = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
        assert_eq!(program.actor_binding_at(1), Some([1, 1]));
        let atom = program.event_atom_at(1);
        assert_eq!((atom.from, atom.to), (2, 1));
        assert_eq!(
            atom.payload,
            crate::eff::EventPayload::Wire(<u32 as crate::transport::wire::WirePayload>::SCHEMA_ID,)
        );

        let send = PackedLocalEventRow::new(
            1,
            ScopeId::none(),
            0,
            RouteChoiceMark::Ordinary,
            RoleLocalDirection::Send,
        );
        let recv = PackedLocalEventRow::new(
            1,
            ScopeId::none(),
            0,
            RouteChoiceMark::Ordinary,
            RoleLocalDirection::Recv,
        );
        let send_node = send.to_node(0, &program, PackedEventConflict::none());
        let recv_node = recv.to_node(1, &program, PackedEventConflict::none());
        assert!(matches!(
            send_node.action(),
            LocalAction::Send { peer: 1, .. }
        ));
        assert!(matches!(
            recv_node.action(),
            LocalAction::Recv { peer: 2, .. }
        ));
        assert_eq!(send_node.next(), StateIndex::from_usize(1));
        assert_eq!(recv_node.next(), StateIndex::from_usize(2));
        assert_eq!(core::mem::size_of::<PackedLocalEventRow>(), 10);
    }

    #[test]
    fn ordinary_self_message_still_has_one_local_operation() {
        use crate::runtime::program::project;
        let protocol = g::send::<7, 7, g::Msg<3, ()>>();
        let role = project::<7>(&protocol);
        let rows = role.role_image_ref();
        assert_eq!(rows.local_step_count(), 1);
        assert!(matches!(
            rows.local_step_node(0).unwrap().action(),
            LocalAction::Local { .. }
        ));
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
        direction: RoleLocalDirection,
    ) -> Self {
        if eff_idx >= crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY {
            panic!("local event row eff index overflow");
        }
        let mut flags = direction.packed_bits();
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

    /// Direction belongs to the logical endpoint, independently of which
    /// actor holds it. Distinct facets at one actor retain send and receive.
    pub(super) const fn direction(self) -> RoleLocalDirection {
        match RoleLocalDirection::decode(self.flags) {
            Some(direction) => direction,
            None => crate::invariant(),
        }
    }

    pub(crate) const fn to_node(
        self,
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
        match self.direction() {
            RoleLocalDirection::Local if atom.from == atom.to => LocalNode::local(facts, meta),
            RoleLocalDirection::Send if atom.from != atom.to => {
                LocalNode::send(atom.to, facts, meta)
            }
            RoleLocalDirection::Recv if atom.from != atom.to => {
                LocalNode::recv(atom.from, facts, meta)
            }
            RoleLocalDirection::Local | RoleLocalDirection::Send | RoleLocalDirection::Recv => {
                crate::invariant()
            }
        }
    }
}

impl<'a> RoleLaneImage<'a> {
    #[inline(always)]
    pub(crate) const fn local_step_node(
        &self,
        step_idx: usize,
        program: &CompiledProgramRef,
    ) -> Option<LocalNode> {
        match self.local_step_event(step_idx) {
            Some(event) => {
                Some(event.to_node(step_idx, program, self.event_conflict_for_index(step_idx)))
            }
            None => None,
        }
    }
}
