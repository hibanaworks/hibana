use super::{
    CapShot, ControlDesc, CursorEndpoint, EpochTable, LabelUniverse, Lane, LoopDecisionHandle,
    LoopRole, MintConfigMarker, MintedControlToken, SendError, SendMeta, SendResult, Transport,
};
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
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
        if !planned_via_loop_metadata && self.cursor.route_scope_slot(loop_scope).is_some() {
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
        if !planned_via_loop_metadata && self.cursor.route_scope_slot(loop_scope).is_some() {
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
}
