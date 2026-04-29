//! Receive-path helpers for deterministic recv.

use core::task::Poll;

use super::{
    core::{CursorEndpoint, RecvRuntimeDesc},
    lane_port,
};
use crate::{
    binding::BindingSlot,
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    global::const_dsl::ScopeKind,
    global::typestate::{JumpReason, PassiveArmNavigation, state_index_to_usize},
    observe::ids,
    policy_runtime::PolicySlot,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{
        Transport,
        trace::TapFrameMeta,
        wire::{FrameFlags, Payload},
    },
};

pub(crate) enum RecvPayloadSource<'a> {
    Empty,
    Borrowed(Payload<'a>),
}

pub(crate) struct RecvState {
    prepared: Option<PreparedRecv>,
    pending_recv: lane_port::PendingRecv,
}

impl RecvState {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            prepared: None,
            pending_recv: lane_port::PendingRecv::new(),
        }
    }

    #[inline]
    pub(crate) fn prepared(&self) -> Option<PreparedRecv> {
        self.prepared
    }

    #[inline]
    pub(crate) fn set_prepared(&mut self, prepared: PreparedRecv) {
        self.prepared = Some(prepared);
    }

    #[inline]
    pub(crate) fn clear_prepared(&mut self) {
        self.prepared = None;
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PreparedRecv {
    pub(crate) descriptor: RecvDescriptor,
    pub(crate) runtime: RecvRuntimeDesc,
}

#[derive(Clone, Copy)]
pub(crate) struct RecvDescriptor {
    pub(crate) meta: crate::global::typestate::RecvMeta,
    pub(crate) sid_raw: u32,
    pub(crate) lane_idx: usize,
    pub(crate) lane_wire: u8,
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[cfg(test)]
    #[expect(
        dead_code,
        reason = "H4 recv delegate proves loop ownership stays in kernel_recv"
    )]
    pub(crate) fn poll_recv_state(
        &mut self,
        logical_label: u8,
        accepts_empty_payload: bool,
        state: &mut RecvState,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'r>>> {
        super::core::kernel_recv(self, logical_label, accepts_empty_payload, state, cx)
    }

    fn prepare_recv_descriptor(
        &mut self,
        target_label: u8,
        accepts_empty_payload: bool,
    ) -> RecvResult<PreparedRecv> {
        self.try_select_lane_for_label(target_label);

        let mut iter_count = 0u32;
        loop {
            iter_count += 1;
            debug_assert!(
                iter_count <= 3,
                "recv() infinite loop detected at iter={}",
                iter_count
            );
            if iter_count > 3 {
                return Err(RecvError::PhaseInvariant);
            }

            if let Some(reason) = self.cursor.jump_reason()
                && matches!(reason, JumpReason::LoopContinue)
                && let Some(region) = self.cursor.scope_region()
                && region.kind == ScopeKind::Route
                && region.linger
            {
                let scope_id = region.scope_id;
                let route_signals = self.policy_signals_for_slot(PolicySlot::Route).into_owned();
                if let Ok(step) =
                    self.prepare_route_decision_from_resolver(scope_id, &route_signals)
                {
                    match step {
                        super::authority::RouteResolveStep::Resolved(arm) => {
                            if arm.as_u8() == 0 {
                                self.cursor.advance_in_place();
                            } else if let Some(nav) =
                                self.cursor.follow_passive_observer_arm(arm.as_u8())
                            {
                                let PassiveArmNavigation::WithinArm { entry } = nav;
                                self.set_cursor_index(state_index_to_usize(entry));
                            }
                            continue;
                        }
                        super::authority::RouteResolveStep::Abort(reason) => {
                            return Err(RecvError::PolicyAbort { reason });
                        }
                        super::authority::RouteResolveStep::Deferred { .. } => {}
                    }
                }
            }

            if let Some(region) = self.cursor.scope_region()
                && region.kind == ScopeKind::Route
                && self.cursor.index() == region.start
            {
                let scope_id = region.scope_id;
                let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
                let existing_arm = self.route_arm_for(lane_wire, scope_id);
                if let Some(arm) = existing_arm {
                    let recv_idx = self.cursor.route_scope_arm_recv_index(scope_id, arm);
                    if let Some(idx) = recv_idx {
                        self.set_cursor_index(idx);
                        continue;
                    }
                    if let Some(nav) = self.cursor.follow_passive_observer_arm(arm) {
                        let PassiveArmNavigation::WithinArm { entry } = nav;
                        self.set_cursor_index(state_index_to_usize(entry));
                        continue;
                    }
                    if self.cursor.advance_scope_if_kind_in_place(ScopeKind::Route) {
                        continue;
                    }
                } else {
                    return Err(RecvError::PhaseInvariant);
                }
            }

            if self.cursor.is_recv() {
                break;
            }

            if let Some(region) = self.cursor.scope_region()
                && region.kind == ScopeKind::Route
                && self.can_advance_route_scope(region.scope_id, target_label)
                && self.cursor.advance_scope_if_kind_in_place(ScopeKind::Route)
            {
                continue;
            }
            return Err(RecvError::PhaseInvariant);
        }

        let meta = self
            .cursor
            .try_recv_meta()
            .ok_or(RecvError::PhaseInvariant)?;
        if meta.label != target_label {
            return Err(RecvError::LabelMismatch {
                expected: meta.label,
                actual: target_label,
            });
        }

        let lane_idx = meta.lane as usize;
        let lane_wire = self.port_for_lane(lane_idx).lane().as_wire();
        let descriptor = RecvDescriptor {
            meta,
            sid_raw: self.sid.raw(),
            lane_idx,
            lane_wire,
        };
        let runtime = RecvRuntimeDesc::new(
            target_label,
            crate::transport::FrameLabel::new(meta.frame_label),
            accepts_empty_payload,
        );
        Ok(PreparedRecv {
            descriptor,
            runtime,
        })
    }

    fn poll_recv_payload_source(
        &mut self,
        desc: RecvDescriptor,
        accepts_empty_payload: bool,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RecvPayloadSource<'r>>> {
        if let Some(payload) = {
            let scratch_ptr = {
                let port = self.port_for_lane(desc.lane_idx);
                lane_port::scratch_ptr(port)
            };
            self.try_recv_from_binding(desc.meta.lane, desc.meta.frame_label, scratch_ptr)
        }? {
            return Poll::Ready(Ok(RecvPayloadSource::Borrowed(payload)));
        }

        loop {
            let payload = {
                let port = self.port_for_lane(desc.lane_idx);
                match lane_port::poll_recv(pending_recv, port, cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(payload)) => payload,
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(RecvError::Transport(err))),
                }
            };

            if let Some(payload) = {
                let scratch_ptr = {
                    let port = self.port_for_lane(desc.lane_idx);
                    lane_port::scratch_ptr(port)
                };
                self.try_recv_from_binding(desc.meta.lane, desc.meta.frame_label, scratch_ptr)
            }? {
                return Poll::Ready(Ok(RecvPayloadSource::Borrowed(payload)));
            }

            if payload.as_bytes().is_empty() {
                let binding_active = self.binding.policy_signals_provider().is_some();
                if !binding_active || accepts_empty_payload {
                    return Poll::Ready(Ok(RecvPayloadSource::Empty));
                }
                continue;
            }

            return Poll::Ready(Ok(RecvPayloadSource::Borrowed(payload)));
        }
    }

    fn finish_recv_payload(
        &mut self,
        desc: RecvDescriptor,
        payload_source: RecvPayloadSource<'r>,
        erased: RecvRuntimeDesc,
    ) -> RecvResult<Payload<'r>> {
        let meta = desc.meta;
        if erased.frame_label() != crate::transport::FrameLabel::new(meta.frame_label) {
            return Err(RecvError::PhaseInvariant);
        }
        self.emit_endpoint_policy_audit(
            PolicySlot::EndpointRx,
            ids::ENDPOINT_RECV,
            desc.sid_raw,
            Self::endpoint_policy_args(
                crate::control::types::Lane::new(meta.lane as u32),
                meta.label,
                FrameFlags::empty(),
            ),
            crate::control::types::Lane::new(meta.lane as u32),
        );

        let logical_meta = TapFrameMeta::new(
            desc.sid_raw,
            desc.lane_wire,
            ROLE,
            meta.label,
            FrameFlags::empty(),
        );
        let scope_trace = self.scope_trace(meta.scope);
        let event_id = if meta.is_control {
            ids::ENDPOINT_CONTROL
        } else {
            ids::ENDPOINT_RECV
        };
        self.emit_endpoint_event(event_id, logical_meta, scope_trace, meta.lane);

        self.cursor
            .try_advance_past_jumps_in_place()
            .map_err(|_| RecvError::PhaseInvariant)?;

        self.advance_lane_cursor(desc.lane_idx, meta.eff_index);
        self.maybe_skip_remaining_route_arm(meta.scope, meta.lane, meta.route_arm, meta.eff_index);
        self.publish_scope_settlement(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        self.maybe_advance_phase();
        match payload_source {
            RecvPayloadSource::Empty if erased.accepts_empty_payload() => Ok(Payload::new(&[])),
            RecvPayloadSource::Empty => Err(RecvError::PhaseInvariant),
            RecvPayloadSource::Borrowed(payload) => Ok(payload),
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    super::core::RecvKernelEndpoint<'r> for CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot + 'r,
{
    #[inline]
    fn prepare_recv_kernel_descriptor(
        &mut self,
        label: u8,
        accepts_empty_payload: bool,
    ) -> RecvResult<PreparedRecv> {
        self.prepare_recv_descriptor(label, accepts_empty_payload)
    }

    #[inline]
    fn poll_recv_kernel_payload_source(
        &mut self,
        desc: RecvDescriptor,
        accepts_empty_payload: bool,
        state: &mut RecvState,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RecvPayloadSource<'r>>> {
        let pending_recv = &mut state.pending_recv;
        self.poll_recv_payload_source(desc, accepts_empty_payload, pending_recv, cx)
    }

    #[inline]
    fn finish_recv_kernel_payload(
        &mut self,
        desc: RecvDescriptor,
        payload_source: RecvPayloadSource<'r>,
        erased: RecvRuntimeDesc,
    ) -> RecvResult<Payload<'r>> {
        self.finish_recv_payload(desc, payload_source, erased)
    }
}
