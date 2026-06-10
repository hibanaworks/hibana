use crate::{
    control::{cap::mint::ControlOp, cluster::core::DecisionSubject},
    eff::EffIndex,
    global::{ControlDesc, const_dsl::ResolverMode},
};

/// Precomputed dynamic resolver site discovered during program lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DynamicPolicySite {
    eff_index: EffIndex,
    logical_label: u8,
    subject: Option<DecisionSubject>,
    policy: ResolverMode,
}

impl DynamicPolicySite {
    #[inline(always)]
    pub(crate) const fn new(
        eff_index: EffIndex,
        logical_label: u8,
        subject: Option<DecisionSubject>,
        policy: ResolverMode,
    ) -> Self {
        Self {
            eff_index,
            logical_label,
            subject,
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
    pub(crate) const fn subject(&self) -> Option<DecisionSubject> {
        self.subject
    }

    #[inline(always)]
    pub(crate) const fn policy(&self) -> ResolverMode {
        self.policy
    }

    #[inline(always)]
    pub(crate) const fn policy_id(&self) -> u16 {
        match self.policy {
            ResolverMode::Dynamic { policy_id, .. } => policy_id,
            ResolverMode::Static => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ControlSemanticKind {
    Other = 0,
    DecisionArm = 1,
    LoopContinue = 2,
    LoopBreak = 3,
}

impl ControlSemanticKind {
    #[inline(always)]
    pub(crate) const fn packed_bits(self) -> u8 {
        match self {
            Self::Other => 0,
            Self::DecisionArm => 1,
            Self::LoopContinue => 2,
            Self::LoopBreak => 3,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_packed_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Other,
            1 => Self::DecisionArm,
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

pub(crate) const MAX_COMPILED_PROGRAM_TAP_EVENTS: usize = 512;
pub(crate) const MAX_COMPILED_PROGRAM_RESOURCES: usize = 128;
pub(crate) const MAX_COMPILED_PROGRAM_SCOPES: usize = crate::eff::meta::MAX_EFF_NODES;
pub(crate) const MAX_COMPILED_PROGRAM_CONTROLS: usize = crate::eff::meta::MAX_EFF_NODES;

#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramCounts {
    pub(crate) tap_events: usize,
    pub(crate) resources: usize,
    pub(crate) controls: usize,
    pub(crate) dynamic_policy_sites: usize,
    pub(crate) route_controls: usize,
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::{CompiledProgramCounts, ControlSemanticKind, ControlSemanticsTable};
    use crate::control::cap::mint::ControlOp;

    #[test]
    fn compiled_program_counts_remain_plain_derived_counts() {
        assert_eq!(size_of::<CompiledProgramCounts>(), 5 * size_of::<usize>());
        let max = CompiledProgramCounts {
            tap_events: super::MAX_COMPILED_PROGRAM_TAP_EVENTS,
            resources: super::MAX_COMPILED_PROGRAM_RESOURCES,
            controls: super::MAX_COMPILED_PROGRAM_CONTROLS,
            dynamic_policy_sites: crate::eff::meta::MAX_EFF_NODES,
            route_controls: crate::eff::meta::MAX_EFF_NODES,
        };
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
            ControlSemanticKind::from_control_op(Some(ControlOp::Fence)),
            ControlSemanticKind::Other
        );
    }
}
