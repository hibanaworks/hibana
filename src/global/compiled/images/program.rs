use crate::{
    control::cap::mint::ControlOp,
    eff::EffIndex,
    global::{
        ControlDesc,
        const_dsl::{CompactScopeId, PolicyMode, ScopeId},
    },
};

/// Precomputed dynamic policy site discovered during program lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DynamicPolicySite {
    eff_index: EffIndex,
    logical_label: u8,
    resource_tag: Option<u8>,
    op: Option<ControlOp>,
    policy: PolicyMode,
}

impl DynamicPolicySite {
    #[inline(always)]
    pub(crate) const fn new(
        eff_index: EffIndex,
        logical_label: u8,
        resource_tag: Option<u8>,
        op: Option<ControlOp>,
        policy: PolicyMode,
    ) -> Self {
        Self {
            eff_index,
            logical_label,
            resource_tag,
            op,
            policy,
        }
    }

    #[inline(always)]
    pub(crate) const fn eff_index(&self) -> EffIndex {
        self.eff_index
    }

    #[inline(always)]
    pub(crate) const fn logical_label(&self) -> u8 {
        self.logical_label
    }

    #[inline(always)]
    pub(crate) const fn resource_tag(&self) -> Option<u8> {
        self.resource_tag
    }

    #[inline(always)]
    pub(crate) const fn op(&self) -> Option<ControlOp> {
        self.op
    }

    #[inline(always)]
    pub(crate) const fn policy(&self) -> PolicyMode {
        self.policy
    }

    #[inline(always)]
    pub(crate) const fn policy_id(&self) -> u16 {
        match self.policy {
            PolicyMode::Dynamic { policy_id, .. } => policy_id,
            PolicyMode::Static => 0,
        }
    }
}

const ROUTE_CONTROL_NONE: u8 = u8::MAX;

/// Shared immutable route/controller facts derived once per lowered program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteControlRecord {
    scope_id: CompactScopeId,
    controller_role: u8,
    route_policy_tag: u8,
    route_policy_op: Option<ControlOp>,
    route_policy_id: u16,
    route_policy_eff: EffIndex,
}

impl RouteControlRecord {
    #[inline(always)]
    pub(in crate::global::compiled) const fn new(
        scope_id: ScopeId,
        controller_role: Option<u8>,
        route_policy_id: u16,
        route_policy_eff: EffIndex,
        route_policy_tag: u8,
        route_policy_op: Option<ControlOp>,
    ) -> Self {
        Self {
            scope_id: CompactScopeId::from_scope_id(scope_id),
            controller_role: match controller_role {
                Some(role) => role,
                None => ROUTE_CONTROL_NONE,
            },
            route_policy_tag,
            route_policy_op,
            route_policy_id,
            route_policy_eff,
        }
    }

    #[inline(always)]
    pub(crate) const fn controller_role(self) -> Option<u8> {
        if self.controller_role == ROUTE_CONTROL_NONE {
            None
        } else {
            Some(self.controller_role)
        }
    }

