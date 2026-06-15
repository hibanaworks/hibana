use super::{EndpointResidentBudget, Rendezvous, ResourceScope, Transport};

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    pub(crate) fn ensure_core_lane_storage_for_lane_slots(
        &mut self,
        required_lane_slots: usize,
    ) -> Result<(), ResourceScope> {
        self.ensure_core_lane_tables_for_lane_slots(required_lane_slots)
    }

    pub(crate) fn ensure_endpoint_resident_budget(
        &mut self,
        budget: EndpointResidentBudget,
    ) -> Result<(), ResourceScope> {
        let route_frame_slots = core::cmp::max(
            self.resident_route_frame_slots_floor(),
            budget.route_frame_slots as usize,
        );
        let route_lane_slots = core::cmp::max(
            self.resident_route_lane_slots_floor(),
            budget.route_lane_slots as usize,
        );
        let frontier_workspace_bytes = core::cmp::max(
            self.resident_frontier_workspace_floor(),
            budget.frontier_workspace_bytes as usize,
        );
        self.ensure_frontier_workspace_capacity(frontier_workspace_bytes)?;
        self.ensure_route_table_capacity(route_frame_slots, route_lane_slots)?;
        Ok(())
    }
}
