use core::{ptr, ptr::NonNull};

#[cfg(test)]
use crate::control::cluster::effects::EffectEnvelope;
#[cfg(test)]
use crate::control::lease::planner::LeaseGraphBudget;
use crate::{
    control::{
        cap::mint::ResourceKind,
        cap::resource_kinds::{
            LoopBreakKind, LoopContinueKind, RerouteKind, RouteDecisionKind, SpliceAckKind,
            SpliceIntentKind,
        },
        cluster::effects::{EffectEnvelopeRef, ResourceDescriptor},
    },
    eff::{EffAtom, EffIndex, EffKind},
    global::{
        ControlLabelSpec,
        const_dsl::{
            CompactScopeId, ControlScopeKind, PolicyMode, ScopeEvent, ScopeId, ScopeKind,
            ScopeMarker,
        },
    },
    runtime::consts::{
        LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE, LABEL_REROUTE, LABEL_ROUTE_DECISION,
        LABEL_SPLICE_ACK, LABEL_SPLICE_INTENT,
    },
};

#[cfg(test)]
use crate::control::cluster::effects::CpEffect;

use super::LoweringSummary;

/// Precomputed dynamic policy site discovered during program lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DynamicPolicySite {
    eff_index: EffIndex,
    label: u8,
    resource_tag: Option<u8>,
    policy: PolicyMode,
}

impl DynamicPolicySite {
    #[cfg(test)]
    const EMPTY: Self = Self {
        eff_index: EffIndex::ZERO,
        label: 0,
        resource_tag: None,
        policy: PolicyMode::Static,
    };

    #[inline(always)]
    const fn new(
        eff_index: EffIndex,
        label: u8,
        resource_tag: Option<u8>,
        policy: PolicyMode,
    ) -> Self {
        Self {
            eff_index,
            label,
            resource_tag,
            policy,
        }
    }

    #[inline(always)]
    pub(crate) const fn eff_index(&self) -> EffIndex {
        self.eff_index
    }

    #[inline(always)]
    pub(crate) const fn label(&self) -> u8 {
        self.label
    }

    #[inline(always)]
    pub(crate) const fn resource_tag(&self) -> Option<u8> {
        self.resource_tag
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
    route_policy_id: u16,
    route_policy_eff: EffIndex,
}

impl RouteControlRecord {
    #[cfg(test)]
    const EMPTY: Self = Self {
        scope_id: CompactScopeId::none(),
        controller_role: ROUTE_CONTROL_NONE,
        route_policy_tag: 0,
        route_policy_id: u16::MAX,
        route_policy_eff: EffIndex::MAX,
    };

    #[inline(always)]
    const fn new(
        scope_id: ScopeId,
        controller_role: Option<u8>,
        route_policy_id: u16,
        route_policy_eff: EffIndex,
        route_policy_tag: u8,
    ) -> Self {
        Self {
            scope_id: CompactScopeId::from_scope_id(scope_id),
            controller_role: match controller_role {
                Some(role) => role,
                None => ROUTE_CONTROL_NONE,
            },
            route_policy_tag,
            route_policy_id,
            route_policy_eff,
        }
    }

    #[inline(always)]
    const fn canonical_raw(self) -> u64 {
        self.scope_id.canonical().raw()
    }

    #[inline(always)]
    const fn controller_role(self) -> Option<u8> {
        if self.controller_role == ROUTE_CONTROL_NONE {
            None
        } else {
            Some(self.controller_role)
        }
    }

