//! Receive-path helpers for deterministic recv.

use core::task::Poll;

use super::{core::CursorEndpoint, lane_port};
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

enum RecvPayloadSource<'a> {
    Empty,
    Borrowed(Payload<'a>),
}

#[derive(Clone, Copy)]
pub(crate) struct RecvDesc {
    target_label: u8,
    accepts_empty_payload: bool,
}

impl RecvDesc {
    #[inline]
    pub(crate) const fn new(target_label: u8, accepts_empty_payload: bool) -> Self {
        Self {
            target_label,
            accepts_empty_payload,
        }
    }
}

pub(crate) struct RecvState {
    desc: Option<RecvDescriptor>,
    pending_recv: lane_port::PendingRecv,
}

impl RecvState {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            desc: None,
            pending_recv: lane_port::PendingRecv::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct RecvDescriptor {
    target_label: u8,
    meta: crate::global::typestate::RecvMeta,
    sid_raw: u32,
    lane_idx: usize,
    lane_wire: u8,
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
    pub(crate) fn poll_recv_state<'a>(
        &mut self,
        erased: RecvDesc,
        state: &mut RecvState,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<Payload<'a>>> {
        let descriptor = match state.desc {
            Some(descriptor) => descriptor,
            None => {
                let descriptor = match self.prepare_recv_descriptor(erased.target_label) {
                    Ok(descriptor) => descriptor,
                    Err(err) => return Poll::Ready(Err(err)),
                };
                state.desc = Some(descriptor);
                descriptor
            }
        };
        match self.poll_recv_payload_source(
            descriptor,
            erased.accepts_empty_payload,
            &mut state.pending_recv,
            cx,
        ) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(payload_source)) => {
                state.desc = None;
                Poll::Ready(
                    self.finish_recv_payload(descriptor, payload_source, erased)
                        .map(lane_port::shrink_payload),
                )
            }
            Poll::Ready(Err(err)) => {
                state.desc = None;
                Poll::Ready(Err(err))
            }
        }
    }

    fn prepare_recv_descriptor(&mut self, target_label: u8) -> RecvResult<RecvDescriptor> {
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
                        self.set_route_arm(lane_wire, scope_id, arm)?;
                        continue;
                    }
                    if let Some(nav) = self.cursor.follow_passive_observer_arm(arm) {
                        let PassiveArmNavigation::WithinArm { entry } = nav;
                        self.set_cursor_index(state_index_to_usize(entry));
                        self.set_route_arm(lane_wire, scope_id, arm)?;
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
        Ok(RecvDescriptor {
            target_label,
            meta,
            sid_raw: self.sid.raw(),
            lane_idx,
            lane_wire,
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
            self.try_recv_from_binding(desc.meta.lane, desc.target_label, scratch_ptr)
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
                self.try_recv_from_binding(desc.meta.lane, desc.target_label, scratch_ptr)
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
        erased: RecvDesc,
    ) -> RecvResult<Payload<'r>> {
        let meta = desc.meta;
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
        self.settle_scope_after_action(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        self.maybe_advance_phase();
        match payload_source {
            RecvPayloadSource::Empty if erased.accepts_empty_payload => Ok(Payload::new(&[])),
            RecvPayloadSource::Empty => Err(RecvError::PhaseInvariant),
            RecvPayloadSource::Borrowed(payload) => Ok(payload),
        }
    }
}
