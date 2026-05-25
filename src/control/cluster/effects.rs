//! Projected control-resource metadata helpers.

#[cfg(test)]
use core::{mem::MaybeUninit, ptr};

use crate::{
    control::cap::mint::ControlOp,
    eff::EffIndex,
    global::{
        ControlDesc,
        compiled::{images::DynamicPolicySite, lowering::CompiledProgramImage},
        const_dsl::{ControlScopeKind, PolicyMode},
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
        ControlOp::RouteDecision => ids::ROUTE_DECISION,
        ControlOp::LoopContinue | ControlOp::LoopBreak => ids::LOOP_DECISION,
        ControlOp::StateSnapshot => ids::STATE_SNAPSHOT_REQ,
        ControlOp::StateRestore => ids::STATE_RESTORE_REQ,
        ControlOp::TopologyBegin => ids::TOPOLOGY_BEGIN,
        ControlOp::TopologyAck => ids::TOPOLOGY_ACK,
        ControlOp::TopologyCommit => ids::TOPOLOGY_COMMIT,
        ControlOp::CapDelegate => ids::DELEG_BEGIN,
        ControlOp::AbortBegin => ids::ABORT_BEGIN,
        ControlOp::AbortAck => ids::ABORT_ACK,
        ControlOp::Fence => ids::POLICY_RA_OK,
        ControlOp::TxCommit => ids::POLICY_COMMIT,
        ControlOp::TxAbort => ids::POLICY_TX_ABORT,
    }
}

#[cfg(test)]
#[inline(always)]
pub(crate) const fn control_op_is_idempotent(op: ControlOp) -> bool {
    matches!(
        op,
        ControlOp::TopologyAck | ControlOp::StateSnapshot | ControlOp::Fence | ControlOp::AbortAck
    )
}

#[cfg(test)]
#[inline(always)]
pub(crate) const fn control_op_requires_gen_bump(op: ControlOp) -> bool {
    matches!(op, ControlOp::TopologyCommit | ControlOp::TxCommit)
}

#[cfg(test)]
#[inline(always)]
pub(crate) const fn control_op_is_terminal(op: ControlOp) -> bool {
    matches!(op, ControlOp::TxCommit | ControlOp::TxAbort)
}

#[cfg(test)]
#[inline(always)]
pub(crate) const fn control_op_modifies_history(op: ControlOp) -> bool {
    matches!(
        op,
        ControlOp::StateSnapshot | ControlOp::StateRestore | ControlOp::TxAbort
    )
}

