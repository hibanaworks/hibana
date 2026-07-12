use super::{Rendezvous, ResourceScope, Transport};

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    pub(crate) fn ensure_core_lane_storage_for_assoc_entries(
        &self,
        required_lane_slots: usize,
        required_assoc_slots: usize,
    ) -> Result<(), ResourceScope> {
        self.ensure_core_lane_tables_for_assoc_entries(required_lane_slots, required_assoc_slots)
    }

    pub(crate) fn ensure_endpoint_resident_capacity(&self) -> Result<(), ResourceScope> {
        let route_frame_slots = self.resident_route_frame_slots_floor();
        let frontier_workspace_bytes = self.resident_frontier_workspace_floor();
        self.ensure_frontier_workspace_capacity(frontier_workspace_bytes)?;
        self.ensure_route_table_capacity(route_frame_slots)?;
        Ok(())
    }
}
