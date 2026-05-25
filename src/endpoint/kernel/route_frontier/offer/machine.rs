use super::*;

pub(in crate::endpoint::kernel) struct RouteFrontierMachine<
    'endpoint,
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    U,
    C,
    E: EpochTable,
    const MAX_RV: usize,
    Mint,
    B: BindingSlot + 'r,
> where
    U: LabelUniverse,
    C: Clock,
    Mint: MintConfigMarker,
{
    pub(super) endpoint: &'endpoint mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    pub(super) frontier_visited: Option<FrontierVisitSet>,
    pub(super) carried_binding_evidence: Option<LaneIngressEvidence>,
    pub(super) carried_transport_payload: Option<lane_port::ReceivedFrame<'r>>,
    pub(super) run_stage: Option<OfferRunStage<'r>>,
    pub(super) pending_recv: lane_port::PendingRecv,
}

impl<'endpoint, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteFrontierMachine<'endpoint, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[inline]
    pub(super) fn discard_terminal_ingress(&mut self) {
        if let Some(payload) = self.carried_transport_payload.take() {
            payload.discard_uncommitted();
        }
        if let Some(stage) = self.run_stage.as_mut() {
            stage.discard_terminal();
        }
        self.run_stage = None;
    }
}

impl<'endpoint, 'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteFrontierMachine<'endpoint, 'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[inline]
    pub(in crate::endpoint::kernel) const fn new(
        endpoint: &'endpoint mut CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    ) -> Self {
        Self {
            endpoint,
            frontier_visited: None,
            carried_binding_evidence: None,
            carried_transport_payload: None,
            run_stage: None,
            pending_recv: lane_port::PendingRecv::new(),
        }
    }
}