    #[inline(always)]
    pub(crate) fn route_controller(self) -> Option<(PolicyMode, EffIndex, u8, ControlOp)> {
        if self.route_policy_eff == EffIndex::MAX {
            return None;
        }
        let op = self.route_policy_op?;
        let policy = if self.route_policy_id == crate::global::ControlDesc::STATIC_POLICY_SITE {
            PolicyMode::Static
        } else {
            PolicyMode::Dynamic {
                policy_id: self.route_policy_id,
                scope: self.scope_id,
            }
        };
        Some((policy, self.route_policy_eff, self.route_policy_tag, op))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ControlSemanticKind {
    Other = 0,
    RouteArm = 1,
    LoopContinue = 2,
    LoopBreak = 3,
}

impl ControlSemanticKind {
    #[inline(always)]
    pub(crate) const fn packed_bits(self) -> u8 {
        match self {
            Self::Other => 0,
            Self::RouteArm => 1,
            Self::LoopContinue => 2,
            Self::LoopBreak => 3,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_packed_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Other,
            1 => Self::RouteArm,
            2 => Self::LoopContinue,
            3 => Self::LoopBreak,
            _ => panic!("invalid packed control semantic bits"),
        }
    }

    #[inline(always)]
    pub(crate) const fn from_control_op(op: Option<ControlOp>) -> Self {
        match op {
            Some(ControlOp::LoopContinue) => Self::LoopContinue,
            Some(ControlOp::LoopBreak) => Self::LoopBreak,
            Some(ControlOp::RouteDecision) => Self::RouteArm,
            _ => Self::Other,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_control_desc(desc: Option<ControlDesc>) -> Self {
        match desc {
            Some(desc) => Self::from_control_op(Some(desc.op())),
            None => Self::Other,
        }
    }

    #[inline(always)]
    pub(crate) const fn is_loop(self) -> bool {
        matches!(self, Self::LoopContinue | Self::LoopBreak)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControlSemanticsTable {}

pub(in crate::global::compiled) static CONTROL_SEMANTICS_TABLE: ControlSemanticsTable =
    ControlSemanticsTable::EMPTY;

impl ControlSemanticsTable {
    pub(crate) const EMPTY: Self = Self {};
}

#[cfg(test)]
pub(in crate::global::compiled) const MAX_DYNAMIC_POLICY_SITES: usize =
    crate::eff::meta::MAX_EFF_NODES;
pub(crate) const MAX_COMPILED_PROGRAM_TAP_EVENTS: usize = 512;
pub(crate) const MAX_COMPILED_PROGRAM_RESOURCES: usize = 128;
pub(crate) const MAX_COMPILED_PROGRAM_SCOPES: usize = crate::eff::meta::MAX_EFF_NODES;
pub(crate) const MAX_COMPILED_PROGRAM_CONTROLS: usize = crate::eff::meta::MAX_EFF_NODES;
#[cfg(test)]
pub(crate) const MAX_COMPILED_PROGRAM_ROUTE_CONTROLS: usize = crate::eff::meta::MAX_EFF_NODES;

#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramCounts {
    pub(crate) tap_events: usize,
    pub(crate) resources: usize,
    pub(crate) controls: usize,
    pub(crate) dynamic_policy_sites: usize,
    pub(crate) route_controls: usize,
}

impl CompiledProgramCounts {
    #[cfg(test)]
    const MAX: Self = Self {
        tap_events: MAX_COMPILED_PROGRAM_TAP_EVENTS,
        resources: MAX_COMPILED_PROGRAM_RESOURCES,
        controls: MAX_COMPILED_PROGRAM_CONTROLS,
        dynamic_policy_sites: MAX_DYNAMIC_POLICY_SITES,
        route_controls: MAX_COMPILED_PROGRAM_ROUTE_CONTROLS,
    };
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::{CompiledProgramCounts, ControlSemanticKind, ControlSemanticsTable};
    use crate::control::cap::mint::ControlOp;

    #[test]
    fn compiled_program_counts_remain_plain_derived_counts() {
        assert_eq!(size_of::<CompiledProgramCounts>(), 5 * size_of::<usize>());
        let max = CompiledProgramCounts::MAX;
        assert!(max.tap_events > 0);
        assert!(max.resources > 0);
        assert!(max.controls > 0);
        assert!(max.dynamic_policy_sites > 0);
        assert!(max.route_controls > 0);
    }

    #[test]
    fn control_semantics_table_stays_stateless() {
        assert_eq!(
            size_of::<ControlSemanticsTable>(),
            0,
            "ControlSemanticsTable must stay a zero-sized semantic dispatch token"
        );
    }

    #[test]
    fn compiled_program_marks_loop_control_semantics_from_control_metadata() {
        assert_eq!(
            ControlSemanticKind::from_control_op(Some(ControlOp::LoopContinue)),
            ControlSemanticKind::LoopContinue
        );
        assert_eq!(
            ControlSemanticKind::from_control_op(Some(ControlOp::LoopBreak)),
            ControlSemanticKind::LoopBreak
        );
        assert_eq!(
            ControlSemanticKind::from_control_op(Some(ControlOp::RouteDecision)),
            ControlSemanticKind::RouteArm
        );
    }
}
