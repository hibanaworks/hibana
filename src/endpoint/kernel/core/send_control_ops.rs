use super::*;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    fn record_loop_decision(
        &mut self,
        metadata: &LoopMetadata,
        decision: LoopDecision,
        lane: u8,
    ) -> SendResult<u16> {
        let idx = Self::loop_index(metadata.scope).ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(lane as usize);
        let disposition = match decision {
            LoopDecision::Continue => LoopDisposition::Continue,
            LoopDecision::Break => LoopDisposition::Break,
        };
        let arm = match decision {
            LoopDecision::Continue => 0,
            LoopDecision::Break => 1,
        };
        let epoch = port.record_loop_decision(idx, disposition);
        let ts = port.now32();
        let causal = TapEvent::make_causal_key(ROLE, idx);
        let arg1 = match decision {
            LoopDecision::Continue => ((idx as u32) << 16) | epoch as u32,
            LoopDecision::Break => ((idx as u32) << 16) | (epoch as u32) | 0x1,
        };
        let event = events::LoopDecision::with_causal_and_scope(
            ts,
            causal,
            self.sid.raw(),
            arg1,
            self.scope_trace(metadata.scope)
                .map(|t| t.pack())
                .unwrap_or(0),
        );
        emit(port.tap(), event);
        if metadata.scope.kind() == ScopeKind::Route {
            self.record_route_decision_for_scope_lanes(metadata.scope, arm, lane);
            self.emit_route_decision(metadata.scope, arm, RouteDecisionSource::Ack, lane);
        }
        Ok(epoch)
    }

    #[inline(never)]
    pub(crate) fn mint_local_loop_continue_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let mut loop_scope = meta.scope;
        let mut epoch = 0;
        let mut recorded_via_loop_metadata = false;
        if let Some(metadata) = self.cursor.loop_metadata_inner()
            && metadata.role == LoopRole::Controller
            && metadata.controller == ROLE
        {
            epoch = self.record_loop_decision(&metadata, LoopDecision::Continue, meta.lane)?;
            loop_scope = metadata.scope;
            recorded_via_loop_metadata = true;
        }
        if loop_scope.is_none() {
            return Err(SendError::PhaseInvariant);
        }
        if !recorded_via_loop_metadata && loop_scope.kind() == ScopeKind::Route {
            self.record_route_decision_for_scope_lanes(loop_scope, 0, meta.lane);
            self.emit_route_decision(loop_scope, 0, RouteDecisionSource::Ack, meta.lane);
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
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let mut loop_scope = meta.scope;
        let mut epoch = 0;
        let mut recorded_via_loop_metadata = false;
        if let Some(metadata) = self.cursor.loop_metadata_inner()
            && metadata.role == LoopRole::Controller
            && metadata.controller == ROLE
        {
            epoch = self.record_loop_decision(&metadata, LoopDecision::Break, meta.lane)?;
            loop_scope = metadata.scope;
            recorded_via_loop_metadata = true;
        }
        if loop_scope.is_none() {
            return Err(SendError::PhaseInvariant);
        }
        if !recorded_via_loop_metadata && loop_scope.kind() == ScopeKind::Route {
            self.record_route_decision_for_scope_lanes(loop_scope, 1, meta.lane);
            self.emit_route_decision(loop_scope, 1, RouteDecisionSource::Ack, meta.lane);
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
    pub(crate) fn mint_local_reroute_control(
        &mut self,
        meta: &SendMeta,
        shot: CapShot,
        lane: Lane,
        src_rv: RendezvousId,
        cp_lane: Lane,
        control: ControlDesc,
    ) -> SendResult<MintedControlToken>
    where
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsEndpointMint,
    {
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let signals = self.policy_signals_for_slot(PolicySlot::Route);
        let mut attrs = *signals.attrs();
        attrs.copy_from(&port.transport().metrics().attrs());
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let policy = cluster
            .policy_mode_for(
                src_rv,
                cp_lane,
                meta.eff_index,
                control.resource_tag(),
                control.op(),
            )
            .map_err(Self::map_cp_error)?;
        let handle = cluster
            .prepare_reroute_handle_from_policy(
                src_rv,
                cp_lane,
                meta.eff_index,
                control.resource_tag(),
                control.op(),
                policy,
                signals.input,
                &attrs,
            )
            .map_err(Self::map_cp_error)?;
        self.mint_descriptor_token_bytes(
            meta.peer,
            shot,
            lane,
            meta.scope,
            0,
            control,
            handle.encode(),
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
    ) -> SendResult<MintedControlToken>
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
        self.record_route_decision_for_scope_lanes(scope, arm, meta.lane);
        self.emit_route_decision(scope, arm, RouteDecisionSource::Resolver, meta.lane);
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
    ) -> SendResult<MintedControlToken>
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
    ) -> SendResult<MintedControlToken>
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
        let operands = cluster
            .take_cached_topology_operands(cp_sid)
            .or_else(|| cluster.distributed_topology_operands(cp_sid))
            .ok_or(SendError::PhaseInvariant)?;
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
    fn mint_control_token_bytes_with_handle<K>(
        &mut self,
        peer: u8,
        shot: CapShot,
        lane: Lane,
        scope: ScopeId,
        epoch: u16,
        handle: K::Handle,
    ) -> SendResult<MintedControlToken>
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
