use crate::{control::cluster::effects::EffectEnvelopeRef, endpoint::kernel::EndpointArenaLayout};

use super::{
    ControlSemanticsTable, ProgramStamp,
    program::{CompiledProgramImage, DynamicPolicySite},
    role::CompiledRoleImage,
};

/// Sealed runtime owner for immutable program-wide compiled facts.
#[derive(Clone, Copy)]
pub(crate) struct ProgramImage {
    stamp: ProgramStamp,
    compiled: *const CompiledProgramImage,
}

impl ProgramImage {
    #[inline(always)]
    pub(crate) unsafe fn from_raw(
        stamp: ProgramStamp,
        compiled: *const CompiledProgramImage,
    ) -> Self {
        debug_assert!(!compiled.is_null());
        Self { stamp, compiled }
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp {
        self.stamp
    }

    #[inline(always)]
    fn compiled(&self) -> &CompiledProgramImage {
        debug_assert!(!self.compiled.is_null());
        unsafe { &*self.compiled }
    }

    #[inline(always)]
    pub(crate) fn effect_envelope(&self) -> EffectEnvelopeRef<'_> {
        self.compiled().effect_envelope()
    }

    #[inline(always)]
    pub(crate) fn role_count(&self) -> usize {
        self.compiled().role_count()
    }

    #[inline(always)]
    pub(crate) fn dynamic_policy_sites_for(
        &self,
        policy_id: u16,
    ) -> impl Iterator<Item = &DynamicPolicySite> + '_ {
        self.compiled().dynamic_policy_sites_for(policy_id)
    }

    #[inline(always)]
    pub(crate) fn control_semantics(&self) -> &ControlSemanticsTable {
        self.compiled().control_semantics()
    }

    #[inline(always)]
    pub(crate) fn control_semantics_ptr(&self) -> *const ControlSemanticsTable {
        self.control_semantics() as *const ControlSemanticsTable
    }
}

/// Sealed runtime owner for role-local immutable compiled facts within a `ProgramImage`.
#[derive(Clone, Copy)]
pub(crate) struct RoleImageSlice<const ROLE: u8> {
    program: ProgramImage,
    compiled: *const CompiledRoleImage,
}

impl<const ROLE: u8> RoleImageSlice<ROLE> {
    #[inline(always)]
    pub(crate) unsafe fn from_raw(
        program: ProgramImage,
        compiled: *const CompiledRoleImage,
    ) -> Self {
        debug_assert!(!compiled.is_null());
        Self { program, compiled }
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> ProgramImage {
        self.program
    }

    #[inline(always)]
    pub(crate) const fn compiled_ptr(&self) -> *const CompiledRoleImage {
        self.compiled
    }

    #[inline(always)]
    fn compiled(&self) -> &CompiledRoleImage {
        debug_assert!(!self.compiled.is_null());
        unsafe { &*self.compiled }
    }

    #[inline(always)]
    pub(crate) fn active_lane_mask(&self) -> u8 {
        self.compiled().active_lane_mask()
    }

    #[inline(always)]
    pub(crate) fn endpoint_lane_slot_count(&self) -> usize {
        self.compiled().endpoint_lane_slot_count()
    }

    #[inline(always)]
    pub(crate) fn route_table_frame_slots(&self) -> usize {
        self.compiled().route_table_frame_slots()
    }

    #[inline(always)]
    pub(crate) fn route_table_lane_slots(&self) -> usize {
        self.compiled().route_table_lane_slots()
    }

    #[inline(always)]
    pub(crate) fn loop_table_slots(&self) -> usize {
        self.compiled().loop_table_slots()
    }

    #[inline(always)]
    pub(crate) fn resident_cap_entries(&self) -> usize {
        self.compiled().resident_cap_entries()
    }

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout_for_binding(
        &self,
        binding_enabled: bool,
    ) -> EndpointArenaLayout {
        self.compiled()
            .endpoint_arena_layout_for_binding(binding_enabled)
    }
}