/// Projected effects from global protocol ready for runtime execution (no_alloc).
///
/// This structure contains the flattened/optimized representation of effects
/// derived from interpreting a global protocol's Eff tree. It serves as the
/// output of the EffInterpreter and input to rendezvous initialization.
///
/// Uses fixed-size arrays for no_std/no_alloc execution.
///
/// Note: This is distinct from `control::cluster::CpCommand`, which wraps
/// individual effect executions with their runtime operands.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct EffectEnvelope {
    /// Tap events to emit during execution.
    #[cfg(test)]
    tap_events: [core::mem::MaybeUninit<u16>; Self::MAX_TAP_EVENTS],
    #[cfg(test)]
    tap_events_len: usize,

    /// Resource descriptors associated with control operations.
    #[cfg(test)]
    resources: [core::mem::MaybeUninit<ResourceDescriptor>; Self::MAX_RESOURCES],
    #[cfg(test)]
    resources_len: usize,
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
        while self.next < 7 {
            let bit = 1u8 << self.next;
            let scope_kind = match self.next {
                0 => ControlScopeKind::Loop,
                1 => ControlScopeKind::State,
                2 => ControlScopeKind::Abort,
                3 => ControlScopeKind::Topology,
                4 => ControlScopeKind::Delegate,
                5 => ControlScopeKind::Policy,
                6 => ControlScopeKind::Route,
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
enum EffectEnvelopeSource<'a> {
    #[cfg(test)]
    Slices {
        tap_events: &'a [u16],
        resources: &'a [ResourceDescriptor],
        dynamic_policy_sites: &'a [DynamicPolicySite],
        control_scope_mask: u8,
    },
    ProgramImage(&'a CompiledProgramImage),
}

#[derive(Clone, Copy)]
pub(crate) struct EffectEnvelopeRef<'a> {
    source: EffectEnvelopeSource<'a>,
}

impl<'a> EffectEnvelopeRef<'a> {
    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn new(
        tap_events: &'a [u16],
        resources: &'a [ResourceDescriptor],
        dynamic_policy_sites: &'a [DynamicPolicySite],
        control_scope_mask: u8,
    ) -> Self {
        Self {
            source: EffectEnvelopeSource::Slices {
                #[cfg(test)]
                tap_events,
                resources,
                dynamic_policy_sites,
                control_scope_mask,
            },
        }
    }

    #[inline(always)]
    pub(crate) const fn from_program_image(image: &'a CompiledProgramImage) -> Self {
        Self {
            source: EffectEnvelopeSource::ProgramImage(image),
        }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn is_empty(&self) -> bool {
        match self.source {
            #[cfg(test)]
            EffectEnvelopeSource::Slices {
                tap_events,
                resources,
                dynamic_policy_sites,
                control_scope_mask,
            } => {
                tap_events.is_empty()
                    && resources.is_empty()
                    && dynamic_policy_sites.is_empty()
                    && control_scope_mask == 0
            }
            EffectEnvelopeSource::ProgramImage(image) => {
                ProgramImageResourceIter::new(image).next().is_none()
                    && image.compiled_program_control_scope_mask() == 0
            }
        }
    }

    #[inline(always)]
    pub(crate) fn resources(&self) -> ResourceIter<'a> {
        match self.source {
            #[cfg(test)]
            EffectEnvelopeSource::Slices { resources, .. } => {
                ResourceIter::Slices(resources.iter().copied())
            }
            EffectEnvelopeSource::ProgramImage(image) => {
                ResourceIter::ProgramImage(ProgramImageResourceIter::new(image))
            }
        }
    }

    #[inline(always)]
    pub(crate) fn resource_policy(&self, descriptor: &ResourceDescriptor) -> PolicyMode {
        match self.source {
            #[cfg(test)]
            EffectEnvelopeSource::Slices {
                dynamic_policy_sites,
                ..
            } => descriptor.policy(dynamic_policy_sites),
            EffectEnvelopeSource::ProgramImage(image) => {
                let policy_site = descriptor.control.policy_site();
                if policy_site == ResourceDescriptor::STATIC_POLICY_SITE {
                    PolicyMode::Static
                } else {
                    ProgramImageDynamicPolicySiteIter::new(image)
                        .nth(policy_site as usize)
                        .map(|site| site.policy())
                        .unwrap_or(PolicyMode::Static)
                }
            }
        }
    }

    #[inline(always)]
    pub(crate) fn control_scopes(&self) -> impl Iterator<Item = ControlScopeKind> {
        let mask = match self.source {
            #[cfg(test)]
            EffectEnvelopeSource::Slices {
                control_scope_mask, ..
            } => control_scope_mask,
            EffectEnvelopeSource::ProgramImage(image) => {
                image.compiled_program_control_scope_mask()
            }
        };
        ControlScopeIter::new(mask)
    }
}

pub(crate) enum ResourceIter<'a> {
    #[cfg(test)]
    Slices(core::iter::Copied<core::slice::Iter<'a, ResourceDescriptor>>),
    ProgramImage(ProgramImageResourceIter<'a>),
}

impl Iterator for ResourceIter<'_> {
    type Item = ResourceDescriptor;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            #[cfg(test)]
            Self::Slices(iter) => iter.next(),
            Self::ProgramImage(iter) => iter.next(),
        }
    }
}

pub(crate) struct ProgramImageDynamicPolicySiteIter<'a> {
    image: &'a CompiledProgramImage,
    offset: usize,
}

impl<'a> ProgramImageDynamicPolicySiteIter<'a> {
    #[inline(always)]
    pub(crate) const fn new(image: &'a CompiledProgramImage) -> Self {
        Self { image, offset: 0 }
    }
}

impl Iterator for ProgramImageDynamicPolicySiteIter<'_> {
    type Item = DynamicPolicySite;

    fn next(&mut self) -> Option<Self::Item> {
        let view = self.image.view();
        while self.offset < view.len() {
            let offset = self.offset;
            self.offset += 1;
            let Some(policy) = view.policy_at(offset) else {
                continue;
            };
            if !policy.is_dynamic() {
                continue;
            }
            let node = view.node_at(offset);
            if !matches!(node.kind, crate::eff::EffKind::Atom) {
                continue;
            }
            let atom = node.atom_data();
            let control = view.control_desc_at(offset);
            return Some(DynamicPolicySite::new(
                EffIndex::from_dense_ordinal(offset),
                atom.label,
                atom.resource,
                control.map(ControlDesc::op),
                policy,
            ));
        }
        None
    }
}

