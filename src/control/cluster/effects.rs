//! Projected control-resource metadata helpers.

#[cfg(test)]
use core::{mem::MaybeUninit, ptr};

use crate::{
    control::cap::mint::ControlOp,
    eff::EffIndex,
    global::{
        ControlDesc,
        compiled::images::DynamicPolicySite,
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
pub(crate) struct EffectEnvelopeRef<'a> {
    #[cfg(test)]
    tap_events: &'a [u16],
    resources: &'a [ResourceDescriptor],
    dynamic_policy_sites: &'a [DynamicPolicySite],
    control_scope_mask: u8,
}

impl<'a> EffectEnvelopeRef<'a> {
    #[inline(always)]
    pub(crate) const fn new(
        #[cfg(test)] tap_events: &'a [u16],
        #[cfg(not(test))] _tap_events: &'a [u16],
        resources: &'a [ResourceDescriptor],
        dynamic_policy_sites: &'a [DynamicPolicySite],
        control_scope_mask: u8,
    ) -> Self {
        Self {
            #[cfg(test)]
            tap_events,
            resources,
            dynamic_policy_sites,
            control_scope_mask,
        }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn is_empty(&self) -> bool {
        self.tap_events.is_empty()
            && self.resources.is_empty()
            && self.dynamic_policy_sites.is_empty()
            && self.control_scope_mask == 0
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn tap_events(&self) -> impl Iterator<Item = u16> + '_ {
        self.tap_events.iter().copied()
    }

    #[inline(always)]
    pub(crate) fn resources(&self) -> impl Iterator<Item = &ResourceDescriptor> {
        self.resources.iter()
    }

    #[inline(always)]
    pub(crate) fn resource_policy(&self, descriptor: &ResourceDescriptor) -> PolicyMode {
        descriptor.policy(self.dynamic_policy_sites)
    }

    #[inline(always)]
    pub(crate) fn control_scopes(&self) -> impl Iterator<Item = ControlScopeKind> {
        ControlScopeIter::new(self.control_scope_mask)
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
            unsafe {
                core::slice::from_raw_parts(
                    self.tap_events.as_ptr().cast::<u16>(),
                    self.tap_events_len,
                )
            },
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

    /// Push a tap event ID.
    #[cfg(test)]
    pub(crate) const fn push_tap_event(&mut self, event_id: u16) {
        if self.tap_events_len >= Self::MAX_TAP_EVENTS {
            panic!("EffectEnvelope: MAX_TAP_EVENTS exceeded");
        }
        self.tap_events[self.tap_events_len] = core::mem::MaybeUninit::new(event_id);
        self.tap_events_len += 1;
    }

    /// Push a resource descriptor.
    #[cfg(test)]
    pub(crate) const fn push_resource(&mut self, descriptor: ResourceDescriptor) {
        if self.resources_len >= Self::MAX_RESOURCES {
            panic!("EffectEnvelope: MAX_RESOURCES exceeded");
        }
        self.resources[self.resources_len] = core::mem::MaybeUninit::new(descriptor);
        self.resources_len += 1;
    }

    /// Iterate over tap events.
    #[cfg(test)]
    pub(crate) fn tap_events(&self) -> impl Iterator<Item = u16> + '_ {
        (0..self.tap_events_len).map(move |i| unsafe { *self.tap_events[i].assume_init_ref() })
    }

    /// Iterate over resource handles.
    #[cfg(test)]
    pub(crate) fn resources(&self) -> impl Iterator<Item = &ResourceDescriptor> {
        (0..self.resources_len).map(move |i| unsafe { self.resources[i].assume_init_ref() })
    }
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;
    use std::vec::Vec;

    mod snapshot_control_kind {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/snapshot_control.rs"
        ));
    }

    use super::*;
    use crate::control::cap::mint::{GenericCapToken, ResourceKind};
    use crate::global::role_program::lowering_input;
    use crate::observe::ids;
    use snapshot_control_kind::{LABEL_SNAPSHOT_CONTROL, SnapshotControl};

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
        let projected: crate::substrate::program::RoleProgram<0> =
            crate::substrate::program::project(&program);
        crate::global::compiled::materialize::with_compiled_program(
            lowering_input(&projected),
            |facts| {
                let projected = facts.effect_envelope();
                assert!(projected.is_empty());
            },
        );
    }

    #[test]
    fn test_interpreter_control_atom() {
        // Local control messages require self-send (From == To)
        let program = crate::g::send::<
            crate::g::Role<0>,
            crate::g::Role<0>,
            crate::g::Msg<
                { LABEL_SNAPSHOT_CONTROL },
                GenericCapToken<SnapshotControl>,
                SnapshotControl,
            >,
            0,
        >();
        let projected: crate::substrate::program::RoleProgram<0> =
            crate::substrate::program::project(&program);
        crate::global::compiled::materialize::with_compiled_program(
            lowering_input(&projected),
            |facts| {
                let projected = facts.effect_envelope();
                assert!(projected.tap_events().count() >= 1);
                let resources: Vec<_> = projected.resources().collect();
                assert_eq!(resources.len(), 1);
                assert_eq!(resources[0].tag(), SnapshotControl::TAG);
                assert_eq!(resources[0].op(), ControlOp::StateSnapshot);
            },
        );
    }
}
