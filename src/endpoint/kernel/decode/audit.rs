use super::{BranchPreviewView, CursorEndpoint, EndpointRxAuditPlan};
use crate::{
    binding::EndpointSlot,
    control::{
        cap::mint::{EpochTable, MintConfigMarker},
        types::Lane,
    },
    observe::ids,
    policy_runtime::PolicySlot,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{Transport, wire::FrameFlags},
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
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
        self.emit_endpoint_policy_audit(
            PolicySlot::EndpointRx,
            ids::ENDPOINT_RECV,
            self.sid.raw(),
            Self::endpoint_policy_args(lane, plan.label, FrameFlags::empty()),
            lane,
        );
    }
}
