//! Projected control-descriptor metadata helpers.

use crate::{
    control::{cap::mint::ControlOp, cluster::core::DecisionSubject},
    eff::EffIndex,
    global::{
        ControlDesc,
        compiled::images::{CompiledProgramRef, DynamicPolicySite},
        const_dsl::{ControlScopeKind, ResolverMode},
    },
};

#[inline(always)]
pub(crate) const fn lane_open_tap_event_id() -> u16 {
    0x0100
}

#[inline(always)]
pub(crate) const fn control_op_tap_event_id(op: ControlOp) -> u16 {
    use crate::observe::ids;
    match op {
        ControlOp::LoopContinue | ControlOp::LoopBreak => ids::LOOP_DECISION,
        ControlOp::StateSnapshot => ids::STATE_SNAPSHOT_REQ,
        ControlOp::StateRestore => ids::STATE_RESTORE_REQ,
        ControlOp::TopologyBegin => ids::TOPOLOGY_BEGIN,
        ControlOp::TopologyAck => ids::TOPOLOGY_ACK,
        ControlOp::TopologyCommit => ids::TOPOLOGY_COMMIT,
        ControlOp::AbortBegin => ids::ABORT_BEGIN,
        ControlOp::AbortAck => ids::ABORT_ACK,
        ControlOp::Fence => ids::POLICY_RA_OK,
        ControlOp::TxCommit => ids::POLICY_COMMIT,
        ControlOp::TxAbort => ids::POLICY_TX_ABORT,
    }
}

#[derive(Clone, Copy)]
struct ControlScopeIter {
    mask: u8,
    next: u8,
}

impl ControlScopeIter {
    #[inline(always)]
    const fn new(mask: u8) -> Self {
        Self { mask, next: 0 }
    }
}

impl Iterator for ControlScopeIter {
    type Item = ControlScopeKind;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        while self.next < 6 {
            let bit = 1u8 << self.next;
            let scope_kind = match self.next {
                0 => ControlScopeKind::Loop,
                1 => ControlScopeKind::State,
                2 => ControlScopeKind::Abort,
                3 => ControlScopeKind::Topology,
                4 => ControlScopeKind::Policy,
                5 => ControlScopeKind::Route,
                _ => unreachable!(),
            };
            self.next += 1;
            if self.mask & bit != 0 {
                return Some(scope_kind);
            }
        }
        None
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EffectEnvelopeRef<'a> {
    program: CompiledProgramRef,
    _marker: core::marker::PhantomData<&'a ()>,
}

impl<'a> EffectEnvelopeRef<'a> {
    #[inline(always)]
    pub(crate) const fn from_program_ref(program: CompiledProgramRef) -> Self {
        Self {
            program,
            _marker: core::marker::PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn resources(&self) -> ResourceIter<'a> {
        ResourceIter {
            inner: ProgramImageResourceIter::new(self.program),
        }
    }

    #[inline(always)]
    pub(crate) fn resource_policy(&self, descriptor: &ResourceDescriptor) -> ResolverMode {
        let policy_site = descriptor.control.policy_site();
        if policy_site == ResourceDescriptor::STATIC_POLICY_SITE {
            ResolverMode::Static
        } else {
            ProgramImageDynamicPolicySiteIter::new(self.program)
                .nth(policy_site as usize)
                .map(|site| site.policy())
                .unwrap_or(ResolverMode::Static)
        }
    }

    #[inline(always)]
    pub(crate) fn control_scopes(&self) -> impl Iterator<Item = ControlScopeKind> {
        ControlScopeIter::new(self.program.control_scope_mask())
    }
}

pub(crate) struct ResourceIter<'a> {
    inner: ProgramImageResourceIter<'a>,
}

impl Iterator for ResourceIter<'_> {
    type Item = ResourceDescriptor;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

pub(crate) struct ProgramImageDynamicPolicySiteIter<'a> {
    program: CompiledProgramRef,
    row: usize,
    _marker: core::marker::PhantomData<&'a ()>,
}

