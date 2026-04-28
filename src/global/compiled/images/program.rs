#[cfg(test)]
use crate::control::cluster::effects::ResourceDescriptor;
use crate::global::compiled::layout::{
    compiled_program_tail_align, compiled_program_tail_bytes_for_counts,
};
use crate::{
    control::{cap::mint::ControlOp, cluster::effects::EffectEnvelopeRef},
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
    label: u8,
    resource_tag: Option<u8>,
    op: Option<ControlOp>,
    policy: PolicyMode,
}

impl DynamicPolicySite {
    #[inline(always)]
    pub(in crate::global::compiled) const fn new(
        eff_index: EffIndex,
        label: u8,
        resource_tag: Option<u8>,
        op: Option<ControlOp>,
        policy: PolicyMode,
    ) -> Self {
        Self {
            eff_index,
            label,
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
    pub(crate) const fn label(&self) -> u8 {
        self.label
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
    ) -> Option<(PolicyMode, EffIndex, u8, ControlOp)> {
        if self.route_policy_eff.raw() == EffIndex::MAX.raw() {
            return None;
        }
        let op = match self.route_policy_op {
            Some(op) => op,
            None => return None,
        };
        let policy = if self.route_policy_id == u16::MAX {
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

pub(in crate::global::compiled) const MAX_DYNAMIC_POLICY_SITES: usize =
    crate::eff::meta::MAX_EFF_NODES;
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
    pub(crate) tap_events: usize,
    pub(crate) resources: usize,
    pub(crate) controls: usize,
    pub(crate) dynamic_policy_sites: usize,
    pub(crate) route_controls: usize,
}

impl CompiledProgramCounts {
    const MAX: Self = Self {
        tap_events: MAX_COMPILED_PROGRAM_TAP_EVENTS,
        resources: MAX_COMPILED_PROGRAM_RESOURCES,
        controls: MAX_COMPILED_PROGRAM_CONTROLS,
        dynamic_policy_sites: MAX_DYNAMIC_POLICY_SITES,
        route_controls: MAX_COMPILED_PROGRAM_ROUTE_CONTROLS,
    };
}

/// Crate-private runtime image for program-level immutable facts.
#[derive(Clone)]
pub(crate) struct CompiledProgramFacts {
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

impl CompiledProgramFacts {
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
    pub(crate) fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(PolicyMode, EffIndex, u8, ControlOp)> {
        compiled_program_lookup_route_control(self.route_controls(), scope_id)
            .and_then(|record| record.route_controller())
    }
}

#[cfg(test)]
mod tests {
    use core::{mem::size_of, ptr};

    mod abort_control_kind {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/abort_control.rs"
        ));
    }
    mod fence_control_kind {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/fence_control.rs"
        ));
    }

    use super::{CompiledProgramFacts, ControlSemanticKind};
    use crate::{
        control::cap::{
            mint::{ControlOp, GenericCapToken, ResourceKind},
            resource_kinds::{LoopBreakKind, LoopContinueKind, RouteDecisionKind},
        },
        eff::EffIndex,
        g::{self, Msg, Role},
        global::const_dsl::{PolicyMode, ScopeEvent, ScopeKind},
        runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE, LABEL_ROUTE_DECISION},
    };
    use abort_control_kind::{AbortControl, LABEL_ABORT_CONTROL};
    use fence_control_kind::{FenceControl, LABEL_FENCE_CONTROL};

    const ROUTE_POLICY_ID: u16 = 4401;

    fn with_compiled_program_facts<R>(
        summary: &crate::global::compiled::lowering::LoweringSummary,
        f: impl FnOnce(&CompiledProgramFacts) -> R,
    ) -> R {
        const fn align_up(value: usize, align: usize) -> usize {
            let mask = align.saturating_sub(1);
            (value + mask) & !mask
        }

        const STORAGE_BYTES: usize =
            CompiledProgramFacts::max_persistent_bytes() + CompiledProgramFacts::persistent_align();
        static STORAGE: std::sync::Mutex<[u8; STORAGE_BYTES]> =
            std::sync::Mutex::new([0u8; STORAGE_BYTES]);

        let counts = summary.compiled_program_counts();
        let bytes = CompiledProgramFacts::persistent_bytes_for_counts(counts);
        let align = CompiledProgramFacts::persistent_align();
        let mut storage = STORAGE
            .lock()
            .expect("compiled program image test storage lock poisoned");
        assert!(
            bytes + align <= storage.len(),
            "compiled program image test storage must cover max persistent image"
        );
        let base = storage.as_mut_ptr() as usize;
        let aligned = align_up(base, align) as *mut CompiledProgramFacts;
        assert!(
            (aligned as usize) + bytes <= base + storage.len(),
            "compiled program image test storage alignment exceeded backing storage"
        );
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
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>();

        let summary = program.summary();
        with_compiled_program_facts(&summary, |compiled| {
            let mut sites = compiled.dynamic_policy_sites_for(ROUTE_POLICY_ID);
            let site = sites.next().expect("route policy site");
            assert!(
                sites.next().is_none(),
                "expected single dynamic policy site"
            );
            assert_eq!(site.label(), LABEL_ROUTE_DECISION);
            assert_eq!(site.resource_tag(), Some(RouteDecisionKind::TAG));
        });

        let mut budget = crate::control::lease::planner::LeaseGraphBudget::new();
        let view = summary.view();
        let mut segment_idx = 0usize;
        while segment_idx < view.segment_count() {
            let segment = view.segment_at(segment_idx);
            let mut local = 0usize;
            while local < segment.len() {
                let node = segment.node_at_local(local);
                if matches!(node.kind, crate::eff::EffKind::Atom) {
                    let policy = segment.policy_at_local(local).unwrap_or(PolicyMode::Static);
                    budget = budget.include_atom(segment.control_desc_at_local(local), policy);
                }
                local += 1;
            }
            segment_idx += 1;
        }
        budget.validate();
        assert!(!budget.requires_caps());
        assert!(!budget.requires_topology());
        assert!(!budget.requires_delegation());

        assert_eq!(
            ControlSemanticKind::from_control_op(Some(ControlOp::RouteDecision)),
            ControlSemanticKind::RouteArm
        );
    }

