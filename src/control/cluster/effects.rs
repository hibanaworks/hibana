//! Control-plane effects enumeration.
//!
//! All control operations (lane open, splice, delegate, cancel, fence, commit, abort)
//! are projected into this enum. Composite operations are expressed through effect composition.

#[cfg(test)]
use core::{mem::MaybeUninit, ptr};

use crate::{
    eff::EffIndex,
    global::{
        compiled::DynamicPolicySite,
        const_dsl::{ControlScopeKind, PolicyMode},
    },
};

/// Control-plane effect primitive.
///
/// This enum represents the atomic effects that the control-plane can perform.
/// All higher-level operations (splice, delegation, cancellation, etc.) are composed
/// from these primitives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CpEffect {
    /// Open a new lane
    Open = 0,

    /// Begin a splice operation.
    SpliceBegin = 1,

    /// Acknowledge a splice operation.
    SpliceAck = 2,

    /// Commit a splice operation.
    SpliceCommit = 3,

    /// Delegate a session/capability
    Delegate = 4,

    /// Begin cancellation
    CancelBegin = 5,

    /// Acknowledge cancellation
    CancelAck = 6,

    /// Fence operation (synchronization barrier)
    Fence = 7,

    /// Commit transaction
    Commit = 8,

    /// Abort transaction
    Abort = 9,

    /// Checkpoint operation (save state)
    Checkpoint = 10,

    /// Rollback to checkpoint
    Rollback = 11,
}

impl CpEffect {
    pub(crate) const fn bit(self) -> u16 {
        1 << (self as u16)
    }

    /// Returns true if this effect is idempotent.
    ///
    /// Idempotent effects can be safely replayed without changing state.
    #[cfg(test)]
    pub(crate) const fn is_idempotent(self) -> bool {
        matches!(
            self,
            Self::SpliceAck | Self::CancelAck | Self::Fence | Self::Abort | Self::Checkpoint
        )
    }

    /// Returns true if this effect requires generation bump.
    #[cfg(test)]
    pub(crate) const fn requires_gen_bump(self) -> bool {
        matches!(self, Self::SpliceCommit | Self::Commit)
    }

    /// Returns true if this effect is terminal (closes a transaction).
    #[cfg(test)]
    pub(crate) const fn is_terminal(self) -> bool {
        matches!(self, Self::Commit | Self::Abort)
    }

    /// Returns true if this effect modifies state history.
    #[cfg(test)]
    pub(crate) const fn modifies_history(self) -> bool {
        matches!(self, Self::Checkpoint | Self::Rollback)
    }

    /// Convert CpEffect to tap event ID.
    ///
    /// Maps internal CpEffect enum values onto the stable observable tap-event IDs.
    pub(crate) const fn to_tap_event_id(self) -> u16 {
        use crate::observe::ids;
        match self {
            Self::Open => 0x0100,         // New event ID for lane open
            Self::SpliceBegin => 0x0110,  // New event ID for splice begin
            Self::SpliceAck => 0x0111,    // New event ID for splice ack
            Self::SpliceCommit => 0x0112, // New event ID for splice commit
            Self::Delegate => 0x0120,     // New event ID for delegate
            Self::CancelBegin => ids::CANCEL_BEGIN,
            Self::CancelAck => ids::CANCEL_ACK,
            Self::Fence => 0x0140,  // New event ID for fence
            Self::Commit => 0x0150, // New event ID for commit
            Self::Abort => 0x0151,  // New event ID for abort
            Self::Checkpoint => ids::CHECKPOINT_REQ,
            Self::Rollback => ids::ROLLBACK_REQ,
        }
    }

    /// Map ResourceKind TAG to CpEffect.
    ///
    /// This function converts control message ResourceKind tags into their corresponding
    /// control-plane effects. Returns None for ResourceKinds that don't map to effects
    /// (e.g., loop decisions which are handled separately).
    ///
    /// TAG ranges (from resource_kinds.rs):
    /// - 0x40: LoopContinue (no CpEffect, handled by LoopTable)
    /// - 0x41: LoopBreak (no CpEffect, handled by LoopTable)
    /// - 0x42: Checkpoint → Checkpoint
    /// - 0x43: Commit → Commit
    /// - 0x44: Rollback → Rollback
    /// - 0x45: Cancel → CancelBegin
    /// - 0x46: CancelAck → CancelAck
    /// - 0x47: SpliceIntent → SpliceBegin
    /// - 0x48: SpliceAck → SpliceAck
    /// - 0x49: Reroute (complex, may need special handling)
    /// - 0x4A-0x4E: Policy ops (no direct CpEffect)
    pub(crate) const fn from_resource_tag(tag: u8) -> Option<Self> {
        match tag {
            0x42 => Some(Self::Checkpoint),
            0x43 => Some(Self::Commit),
            0x44 => Some(Self::Rollback),
            0x45 => Some(Self::CancelBegin),
            0x46 => Some(Self::CancelAck),
            0x47 => Some(Self::SpliceBegin),
            0x48 => Some(Self::SpliceAck),
            0x49 => Some(Self::Delegate),
            _ => None, // Loop decisions (0x40-0x41), Policy ops (0x4A-0x4E)
        }
    }
}

