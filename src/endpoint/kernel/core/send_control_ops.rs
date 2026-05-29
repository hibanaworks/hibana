use super::{
    CAP_HANDLE_LEN, CapShot, ControlDesc, CursorEndpoint, EndpointSlot, EpochTable, LabelUniverse,
    Lane, LoopBreakKind, LoopContinueKind, LoopDecisionHandle, LoopRole, MintConfigMarker,
    MintedControlToken, RendezvousId, ResourceKind, RouteArmHandle, ScopeId, ScopeKind, SendError,
    SendMeta, SendResult, SessionId, TopologyDescriptor, Transport, validate_route_decision_scope,
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
        self.mint_control_token_bytes_with_handle::<LoopContinueKind>(
            meta.peer,
            shot,
            lane,
            loop_scope,
            epoch,
            LoopDecisionHandle {
                sid: self.sid.raw(),
                lane: lane.as_wire(),
                scope: loop_scope,
            },
        )
    }

    #[inline(never)]
    pub(crate) fn mint_local_loop_break_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
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
        self.mint_control_token_bytes_with_handle::<LoopBreakKind>(
            meta.peer,
            shot,
            lane,
            loop_scope,
            epoch,
            LoopDecisionHandle {
                sid: self.sid.raw(),
                lane: lane.as_wire(),
                scope: loop_scope,
            },
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
            RouteArmHandle { scope, arm }.encode(),
        )
    }

    #[inline(never)]
    pub(crate) fn mint_local_topology_begin_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
        src_rv: RendezvousId,
        cp_lane: Lane,
        control: ControlDesc,
        descriptor_handle: [u8; CAP_HANDLE_LEN],
    ) -> SendResult<MintedControlToken<'r>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let descriptor = TopologyDescriptor::decode_for(control.op(), descriptor_handle)
            .map_err(Self::map_cp_error)?;
        let operands = cluster
            .prepare_topology_operands_from_descriptor(src_rv, cp_lane, control, descriptor)
            .map_err(Self::map_cp_error)?;
        self.mint_descriptor_token_bytes(
            meta.peer,
            shot,
            lane,
            meta.scope,
            0,
            control,
            Self::topology_handle_from_operands(operands).encode(),
        )
    }

    #[inline(never)]
    pub(crate) fn mint_local_topology_ack_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
        cp_sid: SessionId,
        control: ControlDesc,
        descriptor_handle: [u8; CAP_HANDLE_LEN],
    ) -> SendResult<MintedControlToken<'r>>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let rv_id = RendezvousId::new(self.rendezvous_id().raw());
        let cp_lane = Lane::new(lane.raw());
        let descriptor = TopologyDescriptor::decode_for(control.op(), descriptor_handle)
            .map_err(Self::map_cp_error)?;
        let preview_operands = cluster
            .cached_topology_operands(cp_sid)
            .or_else(|| cluster.distributed_topology_operands(cp_sid))
            .ok_or(SendError::PhaseInvariant)?;
        cluster
            .validate_topology_operands_from_descriptor(
                rv_id,
                cp_lane,
                control,
                descriptor,
                preview_operands,
            )
            .map_err(Self::map_cp_error)?;
        self.mint_descriptor_token_bytes(
            meta.peer,
            shot,
            lane,
            meta.scope,
            0,
            control,
            Self::topology_handle_from_operands(preview_operands).encode(),
        )
    }

    #[inline(never)]
    fn mint_control_token_bytes_with_handle<K>(
        &mut self,
        peer: u8,
        shot: CapShot,
        lane: Lane,
        scope: ScopeId,
        epoch: u16,
        handle: K::Handle,
    ) -> SendResult<MintedControlToken<'r>>
    where
        K: ResourceKind + crate::control::cap::mint::ControlResourceKind,
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        self.mint_descriptor_token_bytes(
            peer,
            shot,
            lane,
            scope,
            epoch,
            ControlDesc::of::<K>(),
            K::encode_handle(&handle),
        )
    }
}
