use super::{
    BindingLanePreference, CursorEndpoint, EndpointSlot, EpochTable, FrameLabelMask, LabelUniverse,
    LaneSetView, MintConfigMarker, PackedIngressEvidence, Payload, RecvError, RecvResult,
    RestoredBindingPayload, ScopeArmMaterializationMeta, ScopeFrameLabelMeta, ScopeId, Transport,
    lane_port, next_preferred_lane_in_lane_set,
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
    fn take_binding_for_lane(
        &mut self,
        lane_idx: usize,
    ) -> Option<crate::binding::IngressEvidence> {
        let previous_nonempty = self.binding_inbox.nonempty_lanes().contains(lane_idx);
        let evidence = self.binding_inbox.take_or_poll(&mut self.binding, lane_idx);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty);
        evidence
    }

    #[inline]
    pub(crate) fn take_restored_binding_payload(
        &mut self,
        lane_idx: usize,
        evidence: crate::binding::IngressEvidence,
    ) -> Option<Payload<'r>> {
        match self.restored_binding_payload {
            Some(restored) if restored.matches(lane_idx, evidence) => {
                self.restored_binding_payload = None;
                Some(restored.payload)
            }
            Some(_) | None => None,
        }
    }

    #[inline]
    pub(in crate::endpoint::kernel) fn restore_binding_payload_for_lane(
        &mut self,
        lane_idx: usize,
        evidence: crate::binding::IngressEvidence,
        payload: Payload<'r>,
    ) {
        debug_assert!(
            self.restored_binding_payload.is_none(),
            "at most one restored binding payload may be staged per endpoint"
        );
        self.restored_binding_payload = Some(RestoredBindingPayload {
            lane: lane_idx as u8,
            evidence: PackedIngressEvidence::encode(evidence),
            payload,
        });
        self.put_back_binding_for_lane(lane_idx, evidence);
    }

    pub(crate) fn put_back_binding_for_lane(
        &mut self,
        lane_idx: usize,
        evidence: crate::binding::IngressEvidence,
    ) {
        let previous_nonempty = self.binding_inbox.nonempty_lanes().contains(lane_idx);
        self.binding_inbox.put_back(lane_idx, evidence);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty);
    }

    pub(crate) fn take_matching_binding_for_lane(
        &mut self,
        lane_idx: usize,
        expected_frame_label: u8,
    ) -> Option<crate::binding::IngressEvidence> {
        let previous_nonempty = self.binding_inbox.nonempty_lanes().contains(lane_idx);
        let evidence = self.binding_inbox.take_matching_or_poll(
            &mut self.binding,
            lane_idx,
            expected_frame_label,
        );
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty);
        evidence
    }

    fn take_matching_mask_binding_for_lane<F: FnMut(u8) -> bool>(
        &mut self,
        lane_idx: usize,
        frame_label_mask: FrameLabelMask,
        drop_frame_label_mask: FrameLabelMask,
        drop_mismatch: F,
    ) -> Option<crate::binding::IngressEvidence> {
        let previous_nonempty = self.binding_inbox.nonempty_lanes().contains(lane_idx);
        let evidence = self.binding_inbox.take_matching_mask_or_poll(
            &mut self.binding,
            lane_idx,
            frame_label_mask,
            drop_frame_label_mask,
            drop_mismatch,
        );
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty);
        evidence
    }

    #[inline]
    fn take_binding_mask_ignoring_loop_control(
        &mut self,
        lane_idx: usize,
        frame_label_mask: FrameLabelMask,
        drop_frame_label_mask: FrameLabelMask,
    ) -> Option<crate::binding::IngressEvidence> {
        self.take_matching_mask_binding_for_lane(
            lane_idx,
            frame_label_mask,
            drop_frame_label_mask,
            move |_| true,
        )
    }

    #[cfg(test)]
    pub(in crate::endpoint::kernel) fn take_binding_for_selected_arm(
        &mut self,
        lane_idx: usize,
        selected_arm: u8,
        frame_label_meta: ScopeFrameLabelMeta,
        binding_evidence: &mut Option<crate::binding::IngressEvidence>,
    ) -> Option<crate::binding::IngressEvidence> {
        let frame_label_mask =
            frame_label_meta.binding_demux_frame_label_mask_for_arm(selected_arm);
        let drop_frame_label_mask = self.loop_control_drop_frame_label_mask(frame_label_meta);

        if let Some(evidence) = binding_evidence.take() {
            if frame_label_mask.contains_frame_label(evidence.frame_label.raw()) {
                return Some(evidence);
            } else {
                self.put_back_binding_for_lane(lane_idx, evidence);
            }
        }

        self.take_binding_mask_ignoring_loop_control(
            lane_idx,
            frame_label_mask,
            drop_frame_label_mask,
        )
    }

    pub(in crate::endpoint::kernel) fn poll_binding_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
    ) -> Option<(usize, crate::binding::IngressEvidence)> {
        self.poll_binding_for_offer_lanes(
            scope_id,
            offer_lane_idx,
            self.offer_lane_set_for_scope(scope_id),
            frame_label_meta,
            materialization_meta,
        )
    }

    pub(in crate::endpoint::kernel) fn poll_binding_for_offer_lanes(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
    ) -> Option<(usize, crate::binding::IngressEvidence)> {
        if offer_lanes.is_empty() {
            return None;
        }
        let preferred_arm = self
            .peek_scope_ack(scope_id)
            .map(|token| token.arm().as_u8());
        let mut frame_label_mask =
            frame_label_meta.preferred_binding_frame_label_mask(preferred_arm);
        if frame_label_mask.is_empty()
            && self.static_passive_scope_evidence_materializes_poll(scope_id)
        {
            frame_label_mask = frame_label_meta.binding_demux_frame_label_mask_for_arm(0)
                | frame_label_meta.binding_demux_frame_label_mask_for_arm(1);
        }
        if frame_label_mask.is_empty() {
            return None;
        }
        let preference = if let Some(arm) = preferred_arm
            && self.offer_lanes_contain_binding_preference(
                offer_lanes,
                frame_label_meta,
                materialization_meta,
                BindingLanePreference::Arm(arm),
            ) {
            BindingLanePreference::Arm(arm)
        } else if self.offer_lanes_contain_binding_preference(
            offer_lanes,
            frame_label_meta,
            materialization_meta,
            BindingLanePreference::LabelMask(frame_label_mask),
        ) {
            BindingLanePreference::LabelMask(frame_label_mask)
        } else {
            BindingLanePreference::Any
        };
        if let Some(expected_frame_label) =
            frame_label_meta.preferred_binding_frame_label(preferred_arm)
        {
            if let Some(picked) = self.poll_binding_exact_for_offer(
                offer_lane_idx,
                offer_lanes,
                expected_frame_label,
                frame_label_meta,
                materialization_meta,
                preference,
            ) {
                return Some(picked);
            }
        }
        if let Some(evidence) = self.poll_binding_mask_for_offer(
            offer_lane_idx,
            offer_lanes,
            frame_label_mask,
            frame_label_meta,
            materialization_meta,
            preference,
        ) {
            return Some(evidence);
        }
        if self.static_passive_scope_evidence_materializes_poll(scope_id)
            && let Some((lane_idx, evidence)) =
                self.poll_binding_any_for_offer(offer_lane_idx, offer_lanes)
        {
            if self
                .static_passive_dispatch_arm_from_exact_frame_label(
                    scope_id,
                    lane_idx as u8,
                    evidence.frame_label.raw(),
                )
                .is_some()
            {
                return Some((lane_idx, evidence));
            }
            self.put_back_binding_for_lane(lane_idx, evidence);
        }
        None
    }

    fn poll_binding_mask_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        frame_label_mask: FrameLabelMask,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> Option<(usize, crate::binding::IngressEvidence)> {
        let drop_frame_label_mask = self.loop_control_drop_frame_label_mask(frame_label_meta);
        if let Some(evidence) = self.poll_buffered_binding_mask_for_offer(
            offer_lane_idx,
            offer_lanes,
            frame_label_mask,
            FrameLabelMask::EMPTY,
            false,
            frame_label_mask,
            drop_frame_label_mask,
            frame_label_meta,
            materialization_meta,
            preference,
        ) {
            return Some(evidence);
        }
        if let Some(evidence) = self.poll_buffered_binding_mask_for_offer(
            offer_lane_idx,
            offer_lanes,
            drop_frame_label_mask,
            frame_label_mask,
            true,
            frame_label_mask,
            drop_frame_label_mask,
            frame_label_meta,
            materialization_meta,
            preference,
        ) {
            return Some(evidence);
        }
        self.poll_binding_mask_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            frame_label_mask,
            drop_frame_label_mask,
            frame_label_meta,
            materialization_meta,
            preference,
        )
    }

    fn poll_buffered_binding_mask_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        buffered_frame_label_mask: FrameLabelMask,
        excluded_buffered_mask: FrameLabelMask,
        require_preference: bool,
        frame_label_mask: FrameLabelMask,
        drop_frame_label_mask: FrameLabelMask,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> Option<(usize, crate::binding::IngressEvidence)> {
        let lane_limit = self.cursor.logical_lane_count();
        let mut scan_idx = 0usize;
        while let Some(lane_slot) = Self::next_preferred_lane_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            lane_limit,
            &mut scan_idx,
        ) {
            if !self
                .binding_inbox
                .lane_has_buffered_frame_label(lane_slot, buffered_frame_label_mask)
                || (!excluded_buffered_mask.is_empty()
                    && self
                        .binding_inbox
                        .lane_has_buffered_frame_label(lane_slot, excluded_buffered_mask))
                || (require_preference
                    && !self.offer_lane_matches_binding_preference(
                        frame_label_meta,
                        materialization_meta,
                        preference,
                        lane_slot,
                    ))
            {
                continue;
            }
            if let Some(evidence) = self.take_binding_mask_ignoring_loop_control(
                lane_slot,
                frame_label_mask,
                drop_frame_label_mask,
            ) {
                return Some((lane_slot, evidence));
            }
        }
        None
    }

    fn poll_binding_mask_in_lane_set(
        &mut self,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        frame_label_mask: FrameLabelMask,
        drop_frame_label_mask: FrameLabelMask,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> Option<(usize, crate::binding::IngressEvidence)> {
        let lane_limit = self.cursor.logical_lane_count();
        let excluded_mask = frame_label_mask | drop_frame_label_mask;
        let mut scan_idx = 0usize;
        while let Some(lane_slot) = Self::next_preferred_lane_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            lane_limit,
            &mut scan_idx,
        ) {
            if self
                .binding_inbox
                .lane_has_buffered_frame_label(lane_slot, excluded_mask)
                || !self.offer_lane_matches_binding_preference(
                    frame_label_meta,
                    materialization_meta,
                    preference,
                    lane_slot,
                )
            {
                continue;
            }
            return self
                .take_binding_mask_ignoring_loop_control(
                    lane_slot,
                    frame_label_mask,
                    drop_frame_label_mask,
                )
                .map(|evidence| (lane_slot, evidence));
        }
        None
    }

    fn poll_binding_exact_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        expected_frame_label: u8,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> Option<(usize, crate::binding::IngressEvidence)> {
        let expected_frame_label_mask = FrameLabelMask::from_frame_label(expected_frame_label);
        if let Some(evidence) = self.poll_binding_exact_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            expected_frame_label,
            expected_frame_label_mask,
            true,
            frame_label_meta,
            materialization_meta,
            preference,
        ) {
            return Some(evidence);
        }
        self.poll_binding_exact_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            expected_frame_label,
            expected_frame_label_mask,
            false,
            frame_label_meta,
            materialization_meta,
            preference,
        )
    }

    fn poll_binding_exact_in_lane_set(
        &mut self,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
        expected_frame_label: u8,
        expected_frame_label_mask: FrameLabelMask,
        buffered_only: bool,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> Option<(usize, crate::binding::IngressEvidence)> {
        let lane_limit = self.cursor.logical_lane_count();
        let mut scan_idx = 0usize;
        while let Some(lane_idx) = Self::next_preferred_lane_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            lane_limit,
            &mut scan_idx,
        ) {
            let has_buffered = self
                .binding_inbox
                .lane_has_buffered_frame_label(lane_idx, expected_frame_label_mask);
            if buffered_only {
                if !has_buffered {
                    continue;
                }
            } else if has_buffered
                || !self.offer_lane_matches_binding_preference(
                    frame_label_meta,
                    materialization_meta,
                    preference,
                    lane_idx,
                )
            {
                continue;
            }
            if let Some(evidence) =
                self.take_matching_binding_for_lane(lane_idx, expected_frame_label)
            {
                return Some((lane_idx, evidence));
            }
        }
        None
    }

    pub(crate) fn poll_binding_any_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lanes: LaneSetView,
    ) -> Option<(usize, crate::binding::IngressEvidence)> {
        if offer_lanes.is_empty() {
            return None;
        }
        let lane_limit = self.cursor.logical_lane_count();
        let mut scan_idx = 0usize;
        while let Some(lane_idx) = Self::next_preferred_lane_in_lane_set(
            offer_lane_idx,
            offer_lanes,
            lane_limit,
            &mut scan_idx,
        ) {
            if let Some(evidence) = self.take_binding_for_lane(lane_idx) {
                return Some((lane_idx, evidence));
            }
        }
        None
    }

    #[inline]
    fn offer_lanes_contain_binding_preference(
        &self,
        offer_lanes: LaneSetView,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
    ) -> bool {
        let lane_limit = self.cursor.logical_lane_count();
        let mut next = offer_lanes.first_set(lane_limit);
        while let Some(lane_idx) = next {
            if self.offer_lane_matches_binding_preference(
                frame_label_meta,
                materialization_meta,
                preference,
                lane_idx,
            ) {
                return true;
            }
            next = offer_lanes.next_set_from(lane_idx.saturating_add(1), lane_limit);
        }
        false
    }

    #[inline]
    fn offer_lane_matches_binding_preference(
        &self,
        frame_label_meta: ScopeFrameLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        preference: BindingLanePreference,
        lane_idx: usize,
    ) -> bool {
        match preference {
            BindingLanePreference::Any => true,
            BindingLanePreference::Arm(arm) => {
                self.binding_demux_contains_lane(materialization_meta, Some(arm), lane_idx)
            }
            BindingLanePreference::LabelMask(frame_label_mask) => self
                .binding_demux_contains_lane_for_frame_label_mask(
                    materialization_meta,
                    frame_label_meta,
                    frame_label_mask,
                    lane_idx,
                ),
        }
    }

    #[inline]
    fn next_preferred_lane_in_lane_set(
        preferred_lane_idx: usize,
        offer_lanes: LaneSetView,
        lane_limit: usize,
        scan_idx: &mut usize,
    ) -> Option<usize> {
        next_preferred_lane_in_lane_set(preferred_lane_idx, offer_lanes, lane_limit, scan_idx)
    }

    pub(crate) fn try_recv_from_binding(
        &mut self,
        logical_lane: u8,
        expected_frame_label: u8,
        scratch_ptr: *mut [u8],
    ) -> RecvResult<Option<Payload<'r>>> {
        let lane_idx = logical_lane as usize;
        if let Some(evidence) = self.take_matching_binding_for_lane(lane_idx, expected_frame_label)
        {
            if let Some(payload) = self.take_restored_binding_payload(lane_idx, evidence) {
                return Ok(Some(payload));
            }
            let payload = unsafe {
                // SAFETY: binding and scratch storage are owned by this endpoint
                // lane port for the session lifetime; returned payload borrows
                // only from that resident storage.
                lane_port::recv_from_binding(
                    core::ptr::from_mut(&mut self.binding),
                    evidence.channel,
                    scratch_ptr,
                )
            }
            .map_err(RecvError::Binding)?;
            return Ok(Some(payload));
        }
        Ok(None)
    }
}