    #[inline(always)]
    fn route_controller(self) -> Option<(PolicyMode, EffIndex, u8)> {
        if self.route_policy_eff.raw() == EffIndex::MAX.raw() {
            return None;
        }
        let policy = if self.route_policy_id == u16::MAX {
            PolicyMode::Static
        } else {
            PolicyMode::Dynamic {
                policy_id: self.route_policy_id,
                scope: self.scope_id,
            }
        };
        Some((policy, self.route_policy_eff, self.route_policy_tag))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ControlSemanticKind {
    Other = 0,
    RouteArm = 1,
    LoopContinue = 2,
    LoopBreak = 3,
    SpliceIntent = 4,
    SpliceAck = 5,
    Reroute = 6,
}

impl ControlSemanticKind {
    #[inline(always)]
    pub(crate) const fn is_loop(self) -> bool {
        matches!(self, Self::LoopContinue | Self::LoopBreak)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControlSemanticsTable {}

static CONTROL_SEMANTICS_TABLE: ControlSemanticsTable = ControlSemanticsTable::EMPTY;

impl ControlSemanticsTable {
    pub(crate) const EMPTY: Self = Self {};

    #[inline(always)]
    pub(crate) const fn semantic_for_label(&self, label: u8) -> ControlSemanticKind {
        match label {
            LABEL_LOOP_CONTINUE => ControlSemanticKind::LoopContinue,
            LABEL_LOOP_BREAK => ControlSemanticKind::LoopBreak,
            LABEL_SPLICE_INTENT => ControlSemanticKind::SpliceIntent,
            LABEL_SPLICE_ACK => ControlSemanticKind::SpliceAck,
            LABEL_REROUTE => ControlSemanticKind::Reroute,
            LABEL_ROUTE_DECISION => ControlSemanticKind::RouteArm,
            _ => ControlSemanticKind::Other,
        }
    }

    #[inline(always)]
    pub(crate) const fn semantic_for_resource_tag(
        &self,
        resource_tag: Option<u8>,
    ) -> ControlSemanticKind {
        match resource_tag {
            Some(LoopContinueKind::TAG) => ControlSemanticKind::LoopContinue,
            Some(LoopBreakKind::TAG) => ControlSemanticKind::LoopBreak,
            Some(SpliceIntentKind::TAG) => ControlSemanticKind::SpliceIntent,
            Some(SpliceAckKind::TAG) => ControlSemanticKind::SpliceAck,
            Some(RerouteKind::TAG) => ControlSemanticKind::Reroute,
            Some(RouteDecisionKind::TAG) => ControlSemanticKind::RouteArm,
            None => ControlSemanticKind::Other,
            Some(_) => ControlSemanticKind::Other,
        }
    }

    #[inline(always)]
    pub(crate) const fn semantic_for(
        &self,
        label: u8,
        resource_tag: Option<u8>,
    ) -> ControlSemanticKind {
        let by_resource = self.semantic_for_resource_tag(resource_tag);
        if matches!(by_resource, ControlSemanticKind::Other) {
            self.semantic_for_label(label)
        } else {
            by_resource
        }
    }

    #[inline(always)]
    pub(crate) const fn is_loop_label(&self, label: u8) -> bool {
        self.semantic_for_label(label).is_loop()
    }
}

const MAX_DYNAMIC_POLICY_SITES: usize = crate::eff::meta::MAX_EFF_NODES;
pub(crate) const MAX_COMPILED_PROGRAM_CP_EFFECTS: usize = 256;
pub(crate) const MAX_COMPILED_PROGRAM_TAP_EVENTS: usize = 512;
pub(crate) const MAX_COMPILED_PROGRAM_RESOURCES: usize = 128;
pub(crate) const MAX_COMPILED_PROGRAM_SCOPES: usize = crate::eff::meta::MAX_EFF_NODES;
pub(crate) const MAX_COMPILED_PROGRAM_CONTROLS: usize = crate::eff::meta::MAX_EFF_NODES;
pub(crate) const MAX_COMPILED_PROGRAM_ROUTE_CONTROLS: usize = crate::eff::meta::MAX_EFF_NODES;

#[inline(always)]
const fn encode_compact_program_len(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled program compact length overflow");
    }
    value as u16
}

#[inline(always)]
const fn encode_compact_program_offset(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled program compact offset overflow");
    }
    value as u16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CompiledProgramSection {
    offset: u16,
    len: u16,
}

impl CompiledProgramSection {
    const EMPTY: Self = Self { offset: 0, len: 0 };

    #[inline(always)]
    const fn from_offset(offset: usize) -> Self {
        Self {
            offset: encode_compact_program_offset(offset),
            len: 0,
        }
    }

    #[inline(always)]
    unsafe fn from_ptr<T>(base: *const u8, ptr: *const T, count: usize) -> Self {
        if count == 0 {
            Self::EMPTY
        } else {
            Self::from_offset(ptr.cast::<u8>() as usize - base as usize)
        }
    }

    #[inline(always)]
    const fn with_len(self, len: usize) -> Self {
        Self {
            offset: self.offset,
            len: encode_compact_program_len(len),
        }
    }

    #[inline(always)]
    const fn len(self) -> usize {
        self.len as usize
    }

    #[inline(always)]
    const fn is_empty(self) -> bool {
        self.len == 0
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramCounts {
    pub(crate) cp_effects: usize,
    pub(crate) tap_events: usize,
    pub(crate) resources: usize,
    pub(crate) controls: usize,
    pub(crate) dynamic_policy_sites: usize,
    pub(crate) route_controls: usize,
}

impl CompiledProgramCounts {
    const MAX: Self = Self {
        cp_effects: MAX_COMPILED_PROGRAM_CP_EFFECTS,
        tap_events: MAX_COMPILED_PROGRAM_TAP_EVENTS,
        resources: MAX_COMPILED_PROGRAM_RESOURCES,
        controls: MAX_COMPILED_PROGRAM_CONTROLS,
        dynamic_policy_sites: MAX_DYNAMIC_POLICY_SITES,
        route_controls: MAX_COMPILED_PROGRAM_ROUTE_CONTROLS,
    };
}

struct CompiledProgramTailStorage {
    resources: *mut ResourceDescriptor,
    resources_len: usize,
    sites: *mut DynamicPolicySite,
    sites_len: usize,
    route_controls: *mut RouteControlRecord,
    route_controls_len: usize,
}

impl CompiledProgramTailStorage {
    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    const fn max_align() -> usize {
        let mut align = core::mem::align_of::<CompiledProgramImage>();
        if core::mem::align_of::<ResourceDescriptor>() > align {
            align = core::mem::align_of::<ResourceDescriptor>();
        }
        if core::mem::align_of::<ControlScopeKind>() > align {
            align = core::mem::align_of::<ControlScopeKind>();
        }
        if core::mem::align_of::<DynamicPolicySite>() > align {
            align = core::mem::align_of::<DynamicPolicySite>();
        }
        if core::mem::align_of::<RouteControlRecord>() > align {
            align = core::mem::align_of::<RouteControlRecord>();
        }
        align
    }

    #[inline(always)]
    const fn section_bytes<T>(count: usize) -> usize {
        count.saturating_mul(core::mem::size_of::<T>())
    }

    #[inline(always)]
    const fn total_bytes_for_counts(counts: CompiledProgramCounts) -> usize {
        let mut offset = core::mem::size_of::<CompiledProgramImage>();
        offset = Self::align_up(offset, core::mem::align_of::<ResourceDescriptor>());
        offset = offset.saturating_add(Self::section_bytes::<ResourceDescriptor>(counts.resources));
        offset = Self::align_up(offset, core::mem::align_of::<DynamicPolicySite>());
        offset = offset.saturating_add(Self::section_bytes::<DynamicPolicySite>(
            counts.dynamic_policy_sites,
        ));
        offset = Self::align_up(offset, core::mem::align_of::<RouteControlRecord>());
        offset.saturating_add(Self::section_bytes::<RouteControlRecord>(
            counts.route_controls,
        ))
    }

    #[inline(always)]
    unsafe fn section_ptr<T>(base: *mut u8, offset: &mut usize, count: usize) -> *mut T {
        if count == 0 {
            return NonNull::<T>::dangling().as_ptr();
        }
        *offset = Self::align_up(*offset, core::mem::align_of::<T>());
        let ptr = unsafe { base.add(*offset) }.cast::<T>();
        *offset = offset.saturating_add(Self::section_bytes::<T>(count));
        ptr
    }

    #[inline(always)]
    unsafe fn from_image_ptr(
        image: *mut CompiledProgramImage,
        counts: CompiledProgramCounts,
    ) -> Self {
        let base = image.cast::<u8>();
        let mut offset = core::mem::size_of::<CompiledProgramImage>();
        let resources = unsafe { Self::section_ptr(base, &mut offset, counts.resources) };
        let sites = unsafe { Self::section_ptr(base, &mut offset, counts.dynamic_policy_sites) };
        let route_controls = unsafe { Self::section_ptr(base, &mut offset, counts.route_controls) };
        Self {
            resources,
            resources_len: counts.resources,
            sites,
            sites_len: counts.dynamic_policy_sites,
            route_controls,
            route_controls_len: counts.route_controls,
        }
    }
}

/// Crate-private runtime image for program-level immutable facts.
#[derive(Clone)]
pub(crate) struct CompiledProgramImage {
    resources: CompiledProgramSection,
    dynamic_policy_sites: CompiledProgramSection,
    route_controls: CompiledProgramSection,
    role_count: u8,
    control_scope_mask: u8,
}

#[inline(always)]
const fn compiled_program_push_dynamic_policy_site(
    dynamic_policy_sites: &mut [DynamicPolicySite],
    dynamic_policy_sites_len: &mut usize,
    site: DynamicPolicySite,
) -> u16 {
    if *dynamic_policy_sites_len >= dynamic_policy_sites.len() {
        panic!("CompiledProgram: MAX_DYNAMIC_POLICY_SITES exceeded");
    }
    let site_index = *dynamic_policy_sites_len;
    dynamic_policy_sites[site_index] = site;
    *dynamic_policy_sites_len += 1;
    site_index as u16
}

#[inline(always)]
fn compiled_program_push_resource(
    resources: &mut [ResourceDescriptor],
    len: &mut usize,
    descriptor: ResourceDescriptor,
) {
    if *len >= resources.len() {
        panic!("CompiledProgram: MAX_RESOURCES exceeded");
    }
    resources[*len] = descriptor;
    *len += 1;
}

#[inline(always)]
fn compiled_program_route_scope_end(
    scope_markers: &[ScopeMarker],
    enter_idx: usize,
    scope: ScopeId,
    default_end: usize,
) -> usize {
    let mut scope_end = default_end;
    let mut scan_idx = enter_idx + 1;
    let mut nest_depth = 1usize;
    while scan_idx < scope_markers.len() {
        let scan_marker = scope_markers[scan_idx];
        if scan_marker.scope_id.local_ordinal() == scope.local_ordinal() {
            match scan_marker.event {
                ScopeEvent::Enter => nest_depth += 1,
                ScopeEvent::Exit => {
                    nest_depth -= 1;
                    if nest_depth == 0 {
                        scope_end = scan_marker.offset;
                        break;
                    }
                }
            }
        }
        scan_idx += 1;
    }
    scope_end
}

#[inline(always)]
fn compiled_program_insert_route_control(
    route_controls: &mut [RouteControlRecord],
    route_controls_len: &mut usize,
    record: RouteControlRecord,
) {
    let target_raw = record.canonical_raw();
    let mut insert_idx = 0usize;
    while insert_idx < *route_controls_len {
        let existing_raw = route_controls[insert_idx].canonical_raw();
        if existing_raw == target_raw {
            return;
        }
        if existing_raw > target_raw {
            break;
        }
        insert_idx += 1;
    }
    if *route_controls_len >= route_controls.len() {
        panic!("CompiledProgram: MAX_ROUTE_CONTROLS exceeded");
    }
    let mut shift_idx = *route_controls_len;
    while shift_idx > insert_idx {
        route_controls[shift_idx] = route_controls[shift_idx - 1];
        shift_idx -= 1;
    }
    route_controls[insert_idx] = record;
    *route_controls_len += 1;
}

#[inline(always)]
fn compiled_program_emit_route_controls(
    route_controls: &mut [RouteControlRecord],
    route_controls_len: &mut usize,
    summary: &LoweringSummary,
) {
    let view = summary.view();
    let scope_markers = view.scope_markers();
    let default_end = view.as_slice().len();
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers[marker_idx];
        if matches!(marker.event, ScopeEvent::Enter)
            && matches!(marker.scope_kind, ScopeKind::Route)
        {
            let scope_end = compiled_program_route_scope_end(
                scope_markers,
                marker_idx,
                marker.scope_id,
                default_end,
            );
            let (route_policy_id, route_policy_eff, route_policy_tag) = match view
                .first_route_head_dynamic_policy_in_range(marker.scope_id, marker_idx, scope_end)
            {
                Some((policy, eff_offset, tag)) => (
                    match policy.dynamic_policy_id() {
                        Some(policy_id) => policy_id,
                        None => u16::MAX,
                    },
                    EffIndex::from_usize(eff_offset),
                    tag,
                ),
                None => (u16::MAX, EffIndex::MAX, 0),
            };
            compiled_program_insert_route_control(
                route_controls,
                route_controls_len,
                RouteControlRecord::new(
                    marker.scope_id,
                    marker.controller_role,
                    route_policy_id,
                    route_policy_eff,
                    route_policy_tag,
                ),
            );
        }
        marker_idx += 1;
    }
}

#[inline(always)]
fn compiled_program_lookup_route_control(
    route_controls: &[RouteControlRecord],
    scope_id: ScopeId,
) -> Option<&RouteControlRecord> {
    if scope_id.is_none() {
        return None;
    }
    let target_raw = scope_id.canonical().raw();
    let mut lo = 0usize;
    let mut hi = route_controls.len();
    while lo < hi {
        let mid = lo + ((hi - lo) / 2);
        let raw = route_controls[mid].canonical_raw();
        if raw < target_raw {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo >= route_controls.len() || route_controls[lo].canonical_raw() != target_raw {
        None
    } else {
        Some(&route_controls[lo])
    }
}

#[inline(always)]
pub(super) const fn control_scope_mask_bit(scope_kind: ControlScopeKind) -> u8 {
    match scope_kind {
        ControlScopeKind::None => 0,
        ControlScopeKind::Loop => 1 << 0,
        ControlScopeKind::Checkpoint => 1 << 1,
        ControlScopeKind::Cancel => 0,
        ControlScopeKind::Splice => 1 << 3,
        ControlScopeKind::Reroute => 0,
        ControlScopeKind::Policy => 0,
        ControlScopeKind::Route => 0,
    }
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn compiled_program_emit_atom_into_slices(
    resources: &mut [ResourceDescriptor],
    resources_len: &mut usize,
    atom: EffAtom,
    offset: usize,
    policy: PolicyMode,
    resource_policy_site: u16,
    _control_spec: Option<ControlLabelSpec>,
) {
    if atom.is_control {
        if let Some(resource_kind_tag) = atom.resource {
            let descriptor = ResourceDescriptor::new(
                EffIndex::from_usize(offset),
                atom.label,
                resource_kind_tag,
                resource_policy_site,
            );
            compiled_program_push_resource(resources, resources_len, descriptor);
        }
    } else if !policy.is_static() && !matches!(policy, PolicyMode::Dynamic { .. }) {
        panic!("static policy attached to non-control atom");
    }
}

#[inline(always)]
#[cfg(test)]
fn compiled_program_emit_atom(
    effect_envelope: &mut EffectEnvelope,
    atom: EffAtom,
    offset: usize,
    policy: PolicyMode,
    resource_policy_site: u16,
    _control_spec: Option<ControlLabelSpec>,
) {
    if atom.is_control {
        if let Some(resource_kind_tag) = atom.resource {
            if let Some(effect) = CpEffect::from_resource_tag(resource_kind_tag) {
                effect_envelope.push_cp_effect(effect);
                effect_envelope.push_tap_event(effect.to_tap_event_id());
            } else {
                let tap_id = 0x0300 + atom.label as u16;
                effect_envelope.push_tap_event(tap_id);
            }

            effect_envelope.push_resource(
                EffIndex::from_usize(offset),
                atom.label,
                resource_kind_tag,
                resource_policy_site,
            );
        } else {
            let tap_id = 0x0300 + atom.label as u16;
            effect_envelope.push_tap_event(tap_id);
        }
    } else {
        if !policy.is_static() && !matches!(policy, PolicyMode::Dynamic { .. }) {
            panic!("static policy attached to non-control atom");
        }
        let tap_id = 0x0200 + atom.label as u16;
        effect_envelope.push_tap_event(tap_id);
    }
}

/// Crate-private owner for program-level lowering facts.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct CompiledProgram {
    lease_budget: LeaseGraphBudget,
    effect_envelope: EffectEnvelope,
    control_scope_mask: u8,
    dynamic_policy_sites: [DynamicPolicySite; MAX_DYNAMIC_POLICY_SITES],
    route_controls: [RouteControlRecord; MAX_COMPILED_PROGRAM_ROUTE_CONTROLS],
}

#[cfg(test)]
impl CompiledProgram {
    #[inline(always)]
    unsafe fn init_array_copy<T: Copy, const N: usize>(dst: *mut [T; N], value: T) {
        let ptr = dst.cast::<T>();
        let mut idx = 0usize;
        while idx < N {
            unsafe {
                ptr.add(idx).write(value);
            }
            idx += 1;
        }
    }

    #[cfg(test)]
    pub(crate) fn from_summary(summary: &LoweringSummary) -> Self {
        let mut compiled = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_from_summary(compiled.as_mut_ptr(), summary);
            compiled.assume_init()
        }
    }

    #[inline(never)]
    pub(crate) unsafe fn init_from_summary(dst: *mut Self, summary: &LoweringSummary) {
        unsafe {
            ptr::addr_of_mut!((*dst).lease_budget).write(LeaseGraphBudget::new());
            EffectEnvelope::init_empty(ptr::addr_of_mut!((*dst).effect_envelope));
            ptr::addr_of_mut!((*dst).control_scope_mask).write(0);
            Self::init_array_copy(
                ptr::addr_of_mut!((*dst).dynamic_policy_sites),
                DynamicPolicySite::EMPTY,
            );
            Self::init_array_copy(
                ptr::addr_of_mut!((*dst).route_controls),
                RouteControlRecord::EMPTY,
            );
        }

        let effect_envelope = unsafe { &mut *ptr::addr_of_mut!((*dst).effect_envelope) };
        let dynamic_policy_sites = unsafe { &mut *ptr::addr_of_mut!((*dst).dynamic_policy_sites) };
        let mut dynamic_policy_sites_len = 0usize;
        let route_controls = unsafe { &mut *ptr::addr_of_mut!((*dst).route_controls) };
        let mut route_controls_len = 0usize;

        let view = summary.view();
        let mut lease_budget = LeaseGraphBudget::new();

        let nodes = view.as_slice();
        let mut offset = 0usize;
        while offset < nodes.len() {
            let node = nodes[offset];
            if matches!(node.kind, EffKind::Atom) {
                let policy = match view.policy_at(offset) {
                    Some(policy) => policy,
                    None => PolicyMode::Static,
                };
                let atom = node.atom_data();
                let control_spec = view.control_spec_at(offset);
                lease_budget = lease_budget.include_atom(atom.label, atom.resource, policy);
                let resource_policy_site = if policy.is_dynamic() {
                    compiled_program_push_dynamic_policy_site(
                        dynamic_policy_sites,
                        &mut dynamic_policy_sites_len,
                        DynamicPolicySite::new(
                            EffIndex::from_usize(offset),
                            atom.label,
                            atom.resource,
                            policy,
                        ),
                    )
                } else {
                    ResourceDescriptor::STATIC_POLICY_SITE
                };
                compiled_program_emit_atom(
                    effect_envelope,
                    atom,
                    offset,
                    policy,
                    resource_policy_site,
                    control_spec,
                );
            }
            offset += 1;
        }

        lease_budget.validate();
        unsafe {
            ptr::addr_of_mut!((*dst).lease_budget).write(lease_budget);
        }

        let control_markers = summary.control_markers();
        let mut control_idx = 0usize;
        while control_idx < control_markers.len() {
            let marker = control_markers[control_idx];
            if marker.tap_id != 0 {
                effect_envelope.push_tap_event(marker.tap_id);
            }
            control_idx += 1;
        }
        unsafe {
            ptr::addr_of_mut!((*dst).control_scope_mask)
                .write(summary.compiled_program_control_scope_mask());
        }
        compiled_program_emit_route_controls(route_controls, &mut route_controls_len, summary);
    }

    #[inline(always)]
    fn dynamic_policy_sites_len(&self) -> usize {
        let mut len = 0usize;
        while len < self.dynamic_policy_sites.len() {
            if self.dynamic_policy_sites[len] == DynamicPolicySite::EMPTY {
                break;
            }
            len += 1;
        }
        len
    }

    #[inline(always)]
    fn route_controls_len(&self) -> usize {
        let mut len = 0usize;
        while len < self.route_controls.len() {
            if self.route_controls[len] == RouteControlRecord::EMPTY {
                break;
            }
            len += 1;
        }
        len
    }

    #[inline(always)]
    pub(crate) fn effect_envelope(&self) -> EffectEnvelopeRef<'_> {
        self.effect_envelope.as_ref_with_controls(
            self.control_scope_mask,
            &self.dynamic_policy_sites[..self.dynamic_policy_sites_len()],
        )
    }

    #[inline(always)]
    pub(crate) const fn control_semantics(&self) -> &ControlSemanticsTable {
        &CONTROL_SEMANTICS_TABLE
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites(&self) -> &[DynamicPolicySite] {
        &self.dynamic_policy_sites[..self.dynamic_policy_sites_len()]
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites_for(
        &self,
        policy_id: u16,
    ) -> impl Iterator<Item = &DynamicPolicySite> + '_ {
        self.dynamic_policy_sites()
            .iter()
            .filter(move |site| site.policy_id() == policy_id)
    }

    #[inline(always)]
    pub(crate) fn route_controller_role(&self, scope_id: ScopeId) -> Option<u8> {
        compiled_program_lookup_route_control(
            &self.route_controls[..self.route_controls_len()],
            scope_id,
        )
        .and_then(|record| record.controller_role())
    }

    #[inline(always)]
    pub(crate) fn route_controller(&self, scope_id: ScopeId) -> Option<(PolicyMode, EffIndex, u8)> {
        compiled_program_lookup_route_control(
            &self.route_controls[..self.route_controls_len()],
            scope_id,
        )
        .and_then(|record| record.route_controller())
    }
}

impl CompiledProgramImage {
    #[inline(always)]
    fn section_slice<T>(&self, section: CompiledProgramSection) -> &[T] {
        if section.is_empty() {
            &[]
        } else {
            unsafe {
                core::slice::from_raw_parts(
                    (self as *const Self)
                        .cast::<u8>()
                        .add(section.offset as usize)
                        .cast::<T>(),
                    section.len(),
                )
            }
        }
    }

    #[inline(always)]
    pub(crate) fn counts(summary: &LoweringSummary) -> CompiledProgramCounts {
        summary.compiled_program_counts()
    }

    #[inline(always)]
    pub(crate) const fn persistent_bytes_for_counts(counts: CompiledProgramCounts) -> usize {
        CompiledProgramTailStorage::total_bytes_for_counts(counts)
    }

    #[inline(always)]
    pub(crate) const fn max_persistent_bytes() -> usize {
        Self::persistent_bytes_for_counts(CompiledProgramCounts::MAX)
    }

    #[inline(always)]
    pub(crate) const fn persistent_align() -> usize {
        CompiledProgramTailStorage::max_align()
    }

    pub(crate) unsafe fn init_from_summary(dst: *mut Self, summary: &LoweringSummary) {
        let counts = summary.compiled_program_counts();
        let storage = unsafe { CompiledProgramTailStorage::from_image_ptr(dst, counts) };
        let base = dst.cast::<u8>().cast_const();
        let resources_section = unsafe {
            CompiledProgramSection::from_ptr(
                base,
                storage.resources.cast_const(),
                storage.resources_len,
            )
        };
        let dynamic_policy_sites_section = unsafe {
            CompiledProgramSection::from_ptr(base, storage.sites.cast_const(), storage.sites_len)
        };
        let route_controls_section = unsafe {
            CompiledProgramSection::from_ptr(
                base,
                storage.route_controls.cast_const(),
                storage.route_controls_len,
            )
        };
        unsafe {
            ptr::addr_of_mut!((*dst).resources).write(resources_section);
            ptr::addr_of_mut!((*dst).dynamic_policy_sites).write(dynamic_policy_sites_section);
            ptr::addr_of_mut!((*dst).route_controls).write(route_controls_section);
            ptr::addr_of_mut!((*dst).role_count).write(summary.compiled_program_role_count() as u8);
            ptr::addr_of_mut!((*dst).control_scope_mask).write(0);
        }

        let resources =
            unsafe { core::slice::from_raw_parts_mut(storage.resources, storage.resources_len) };
        let mut resources_len = 0usize;
        let dynamic_policy_sites =
            unsafe { core::slice::from_raw_parts_mut(storage.sites, storage.sites_len) };
        let mut dynamic_policy_sites_len = 0usize;
        let route_controls = unsafe {
            core::slice::from_raw_parts_mut(storage.route_controls, storage.route_controls_len)
        };
        let mut route_controls_len = 0usize;

        let view = summary.view();
        let nodes = view.as_slice();
        let mut offset = 0usize;
        while offset < nodes.len() {
            let node = nodes[offset];
            if matches!(node.kind, EffKind::Atom) {
                let policy = match view.policy_at(offset) {
                    Some(policy) => policy,
                    None => PolicyMode::Static,
                };
                let atom = node.atom_data();
                let control_spec = view.control_spec_at(offset);
                let resource_policy_site = if policy.is_dynamic() {
                    compiled_program_push_dynamic_policy_site(
                        dynamic_policy_sites,
                        &mut dynamic_policy_sites_len,
                        DynamicPolicySite::new(
                            EffIndex::from_usize(offset),
                            atom.label,
                            atom.resource,
                            policy,
                        ),
                    )
                } else {
                    ResourceDescriptor::STATIC_POLICY_SITE
                };
                compiled_program_emit_atom_into_slices(
                    resources,
                    &mut resources_len,
                    atom,
                    offset,
                    policy,
                    resource_policy_site,
                    control_spec,
                );
            }
            offset += 1;
        }

        unsafe {
            ptr::addr_of_mut!((*dst).resources).write(resources_section.with_len(resources_len));
            ptr::addr_of_mut!((*dst).dynamic_policy_sites)
                .write(dynamic_policy_sites_section.with_len(dynamic_policy_sites_len));
            compiled_program_emit_route_controls(route_controls, &mut route_controls_len, summary);
            ptr::addr_of_mut!((*dst).route_controls)
                .write(route_controls_section.with_len(route_controls_len));
            ptr::addr_of_mut!((*dst).control_scope_mask)
                .write(summary.compiled_program_control_scope_mask());
        }
    }

    #[inline(always)]
    pub(crate) fn effect_envelope(&self) -> EffectEnvelopeRef<'_> {
        EffectEnvelopeRef::new(
            &[],
            &[],
            self.section_slice(self.resources),
            self.section_slice(self.dynamic_policy_sites),
            self.control_scope_mask,
        )
    }

    #[inline(always)]
    pub(crate) const fn control_semantics(&self) -> &ControlSemanticsTable {
        &CONTROL_SEMANTICS_TABLE
    }

    #[inline(always)]
    pub(crate) const fn role_count(&self) -> usize {
        self.role_count as usize
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites(&self) -> &[DynamicPolicySite] {
        self.section_slice(self.dynamic_policy_sites)
    }

    #[inline(always)]
    fn route_controls(&self) -> &[RouteControlRecord] {
        self.section_slice(self.route_controls)
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites_for(
        &self,
        policy_id: u16,
    ) -> impl Iterator<Item = &DynamicPolicySite> + '_ {
        self.dynamic_policy_sites()
            .iter()
            .filter(move |site| site.policy_id() == policy_id)
    }

    #[inline(always)]
    pub(crate) fn route_controller_role(&self, scope_id: ScopeId) -> Option<u8> {
        compiled_program_lookup_route_control(self.route_controls(), scope_id)
            .and_then(|record| record.controller_role())
    }

    #[inline(always)]
    pub(crate) fn route_controller(&self, scope_id: ScopeId) -> Option<(PolicyMode, EffIndex, u8)> {
        compiled_program_lookup_route_control(self.route_controls(), scope_id)
            .and_then(|record| record.route_controller())
    }
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::{CompiledProgram, ControlSemanticKind};
    use crate::{
        control::cap::{
            mint::{GenericCapToken, ResourceKind},
            resource_kinds::{
                CancelKind, LoopBreakKind, LoopContinueKind, RerouteKind, RouteDecisionKind,
            },
        },
        eff::EffIndex,
        g::{self, Msg, Role},
        global::{
            CanonicalControl,
            const_dsl::{PolicyMode, ScopeEvent, ScopeKind},
        },
        runtime::consts::{
            LABEL_CANCEL, LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE, LABEL_REROUTE,
            LABEL_ROUTE_DECISION,
        },
    };

    const ROUTE_POLICY_ID: u16 = 4401;

    #[test]
    fn compiled_program_tracks_dynamic_policy_sites_and_lease_budget() {
        let program = g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>();

        let summary = program.summary();
        let compiled = CompiledProgram::from_summary(&summary);
        let mut sites = compiled.dynamic_policy_sites_for(ROUTE_POLICY_ID);
        let site = sites.next().expect("route policy site");
        assert!(
            sites.next().is_none(),
            "expected single dynamic policy site"
        );
        assert_eq!(site.label(), LABEL_ROUTE_DECISION);
        assert_eq!(site.resource_tag(), Some(RouteDecisionKind::TAG));

        let budget = compiled.lease_budget;
        assert!(!budget.requires_caps());
        assert!(!budget.requires_slots());
        assert!(!budget.requires_splice());
        assert!(!budget.requires_delegation());

        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_label(LABEL_ROUTE_DECISION),
            ControlSemanticKind::RouteArm
        );
        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_resource_tag(Some(RouteDecisionKind::TAG)),
            ControlSemanticKind::RouteArm
        );
    }

    #[test]
    fn compiled_program_tracks_shared_route_controller_atlas() {
        let left = g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>();
        let right = g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>();
        let program = g::route(left, right);

        let summary = program.summary();
        let scope_id = summary
            .view()
            .scope_markers()
            .iter()
            .find(|marker| {
                matches!(marker.event, ScopeEvent::Enter)
                    && matches!(marker.scope_kind, ScopeKind::Route)
            })
            .expect("route enter marker")
            .scope_id;
        let compiled = CompiledProgram::from_summary(&summary);

        assert_eq!(compiled.route_controller_role(scope_id), Some(0));
        let (policy, eff_index, resource_tag) = compiled
            .route_controller(scope_id)
            .expect("route controller atlas entry");
        assert_eq!(
            policy,
            PolicyMode::dynamic(ROUTE_POLICY_ID).with_scope(scope_id)
        );
        assert_eq!(eff_index, EffIndex::ZERO);
        assert_eq!(resource_tag, LoopContinueKind::TAG);
    }

    #[test]
    fn compiled_program_route_controller_atlas_ignores_nested_dynamic_policy_scopes() {
        let nested_left = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>();
        let nested_right = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_REROUTE }, GenericCapToken<RerouteKind>, CanonicalControl<RerouteKind>>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>();
        let left = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                0,
            >(),
            g::route(nested_left, nested_right),
        );
        let right = g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            0,
        >();
        let program = g::route(left, right);

        let summary = program.summary();
        let scope_id = summary
            .view()
            .scope_markers()
            .iter()
            .find(|marker| {
                matches!(marker.event, ScopeEvent::Enter)
                    && matches!(marker.scope_kind, ScopeKind::Route)
            })
            .expect("outer route enter marker")
            .scope_id;
        let compiled = CompiledProgram::from_summary(&summary);

        assert_eq!(compiled.route_controller_role(scope_id), Some(0));
        assert_eq!(
            compiled.route_controller(scope_id),
            None,
            "nested dynamic policies must not promote the enclosing static route"
        );
    }

    #[test]
    fn compiled_program_image_header_stays_compact() {
        assert!(
            size_of::<super::CompiledProgramImage>() < 192,
            "CompiledProgramImage header regressed back to pointer-rich layout: {} bytes",
            size_of::<super::CompiledProgramImage>()
        );
    }

    #[test]
    fn control_semantics_table_stays_stateless() {
        assert_eq!(
            size_of::<super::ControlSemanticsTable>(),
            0,
            "ControlSemanticsTable regressed from fixed semantic dispatch: {} bytes",
            size_of::<super::ControlSemanticsTable>()
        );
    }

    #[test]
    fn compiled_program_marks_loop_control_semantics_from_control_metadata() {
        let body = g::send::<Role<0>, Role<1>, Msg<7, ()>, 0>();
        let continue_arm = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                0,
            >(),
            body,
        );
        let break_arm = g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            0,
        >();
        let program = g::route(continue_arm, break_arm);

        let summary = program.summary();
        let compiled = CompiledProgram::from_summary(&summary);

        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_label(LABEL_LOOP_CONTINUE),
            ControlSemanticKind::LoopContinue
        );
        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_label(LABEL_LOOP_BREAK),
            ControlSemanticKind::LoopBreak
        );
        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_resource_tag(Some(LoopContinueKind::TAG)),
            ControlSemanticKind::LoopContinue
        );
        assert_eq!(
            compiled
                .control_semantics()
                .semantic_for_resource_tag(Some(LoopBreakKind::TAG)),
            ControlSemanticKind::LoopBreak
        );
    }

    #[test]
    fn compiled_program_skips_noop_control_scope_resets() {
        let cancel = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>,
            0,
        >();
        let reroute = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_REROUTE }, GenericCapToken<RerouteKind>, CanonicalControl<RerouteKind>>,
            0,
        >();
        let route = g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >,
            0,
        >();
        let program = g::seq(cancel, g::seq(reroute, route));

        let summary = program.summary();
        let compiled = CompiledProgram::from_summary(&summary);
        let effect_envelope = compiled.effect_envelope();

        assert_eq!(effect_envelope.resources().count(), 3);
        assert!(
            effect_envelope.control_scopes().next().is_none(),
            "no-op control scopes should not stay in the runtime reset mask"
        );
    }
}
