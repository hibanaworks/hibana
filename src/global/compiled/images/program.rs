#[cfg(test)]
use crate::control::cluster::effects::ResourceDescriptor;
use crate::global::compiled::layout::{
    compiled_program_tail_align, compiled_program_tail_bytes_for_counts,
};
use crate::{
    control::{
        cap::mint::ResourceKind,
        cap::resource_kinds::{
            LoopBreakKind, LoopContinueKind, RerouteKind, RouteDecisionKind, SpliceAckKind,
            SpliceIntentKind,
        },
        cluster::effects::EffectEnvelopeRef,
    },
    eff::EffIndex,
    global::const_dsl::{CompactScopeId, PolicyMode, ScopeId},
    runtime::consts::{
        LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE, LABEL_REROUTE, LABEL_ROUTE_DECISION,
        LABEL_SPLICE_ACK, LABEL_SPLICE_INTENT,
    },
};

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
    pub(in crate::global::compiled) const EMPTY: Self = Self {
        eff_index: EffIndex::ZERO,
        label: 0,
        resource_tag: None,
        policy: PolicyMode::Static,
    };

    #[inline(always)]
    pub(in crate::global::compiled) const fn new(
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
    pub(in crate::global::compiled) const EMPTY: Self = Self {
        scope_id: CompactScopeId::none(),
        controller_role: ROUTE_CONTROL_NONE,
        route_policy_tag: 0,
        route_policy_id: u16::MAX,
        route_policy_eff: EffIndex::MAX,
    };

    #[inline(always)]
    pub(in crate::global::compiled) const fn new(
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
    pub(in crate::global::compiled) const fn canonical_raw(self) -> u64 {
        self.scope_id.canonical().raw()
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn controller_role(self) -> Option<u8> {
        if self.controller_role == ROUTE_CONTROL_NONE {
            None
        } else {
            Some(self.controller_role)
        }
    }

    #[inline(always)]
    pub(in crate::global::compiled) fn route_controller(
        self,
    ) -> Option<(PolicyMode, EffIndex, u8)> {
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

pub(in crate::global::compiled) static CONTROL_SEMANTICS_TABLE: ControlSemanticsTable =
    ControlSemanticsTable::EMPTY;

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

pub(in crate::global::compiled) const MAX_DYNAMIC_POLICY_SITES: usize =
    crate::eff::meta::MAX_EFF_NODES;
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
pub(in crate::global::compiled) struct CompiledProgramSection {
    offset: u16,
    len: u16,
}

impl CompiledProgramSection {
    pub(in crate::global::compiled) const EMPTY: Self = Self { offset: 0, len: 0 };

    #[inline(always)]
    const fn from_offset(offset: usize) -> Self {
        Self {
            offset: encode_compact_program_offset(offset),
            len: 0,
        }
    }

    #[inline(always)]
    pub(in crate::global::compiled) unsafe fn from_ptr<T>(
        base: *const u8,
        ptr: *const T,
        count: usize,
    ) -> Self {
        if count == 0 {
            Self::EMPTY
        } else {
            Self::from_offset(ptr.cast::<u8>() as usize - base as usize)
        }
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn with_len(self, len: usize) -> Self {
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

/// Crate-private runtime image for program-level immutable facts.
#[derive(Clone)]
pub(crate) struct CompiledProgramImage {
    pub(in crate::global::compiled) resources: CompiledProgramSection,
    pub(in crate::global::compiled) dynamic_policy_sites: CompiledProgramSection,
    pub(in crate::global::compiled) route_controls: CompiledProgramSection,
    pub(in crate::global::compiled) role_count: u8,
    pub(in crate::global::compiled) control_scope_mask: u8,
}

#[inline(always)]
pub(in crate::global::compiled) fn compiled_program_lookup_route_control(
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

    #[cfg(test)]
    #[inline(always)]
    const fn section_end<T>(section: CompiledProgramSection) -> usize {
        if section.is_empty() {
            core::mem::size_of::<Self>()
        } else {
            section.offset as usize + section.len() * core::mem::size_of::<T>()
        }
    }

    #[inline(always)]
    pub(crate) const fn persistent_bytes_for_counts(counts: CompiledProgramCounts) -> usize {
        compiled_program_tail_bytes_for_counts(counts)
    }

    #[inline(always)]
    pub(crate) const fn max_persistent_bytes() -> usize {
        Self::persistent_bytes_for_counts(CompiledProgramCounts::MAX)
    }

    #[inline(always)]
    pub(crate) const fn persistent_align() -> usize {
        compiled_program_tail_align()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn actual_persistent_bytes(&self) -> usize {
        let mut end = core::mem::size_of::<Self>();
        let resources_end = Self::section_end::<ResourceDescriptor>(self.resources);
        if resources_end > end {
            end = resources_end;
        }
        let dynamic_sites_end = Self::section_end::<DynamicPolicySite>(self.dynamic_policy_sites);
        if dynamic_sites_end > end {
            end = dynamic_sites_end;
        }
        let route_controls_end = Self::section_end::<RouteControlRecord>(self.route_controls);
        if route_controls_end > end {
            end = route_controls_end;
        }
        end
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
    use core::{mem::size_of, ptr};

    use super::{CompiledProgramImage, ControlSemanticKind};
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

    fn with_compiled_program_image<R>(
        summary: &crate::global::compiled::lowering::LoweringSummary,
        f: impl FnOnce(&CompiledProgramImage) -> R,
    ) -> R {
        const fn align_up(value: usize, align: usize) -> usize {
            let mask = align.saturating_sub(1);
            (value + mask) & !mask
        }

        let counts = summary.compiled_program_counts();
        let bytes = CompiledProgramImage::persistent_bytes_for_counts(counts);
        let align = CompiledProgramImage::persistent_align();
        let mut storage = std::vec::Vec::with_capacity(bytes + align);
        storage.resize(bytes + align, 0u8);
        let base = storage.as_mut_ptr() as usize;
        let aligned = align_up(base, align) as *mut CompiledProgramImage;
        debug_assert!((aligned as usize) + bytes <= base + storage.len());
        unsafe {
            crate::global::compiled::lowering::program_image_builder::init_compiled_program_image_from_summary(aligned, summary);
            let result = f(&*aligned);
            ptr::drop_in_place(aligned);
            result
        }
    }

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
        let compiled = crate::global::compiled::lowering::CompiledProgram::from_summary(&summary);
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
        let compiled = crate::global::compiled::lowering::CompiledProgram::from_summary(&summary);

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
        let compiled = crate::global::compiled::lowering::CompiledProgram::from_summary(&summary);

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
    fn compiled_program_image_persistent_bytes_match_exact_counts() {
        let program = g::seq(
            g::send::<Role<0>, Role<1>, Msg<7, ()>, 0>(),
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    CanonicalControl<RouteDecisionKind>,
                >,
                0,
            >()
            .policy::<ROUTE_POLICY_ID>(),
        );
        let summary = program.summary();
        let expected =
            CompiledProgramImage::persistent_bytes_for_counts(summary.compiled_program_counts());
        with_compiled_program_image(&summary, |image| {
            assert_eq!(image.actual_persistent_bytes(), expected);
        });
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
        let compiled = crate::global::compiled::lowering::CompiledProgram::from_summary(&summary);

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
        let compiled = crate::global::compiled::lowering::CompiledProgram::from_summary(&summary);
        let effect_envelope = compiled.effect_envelope();

        assert_eq!(effect_envelope.resources().count(), 3);
        assert!(
            effect_envelope.control_scopes().next().is_none(),
            "no-op control scopes should not stay in the runtime reset mask"
        );
    }
}
