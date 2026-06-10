use crate::endpoint::kernel::EndpointArenaLayout;
use crate::global::role_program::RoleImageRef;

mod blob_storage;
mod columns;
mod program_ref;
mod role_descriptor_ref;
mod route_controls;

pub(crate) use self::{
    blob_storage::ProgramImageBytes, program_ref::CompiledProgramRef,
    role_descriptor_ref::RoleDescriptorRef,
};
/// Sealed runtime owner for role-local immutable compiled facts within a compiled program ref.
#[derive(Clone, Copy)]
pub(crate) struct RoleImageSlice<const ROLE: u8> {
    descriptor: RoleDescriptorRef,
}

impl<const ROLE: u8> RoleImageSlice<ROLE> {
    #[inline(always)]
    pub(crate) const fn from_resident(image: &'static RoleImageRef) -> Self {
        Self {
            descriptor: RoleDescriptorRef::from_resident(image),
        }
    }

    #[inline(always)]
    pub(crate) const fn descriptor(&self) -> RoleDescriptorRef {
        self.descriptor
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> &'static CompiledProgramRef {
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

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout(&self) -> EndpointArenaLayout {
        self.descriptor.endpoint_arena_layout()
    }
}
