use crate::{
    control::cluster::effects::{EffectEnvelopeRef, ProgramImageDynamicPolicySiteIter},
    endpoint::kernel::EndpointArenaLayout,
    global::const_dsl::{PolicyMode, ScopeId},
};
#[cfg(all(test, hibana_repo_tests))]
use crate::{
    eff::{EffIndex, EffKind},
    global::const_dsl::ScopeKind,
    global::typestate::{LocalAtomFacts, LocalNode, LocalNodeMeta, StateIndex},
};

#[cfg(all(test, hibana_repo_tests))]
use super::program::ControlSemanticKind;
use super::{
    program::{ControlSemanticsTable, DynamicPolicySite},
    role::CompiledRoleImage,
};
use crate::global::compiled::lowering::{CompiledProgramImage, ProgramStamp};

mod role_descriptor_ref;
pub(crate) use self::role_descriptor_ref::RoleDescriptorRef;
#[cfg(all(test, hibana_repo_tests))]
#[inline(always)]
fn same_scope(left: ScopeId, right: ScopeId) -> bool {
    !left.is_none() && left.canonical_raw() == right.canonical_raw()
}

/// Sealed runtime owner for immutable program-wide compiled facts.
#[derive(Clone, Copy)]
pub(crate) struct CompiledProgramRef {
    stamp: ProgramStamp,
    image: &'static CompiledProgramImage,
}

impl core::fmt::Debug for CompiledProgramRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompiledProgramRef")
            .field("stamp", &self.stamp.words())
            .field("image", &(self.image as *const CompiledProgramImage))
            .finish()
    }
}

impl PartialEq for CompiledProgramRef {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.stamp.words() == other.stamp.words() && core::ptr::eq(self.image, other.image)
    }
}

impl Eq for CompiledProgramRef {}

impl CompiledProgramRef {
    #[inline(always)]
    pub(crate) const fn resident(
        stamp: ProgramStamp,
        image: &'static CompiledProgramImage,
    ) -> Self {
        Self { stamp, image }
    }

    #[inline(always)]
    pub(crate) fn effect_envelope(&self) -> EffectEnvelopeRef<'_> {
        EffectEnvelopeRef::from_program_image(self.image)
    }

    #[inline(always)]
    pub(crate) fn role_count(&self) -> usize {
        self.image.compiled_program_role_count()
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites_for(
        &self,
        policy_id: u16,
    ) -> impl Iterator<Item = DynamicPolicySite> + '_ {
        ProgramImageDynamicPolicySiteIter::new(self.image)
            .filter(move |site| site.policy_id() == policy_id)
    }

    #[inline(always)]
    pub(crate) fn control_semantics(&self) -> &'static ControlSemanticsTable {
        &super::program::CONTROL_SEMANTICS_TABLE
    }

    pub(crate) fn validate_label_universe(
        &self,
        max: u8,
    ) -> Result<(), crate::global::role_program::LabelUniverseViolation> {
        if max == u8::MAX {
            return Ok(());
        }

        let view = self.image.view();
        let mut idx = 0usize;
        while idx < view.len() {
            let node = view.node_at(idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let actual = node.atom_data().label;
                if actual > max {
                    return Err(crate::global::role_program::LabelUniverseViolation {
                        max,
                        actual,
                    });
                }
            }
            idx += 1;
        }

        Ok(())
    }

    #[inline(always)]
    pub(crate) fn route_controller_role(&self, scope_id: ScopeId) -> Option<u8> {
        crate::global::compiled::lowering::program_lowering::compiled_program_route_control_for_scope(
            self.image,
            scope_id,
        )
        .and_then(|record| record.controller_role())
    }

    #[inline(always)]
    pub(crate) fn route_controller(
        &self,
        scope_id: ScopeId,
    ) -> Option<(
        PolicyMode,
        crate::eff::EffIndex,
        u8,
        crate::control::cap::mint::ControlOp,
    )> {
        crate::global::compiled::lowering::program_lowering::compiled_program_route_control_for_scope(
            self.image,
            scope_id,
        )
        .and_then(|record| record.route_controller())
    }
}

/// Sealed runtime owner for role-local immutable compiled facts within a compiled program ref.
#[derive(Clone, Copy)]
pub(crate) struct RoleImageSlice<const ROLE: u8> {
    descriptor: RoleDescriptorRef,
}

impl<const ROLE: u8> RoleImageSlice<ROLE> {
    #[inline(always)]
    pub(crate) const fn from_resident(compiled: &'static CompiledRoleImage) -> Self {
        Self {
            descriptor: RoleDescriptorRef::from_resident(compiled),
        }
    }

    #[inline(always)]
    pub(crate) const fn descriptor(&self) -> RoleDescriptorRef {
        self.descriptor
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> CompiledProgramRef {
        self.descriptor.program()
    }

    #[inline(always)]
    pub(crate) fn has_active_lane(&self, lane_idx: usize) -> bool {
        self.descriptor.has_active_lane(lane_idx)
    }

    #[inline(always)]
    pub(crate) fn first_active_lane(&self) -> Option<usize> {
        self.descriptor.first_active_lane()
    }

    #[inline(always)]
    pub(crate) fn endpoint_lane_slot_count(&self) -> usize {
        self.descriptor.endpoint_lane_slot_count()
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        self.descriptor.logical_lane_count()
    }

    #[inline(always)]
    pub(crate) fn route_table_frame_slots(&self) -> usize {
        self.descriptor.route_table_frame_slots()
    }

    #[inline(always)]
    pub(crate) fn route_table_lane_slots(&self) -> usize {
        self.descriptor.route_table_lane_slots()
    }

    #[inline(always)]
    pub(crate) fn loop_table_slots(&self) -> usize {
        self.descriptor.loop_table_slots()
    }

    #[inline(always)]
    pub(crate) fn resident_cap_entries(&self) -> usize {
        self.descriptor.resident_cap_entries()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn active_lane_count(&self) -> usize {
        self.descriptor.active_lane_count()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn max_route_stack_depth(&self) -> usize {
        self.descriptor.max_route_stack_depth()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn max_loop_stack_depth(&self) -> usize {
        self.descriptor.max_loop_stack_depth()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.descriptor.route_scope_count()
    }

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout(&self) -> EndpointArenaLayout {
        self.descriptor.endpoint_arena_layout()
    }
}