/// Projected effects from global protocol ready for runtime execution (no_alloc).
///
/// This structure contains the flattened/optimized representation of effects
/// derived from interpreting a global protocol's Eff tree. It serves as the
/// output of the EffInterpreter and input to rendezvous initialization.
///
/// Uses fixed-size arrays for no_std/no_alloc compatibility.
///
/// Note: This is distinct from `control::cluster::CpCommand`, which wraps
/// individual effect executions with their runtime operands.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct EffectEnvelope {
    /// Sequence of control-plane effects to execute.
    #[cfg(test)]
    cp_effects: [core::mem::MaybeUninit<CpEffect>; Self::MAX_CP_EFFECTS],
    #[cfg(test)]
    cp_effects_len: usize,

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
                1 => ControlScopeKind::Checkpoint,
                2 => ControlScopeKind::Cancel,
                3 => ControlScopeKind::Splice,
                4 => ControlScopeKind::Reroute,
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
    cp_effects: &'a [CpEffect],
    #[cfg(test)]
    tap_events: &'a [u16],
    resources: &'a [ResourceDescriptor],
    dynamic_policy_sites: &'a [DynamicPolicySite],
    control_scope_mask: u8,
}

impl<'a> EffectEnvelopeRef<'a> {
    #[inline(always)]
    pub(crate) const fn new(
        #[cfg(test)] cp_effects: &'a [CpEffect],
        #[cfg(not(test))] _cp_effects: &'a [CpEffect],
        #[cfg(test)] tap_events: &'a [u16],
        #[cfg(not(test))] _tap_events: &'a [u16],
        resources: &'a [ResourceDescriptor],
        dynamic_policy_sites: &'a [DynamicPolicySite],
        control_scope_mask: u8,
    ) -> Self {
        Self {
            #[cfg(test)]
            cp_effects,
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
        self.cp_effects.is_empty()
            && self.tap_events.is_empty()
            && self.resources.is_empty()
            && self.dynamic_policy_sites.is_empty()
            && self.control_scope_mask == 0
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn cp_effects(&self) -> impl Iterator<Item = &CpEffect> {
        let _ = self.tap_events.len();
        self.cp_effects.iter()
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
    /// Effect index associated with the control atom.
    eff_index: EffIndex,
    /// Dynamic-policy site index, or [`Self::STATIC_POLICY_SITE`] for static policy.
    policy_site: u16,
    /// Label associated with the control message.
    label: u8,
    /// Resource kind tag (maps to [`crate::control::cap::resource_kinds`]).
    tag: u8,
}

impl ResourceDescriptor {
    pub(crate) const STATIC_POLICY_SITE: u16 = u16::MAX;

    #[inline(always)]
    pub(crate) const fn new(eff_index: EffIndex, label: u8, tag: u8, policy_site: u16) -> Self {
        Self {
            eff_index,
            policy_site,
            label,
            tag,
        }
    }

    #[inline(always)]
    pub(crate) const fn eff_index(&self) -> EffIndex {
        self.eff_index
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) const fn label(&self) -> u8 {
        self.label
    }

    #[inline(always)]
    pub(crate) const fn tag(&self) -> u8 {
        self.tag
    }

    #[inline(always)]
    pub(crate) fn policy(&self, dynamic_policy_sites: &[DynamicPolicySite]) -> PolicyMode {
        if self.policy_site == Self::STATIC_POLICY_SITE {
            PolicyMode::Static
        } else {
            dynamic_policy_sites[self.policy_site as usize].policy()
        }
    }
}

#[cfg(test)]
impl EffectEnvelope {
    /// Maximum number of control-plane effects per projection.
    /// Conservative upper bound based on typical protocol complexity.
    pub(crate) const MAX_CP_EFFECTS: usize = 256;

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
                    self.cp_effects.as_ptr().cast::<CpEffect>(),
                    self.cp_effects_len,
                )
            },
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
            Self::zero_maybe_uninit_array(ptr::addr_of_mut!((*dst).cp_effects));
            ptr::addr_of_mut!((*dst).cp_effects_len).write(0);
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

    /// Push a control-plane effect.
    #[cfg(test)]
    pub(crate) const fn push_cp_effect(&mut self, effect: CpEffect) {
        if self.cp_effects_len >= Self::MAX_CP_EFFECTS {
            panic!("EffectEnvelope: MAX_CP_EFFECTS exceeded");
        }
        self.cp_effects[self.cp_effects_len] = core::mem::MaybeUninit::new(effect);
        self.cp_effects_len += 1;
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
    pub(crate) const fn push_resource(
        &mut self,
        eff_index: EffIndex,
        label: u8,
        kind_tag: u8,
        policy_site: u16,
    ) {
        if self.resources_len >= Self::MAX_RESOURCES {
            panic!("EffectEnvelope: MAX_RESOURCES exceeded");
        }
        self.resources[self.resources_len] = core::mem::MaybeUninit::new(ResourceDescriptor::new(
            eff_index,
            label,
            kind_tag,
            policy_site,
        ));
        self.resources_len += 1;
    }

    /// Iterate over control-plane effects.
    #[cfg(test)]
    pub(crate) fn cp_effects(&self) -> impl Iterator<Item = &CpEffect> {
        (0..self.cp_effects_len).map(move |i| unsafe { self.cp_effects[i].assume_init_ref() })
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

    use super::*;
    use crate::global::CanonicalControl;
    use crate::global::compiled::{CompiledProgram, LoweringSummary};
    use crate::global::const_dsl::EffList;

    #[test]
    fn resource_descriptor_stays_compact() {
        assert!(
            size_of::<ResourceDescriptor>() <= 8,
            "ResourceDescriptor regressed to a wide unused-field layout: {} bytes",
            size_of::<ResourceDescriptor>()
        );
    }

    #[test]
    fn test_effect_properties() {
        assert!(CpEffect::SpliceAck.is_idempotent());
        assert!(!CpEffect::SpliceBegin.is_idempotent());
        assert!(CpEffect::Checkpoint.is_idempotent());

        assert!(CpEffect::SpliceCommit.requires_gen_bump());
        assert!(!CpEffect::SpliceBegin.requires_gen_bump());

        assert!(CpEffect::Commit.is_terminal());
        assert!(CpEffect::Abort.is_terminal());
        assert!(!CpEffect::Fence.is_terminal());

        assert!(CpEffect::Checkpoint.modifies_history());
        assert!(CpEffect::Rollback.modifies_history());
        assert!(!CpEffect::Commit.modifies_history());
    }

    #[test]
    fn test_empty_projected_effects() {
        let projected = EffectEnvelope::empty();
        assert!(projected.is_empty());
        assert_eq!(projected.cp_effects().count(), 0);
        assert_eq!(projected.tap_events().count(), 0);
        assert_eq!(projected.resources().count(), 0);
    }

    #[test]
    fn test_interpreter_pure() {
        let program = EffList::new();
        let summary = LoweringSummary::scan_const(&program);
        let facts = CompiledProgram::from_summary(&summary);
        let projected = facts.effect_envelope();
        assert!(projected.is_empty());
    }

    #[test]
    fn test_interpreter_control_atom() {
        // CanonicalControl messages require self-send (From == To)
        use crate::control::cap::mint::GenericCapToken;
        use crate::control::cap::resource_kinds::CheckpointKind;

        let program = crate::g::send::<
            crate::g::Role<0>,
            crate::g::Role<0>,
            crate::g::Msg<
                { crate::runtime::consts::LABEL_CHECKPOINT },
                GenericCapToken<CheckpointKind>,
                CanonicalControl<CheckpointKind>,
            >,
            0,
        >();
        let eff_list = program.into_eff();
        let summary = LoweringSummary::scan_const(&eff_list);
        let facts = CompiledProgram::from_summary(&summary);
        let projected = facts.effect_envelope();
        assert_eq!(projected.cp_effects().count(), 1);
        assert!(projected.tap_events().count() >= 1);
    }
}
