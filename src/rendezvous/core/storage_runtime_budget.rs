use super::{
    CapTable, Clock, EndpointResidentBudget, LabelUniverse, LoopTable, Rendezvous, ResourceScope,
    RouteTable, Transport,
};

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    pub(crate) fn ensure_core_lane_storage_for_lane_slots(
        &mut self,
        required_lane_slots: usize,
    ) -> Option<()> {
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
        let loop_slots =
            core::cmp::max(self.resident_loop_slots_floor(), budget.loop_slots as usize);
        let cap_entries = core::cmp::max(
            self.resident_cap_entries_floor(),
            budget.cap_entries as usize,
        );
        let frontier_workspace_bytes = core::cmp::max(
            self.resident_frontier_workspace_floor(),
            budget.frontier_workspace_bytes as usize,
        );
        self.ensure_frontier_workspace_capacity(frontier_workspace_bytes)
            .ok_or(ResourceScope::EndpointLease)?;
        self.ensure_route_table_capacity(route_frame_slots, route_lane_slots)
            .ok_or(ResourceScope::RouteTable)?;
        self.ensure_loop_table_capacity(loop_slots)
            .ok_or(ResourceScope::LoopTable)?;
        self.ensure_cap_table_capacity(cap_entries)
            .ok_or(ResourceScope::CapTable)?;
        Ok(())
    }

    pub(crate) fn trim_resident_headers_to_live_budget(&mut self) {
        if self.resident_route_frame_slots_floor() == 0 && self.routes.route_slots() != 0 {
            self.free_bound_persistent_region(
                self.reclaim_offset_for_payload(
                    self.routes.storage_ptr(),
                    self.routes.storage_reclaim_delta(),
                ),
                self.routes.storage_ptr(),
                self.routes.storage_bytes_current(),
            );
            self.routes = RouteTable::empty();
        }
        if self.resident_loop_slots_floor() == 0 && self.loops.loop_slots() != 0 {
            self.free_bound_persistent_region(
                self.reclaim_offset_for_payload(
                    self.loops.storage_ptr(),
                    self.loops.storage_reclaim_delta(),
                ),
                self.loops.storage_ptr(),
                self.loops.storage_bytes_current(),
            );
            self.loops = LoopTable::empty();
        }
        if self.resident_cap_entries_floor() == 0 && self.caps.capacity() != 0 {
            self.free_bound_persistent_region(
                self.reclaim_offset_for_payload(
                    self.caps.storage_ptr(),
                    self.caps.storage_reclaim_delta(),
                ),
                self.caps.storage_ptr(),
                self.caps.storage_bytes_current(),
            );
            self.caps = CapTable::empty();
        }
    }
}