    #[test]
    fn compiled_program_tracks_shared_route_controller_atlas() {
        let left = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>();
        let right = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
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
        with_compiled_program_facts(&summary, |compiled| {
            assert_eq!(compiled.route_controller_role(scope_id), Some(0));
            let (policy, eff_index, resource_tag, op) = compiled
                .route_controller(scope_id)
                .expect("route controller atlas entry");
            assert_eq!(
                policy,
                PolicyMode::dynamic(ROUTE_POLICY_ID).with_scope(scope_id)
            );
            assert_eq!(eff_index, EffIndex::ZERO);
            assert_eq!(resource_tag, LoopContinueKind::TAG);
            assert_eq!(op, ControlOp::LoopContinue);
        });
    }

    #[test]
    fn compiled_program_route_controller_atlas_ignores_nested_dynamic_policy_scopes() {
        let nested_left = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>();
        let nested_right = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            0,
        >()
        .policy::<ROUTE_POLICY_ID>();
        let left = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
                0,
            >(),
            g::route(nested_left, nested_right),
        );
        let right = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
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
        with_compiled_program_facts(&summary, |compiled| {
            assert_eq!(compiled.route_controller_role(scope_id), Some(0));
            assert_eq!(
                compiled.route_controller(scope_id),
                None,
                "nested dynamic policies must not promote the enclosing static route"
            );
        });
    }

    #[test]
    fn compiled_program_image_header_stays_compact() {
        assert!(
            size_of::<super::CompiledProgramFacts>() < 192,
            "CompiledProgramFacts header regressed back to pointer-rich layout: {} bytes",
            size_of::<super::CompiledProgramFacts>()
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
                    RouteDecisionKind,
                >,
                0,
            >()
            .policy::<ROUTE_POLICY_ID>(),
        );
        let summary = program.summary();
        let expected =
            CompiledProgramFacts::persistent_bytes_for_counts(summary.compiled_program_counts());
        with_compiled_program_facts(&summary, |image| {
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
        assert_eq!(
            ControlSemanticKind::from_control_op(Some(ControlOp::LoopContinue)),
            ControlSemanticKind::LoopContinue
        );
        assert_eq!(
            ControlSemanticKind::from_control_op(Some(ControlOp::LoopBreak)),
            ControlSemanticKind::LoopBreak
        );
    }

    #[test]
    fn control_semantic_kind_packed_bits_roundtrip() {
        let kinds = [
            ControlSemanticKind::Other,
            ControlSemanticKind::RouteArm,
            ControlSemanticKind::LoopContinue,
            ControlSemanticKind::LoopBreak,
        ];
        let mut idx = 0usize;
        while idx < kinds.len() {
            let kind = kinds[idx];
            assert_eq!(
                ControlSemanticKind::from_packed_bits(kind.packed_bits()),
                kind
            );
            idx += 1;
        }
    }

    #[test]
    fn compiled_program_skips_noop_control_scope_resets() {
        let cancel = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ABORT_CONTROL }, GenericCapToken<AbortControl>, AbortControl>,
            0,
        >();
        let fence = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_FENCE_CONTROL }, GenericCapToken<FenceControl>, FenceControl>,
            0,
        >();
        let route = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_ROUTE_DECISION }, GenericCapToken<RouteDecisionKind>, RouteDecisionKind>,
            0,
        >();
        let program = g::seq(cancel, g::seq(fence, route));

        let summary = program.summary();
        with_compiled_program_facts(&summary, |compiled| {
            let effect_envelope = compiled.effect_envelope();

            assert_eq!(effect_envelope.resources().count(), 3);
            assert!(
                effect_envelope.control_scopes().next().is_none(),
                "no-op control scopes should not stay in the runtime reset mask"
            );
        });
    }
}