impl<'a> ProgramImageDynamicPolicySiteIter<'a> {
    #[inline(always)]
    pub(crate) const fn new(program: CompiledProgramRef) -> Self {
        Self {
            program,
            row: 0,
            _marker: core::marker::PhantomData,
        }
    }
}

impl Iterator for ProgramImageDynamicPolicySiteIter<'_> {
    type Item = DynamicPolicySite;

    fn next(&mut self) -> Option<Self::Item> {
        while self.row < self.program.atom_row_count() {
            let row = self.row;
            self.row += 1;
            let offset = self.program.atom_eff_at_row(row)?;
            let Some(policy) = self.program.resident_policy_at(offset) else {
                continue;
            };
            if !policy.is_dynamic() {
                continue;
            }
            let atom = self
                .program
                .atom_at(offset)
                .expect("atom row offset must resolve to an atom");
            let subject = match self
                .program
                .resident_control_desc_at(offset)
                .map(|desc| desc.op())
            {
                Some(ControlOp::LoopContinue) => Some(DecisionSubject::LoopContinue),
                Some(ControlOp::LoopBreak) => Some(DecisionSubject::LoopBreak),
                Some(_) => None,
                None => Some(DecisionSubject::RouteArm),
            };
            return Some(DynamicPolicySite::new(
                EffIndex::from_dense_ordinal(offset),
                atom.label,
                subject,
                policy,
            ));
        }
        None
    }
}

pub(crate) struct ProgramImageResourceIter<'a> {
    program: CompiledProgramRef,
    row: usize,
    dynamic_policy_site_len: u16,
    _marker: core::marker::PhantomData<&'a ()>,
}

impl<'a> ProgramImageResourceIter<'a> {
    #[inline(always)]
    const fn new(program: CompiledProgramRef) -> Self {
        Self {
            program,
            row: 0,
            dynamic_policy_site_len: 0,
            _marker: core::marker::PhantomData,
        }
    }
}

impl Iterator for ProgramImageResourceIter<'_> {
    type Item = ResourceDescriptor;

    fn next(&mut self) -> Option<Self::Item> {
        while self.row < self.program.atom_row_count() {
            let row = self.row;
            self.row += 1;
            let offset = self.program.atom_eff_at_row(row)?;
            let policy = self
                .program
                .resident_policy_at(offset)
                .unwrap_or(ResolverMode::Static);
            if policy.is_dynamic() {
                self.dynamic_policy_site_len = self.dynamic_policy_site_len.saturating_add(1);
            }
            let resource_policy_site = ResourceDescriptor::STATIC_POLICY_SITE;
            let atom = self
                .program
                .atom_at(offset)
                .expect("atom row offset must resolve to an atom");
            if !atom.is_control {
                continue;
            }
            let resource_kind_tag = atom
                .resource
                .expect("control atom must carry a resource tag");
            let control_desc = self
                .program
                .resident_control_desc_at(offset)
                .expect("control atom missing control descriptor");
            if control_desc.resource_tag() != resource_kind_tag {
                panic!("control atom/control descriptor mismatch");
            }
            return Some(ResourceDescriptor::new(control_desc.with_sites(
                EffIndex::from_dense_ordinal(offset),
                resource_policy_site,
            )));
        }
        None
    }
}

/// Metadata describing a projected control descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResourceDescriptor {
    control: ControlDesc,
}

impl ResourceDescriptor {
    pub(crate) const STATIC_POLICY_SITE: u16 = ControlDesc::STATIC_POLICY_SITE;

    #[inline(always)]
    pub(crate) const fn new(control: ControlDesc) -> Self {
        Self { control }
    }

    #[inline(always)]
    pub(crate) const fn eff_index(&self) -> EffIndex {
        self.control.eff_index()
    }

    #[inline(always)]
    pub(crate) const fn tag(&self) -> u8 {
        self.control.resource_tag()
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[path = "effects/tests.rs"]
mod tests;
