use super::{
    CapShot, ControlDesc, CursorEndpoint, EndpointSlot, EpochTable, LabelUniverse, Lane,
    LoopDecisionHandle, LoopRole, MintConfigMarker, MintedControlToken, RendezvousId,
    RouteArmHandle, ScopeKind, SendError, SendMeta, SendResult, Transport,
    validate_route_decision_scope,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot,
{
    #[inline(never)]
    pub(crate) fn mint_local_loop_continue_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
        control: ControlDesc,
    ) -> SendResult<MintedControlToken<'r>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let mut loop_scope = meta.scope;
        let mut epoch = 0;
        let mut planned_via_loop_metadata = false;
        if let Some(metadata) = self.cursor.loop_metadata_inner()
            && metadata.role == LoopRole::Controller
            && metadata.controller == ROLE
        {
            Self::loop_index(metadata.scope).ok_or(SendError::PhaseInvariant)?;
            loop_scope = metadata.scope;
            planned_via_loop_metadata = true;
        }
        if loop_scope.is_none() {
            return Err(SendError::PhaseInvariant);
        }
        if !planned_via_loop_metadata && loop_scope.kind() == ScopeKind::Route {
            // Route/loop control-token epochs are pre-publication descriptor
            // markers. The authority epoch is advanced later when the
            // prepared send commit publishes the decision proof.
            epoch = self.port_for_lane(meta.lane as usize).route_change_epoch();
        }
        self.mint_descriptor_token_bytes(
            meta.peer,
            shot,
            lane,
            loop_scope,
            epoch,
            control,
            LoopDecisionHandle::new(self.sid.raw(), lane.as_wire()).encode(),
        )
    }

    #[inline(never)]
    pub(crate) fn mint_local_loop_break_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
        control: ControlDesc,
    ) -> SendResult<MintedControlToken<'r>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let mut loop_scope = meta.scope;
        let mut epoch = 0;
        let mut planned_via_loop_metadata = false;
        if let Some(metadata) = self.cursor.loop_metadata_inner()
            && metadata.role == LoopRole::Controller
            && metadata.controller == ROLE
        {
            Self::loop_index(metadata.scope).ok_or(SendError::PhaseInvariant)?;
            loop_scope = metadata.scope;
            planned_via_loop_metadata = true;
        }
        if loop_scope.is_none() {
            return Err(SendError::PhaseInvariant);
        }
        if !planned_via_loop_metadata && loop_scope.kind() == ScopeKind::Route {
            // Route/loop control-token epochs are pre-publication descriptor
            // markers. The authority epoch is advanced later when the
            // prepared send commit publishes the decision proof.
            epoch = self.port_for_lane(meta.lane as usize).route_change_epoch();
        }
        self.mint_descriptor_token_bytes(
            meta.peer,
            shot,
            lane,
            loop_scope,
            epoch,
            control,
            LoopDecisionHandle::new(self.sid.raw(), lane.as_wire()).encode(),
        )
    }

    #[inline(never)]
    pub(crate) fn mint_local_route_decision_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
        src_rv: RendezvousId,
        cp_lane: Lane,
        control: ControlDesc,
    ) -> SendResult<MintedControlToken<'r>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let policy = cluster
            .policy_mode_for(
                src_rv,
                cp_lane,
                meta.eff_index,
                control.resource_tag(),
                control.op(),
            )
            .map_err(|_| SendError::PhaseInvariant)?;
        let scope = meta.scope;
        validate_route_decision_scope(scope, policy.scope())?;
        let arm = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
        if arm > 1 {
            return Err(SendError::PhaseInvariant);
        }
        // Route/loop control-token epochs are pre-publication descriptor
        // markers. The authority epoch is advanced later when the prepared send
        // commit publishes the decision proof.
        let epoch = self.port_for_lane(meta.lane as usize).route_change_epoch();
        self.mint_descriptor_token_bytes(
            meta.peer,
            shot,
            lane,
            scope,
            epoch,
            control,
            RouteArmHandle::new(arm)
                .map_err(|_| SendError::PhaseInvariant)?
                .encode(),
        )
    }
}