pub(crate) struct ProgramImageResourceIter<'a> {
    image: &'a CompiledProgramImage,
    offset: usize,
    dynamic_policy_site_len: u16,
}

impl<'a> ProgramImageResourceIter<'a> {
    #[inline(always)]
    const fn new(image: &'a CompiledProgramImage) -> Self {
        Self {
            image,
            offset: 0,
            dynamic_policy_site_len: 0,
        }
    }
}

impl Iterator for ProgramImageResourceIter<'_> {
    type Item = ResourceDescriptor;

    fn next(&mut self) -> Option<Self::Item> {
        let view = self.image.view();
        while self.offset < view.len() {
            let offset = self.offset;
            self.offset += 1;
            let node = view.node_at(offset);
            let policy = view.policy_at(offset).unwrap_or(PolicyMode::Static);
            let resource_policy_site = if policy.is_dynamic() {
                let site = self.dynamic_policy_site_len;
                self.dynamic_policy_site_len = self.dynamic_policy_site_len.saturating_add(1);
                site
            } else {
                ResourceDescriptor::STATIC_POLICY_SITE
            };
            if !matches!(node.kind, crate::eff::EffKind::Atom) {
                continue;
            }
            let atom = node.atom_data();
            if !atom.is_control {
                continue;
            }
            let resource_kind_tag = atom
                .resource
                .expect("control atom must carry a resource tag");
            let control_desc = view
                .control_desc_at(offset)
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

/// Metadata describing a control resource discovered during projection.
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

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn op(&self) -> ControlOp {
        self.control.op()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn policy(&self, dynamic_policy_sites: &[DynamicPolicySite]) -> PolicyMode {
        if self.control.policy_site() == Self::STATIC_POLICY_SITE {
            PolicyMode::Static
        } else {
            dynamic_policy_sites[self.control.policy_site() as usize].policy()
        }
    }
}

#[cfg(test)]
impl EffectEnvelope {
    /// Maximum number of tap events per projection.
    pub(crate) const MAX_TAP_EVENTS: usize = 512;

    /// Maximum number of resource handles per projection.
    pub(crate) const MAX_RESOURCES: usize = 128;

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn as_ref(&self) -> EffectEnvelopeRef<'_> {
        self.as_ref_with_controls(0, &[])
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn as_ref_with_controls<'a>(
        &'a self,
        control_scope_mask: u8,
        dynamic_policy_sites: &'a [DynamicPolicySite],
    ) -> EffectEnvelopeRef<'a> {
        EffectEnvelopeRef::new(
            /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
            unsafe {
                core::slice::from_raw_parts(
                    self.tap_events.as_ptr().cast::<u16>(),
                    self.tap_events_len,
                )
            },
            /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
            unsafe {
                core::slice::from_raw_parts(
                    self.resources.as_ptr().cast::<ResourceDescriptor>(),
                    self.resources_len,
                )
            },
            dynamic_policy_sites,
            control_scope_mask,
        )
    }

    #[cfg(test)]
    #[inline(always)]
    unsafe fn zero_maybe_uninit_array<T, const N: usize>(dst: *mut [MaybeUninit<T>; N]) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::write_bytes(
                dst.cast::<u8>(),
                0,
                core::mem::size_of::<[MaybeUninit<T>; N]>(),
            );
        }
    }

    /// Create an empty projection.
    #[cfg(test)]
    #[inline(never)]
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            Self::zero_maybe_uninit_array(ptr::addr_of_mut!((*dst).tap_events));
            ptr::addr_of_mut!((*dst).tap_events_len).write(0);
            Self::zero_maybe_uninit_array(ptr::addr_of_mut!((*dst).resources));
            ptr::addr_of_mut!((*dst).resources_len).write(0);
        }
    }

    /// Create an empty projection.
    #[cfg(test)]
    fn empty() -> Self {
        let mut envelope = MaybeUninit::<Self>::uninit();
        /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
        unsafe {
            Self::init_empty(envelope.as_mut_ptr());
            envelope.assume_init()
        }
    }

    /// Check if this projection has any effects to execute.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }

    /// Iterate over tap events.
    #[cfg(test)]
    pub(crate) fn tap_events(&self) -> impl Iterator<Item = u16> + '_ {
        (0..self.tap_events_len).map(move |i| /* SAFETY: the table owner tracks the initialized prefix and checks this slot before reading initialized storage. */ unsafe { *self.tap_events[i].assume_init_ref() })
    }

    /// Iterate over resource handles.
    #[cfg(test)]
    pub(crate) fn resources(&self) -> impl Iterator<Item = &ResourceDescriptor> {
        (0..self.resources_len).map(move |i| /* SAFETY: the table owner tracks the initialized prefix and checks this slot before reading initialized storage. */ unsafe { self.resources[i].assume_init_ref() })
    }
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;
    use std::vec::Vec;

    use super::*;
    use crate::control::cap::mint::{GenericCapToken, ResourceKind};
    use crate::observe::ids;
    use crate::test_support::snapshot_control::{SNAPSHOT_CONTROL_LOGICAL, SnapshotControl};

    #[test]
    fn resource_descriptor_stays_compact() {
        assert!(
            size_of::<ResourceDescriptor>() <= 16,
            "ResourceDescriptor regressed to a wide unused-field layout: {} bytes",
            size_of::<ResourceDescriptor>()
        );
    }

    #[test]
    fn test_effect_properties() {
        assert_eq!(
            control_op_tap_event_id(ControlOp::TopologyAck),
            ids::TOPOLOGY_ACK
        );
        assert_eq!(
            control_op_tap_event_id(ControlOp::CapDelegate),
            ids::DELEG_BEGIN
        );
        assert_eq!(
            control_op_tap_event_id(ControlOp::TxAbort),
            ids::POLICY_TX_ABORT
        );
        assert!(control_op_is_idempotent(ControlOp::TopologyAck));
        assert!(!control_op_is_idempotent(ControlOp::TopologyBegin));
        assert!(control_op_is_idempotent(ControlOp::StateSnapshot));
        assert!(!control_op_is_idempotent(ControlOp::StateRestore));

        assert!(control_op_requires_gen_bump(ControlOp::TopologyCommit));
        assert!(!control_op_requires_gen_bump(ControlOp::TopologyBegin));

        assert!(control_op_is_terminal(ControlOp::TxCommit));
        assert!(control_op_is_terminal(ControlOp::TxAbort));
        assert!(!control_op_is_terminal(ControlOp::Fence));

        assert!(control_op_modifies_history(ControlOp::StateSnapshot));
        assert!(control_op_modifies_history(ControlOp::StateRestore));
        assert!(control_op_modifies_history(ControlOp::TxAbort));
        assert!(!control_op_modifies_history(ControlOp::TxCommit));
    }

    #[test]
    fn fence_performs_no_state_mutation() {
        assert!(!control_op_requires_gen_bump(ControlOp::Fence));
        assert!(!control_op_is_terminal(ControlOp::Fence));
        assert!(!control_op_modifies_history(ControlOp::Fence));
    }

    #[test]
    fn test_empty_projected_effects() {
        let projected = EffectEnvelope::empty();
        assert!(projected.is_empty());
        assert_eq!(projected.tap_events().count(), 0);
        assert_eq!(projected.resources().count(), 0);
    }

    #[test]
    fn test_interpreter_pure() {
        let program = crate::g::Program::<crate::global::steps::StepNil>::empty();
        let projected: crate::integration::program::RoleProgram<0> =
            crate::integration::program::project(&program);
        let program = projected.compiled_role_image().program();
        let effects = program.effect_envelope();
        assert!(effects.is_empty());
    }

    #[test]
    fn test_interpreter_control_atom() {
        // Local control messages require self-send (From == To)
        let program = crate::g::send::<
            crate::g::Role<0>,
            crate::g::Role<0>,
            crate::g::Msg<
                { SNAPSHOT_CONTROL_LOGICAL },
                GenericCapToken<SnapshotControl>,
                SnapshotControl,
            >,
            0,
        >();
        let projected: crate::integration::program::RoleProgram<0> =
            crate::integration::program::project(&program);
        let program = projected.compiled_role_image().program();
        let effects = program.effect_envelope();
        let resources: Vec<_> = effects.resources().collect();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].tag(), SnapshotControl::TAG);
        assert_eq!(resources[0].op(), ControlOp::StateSnapshot);
    }
}
