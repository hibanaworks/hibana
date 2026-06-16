use super::{BranchPreviewView, CursorEndpoint, EndpointRxAuditPlan};
use crate::{
    observe::ids,
    resolver_audit::ResolverSlot,
    session::types::Lane,
    transport::{Transport, wire::FrameFlags},
};

impl<'r, const ROLE: u8, T> CursorEndpoint<'r, ROLE, T>
where
    T: Transport + 'r,
{
    pub(super) fn build_endpoint_rx_audit_plan(
        &self,
        branch: BranchPreviewView,
    ) -> EndpointRxAuditPlan {
        EndpointRxAuditPlan {
            lane: branch.branch_meta.lane_wire,
            label: branch.label,
        }
    }

    pub(super) fn publish_endpoint_rx_audit(&self, plan: EndpointRxAuditPlan) {
        let lane = Lane::new(plan.lane as u32);
        self.emit_endpoint_resolver_audit(
            ResolverSlot::EndpointRx,
            ids::ENDPOINT_RECV,
            self.sid.raw(),
            Self::endpoint_resolver_args(lane, plan.label, FrameFlags::empty()),
            lane,
        );
    }
}
