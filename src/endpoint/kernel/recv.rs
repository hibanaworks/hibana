//! Receive-path helpers for deterministic recv.

use super::{core::CursorEndpoint, lane_port};
use crate::{
    binding::BindingSlot,
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    epf::vm::Slot,
    global::MessageSpec,
    global::const_dsl::ScopeKind,
    global::typestate::{JumpReason, PassiveArmNavigation, state_index_to_usize},
    observe::ids,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{Transport, trace::TapFrameMeta, wire::FrameFlags, wire::WireDecodeOwned},
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    /// Receive a payload of type `M` according to the current typestate step.
    pub async fn recv<M>(mut self) -> RecvResult<(Self, <M as MessageSpec>::Payload)>
    where
        M: MessageSpec,
        M::Payload: WireDecodeOwned,
    {
        let target_label = <M as MessageSpec>::LABEL;
        self.try_select_lane_for_label(target_label);

        let mut _iter_count = 0u32;
        loop {
            _iter_count += 1;
            debug_assert!(
                _iter_count <= 3,
                "recv() infinite loop detected at iter={}",
                _iter_count
            );
            if _iter_count > 3 {
                return Err(RecvError::PhaseInvariant);
            }

            if let Some(reason) = self.cursor.jump_reason() {
                if matches!(reason, JumpReason::LoopContinue) {
                    if let Some(region) = self.cursor.scope_region() {
                        if region.kind == ScopeKind::Route && region.linger {
                            let scope_id = region.scope_id;
                            let route_signals = self.policy_signals_for_slot(Slot::Route);
                            if let Ok(step) =
                                self.prepare_route_decision_from_resolver(scope_id, route_signals)
                            {
                                match step {
                                    super::authority::RouteResolveStep::Resolved(arm) => {
                                        if arm.as_u8() == 0 {
                                            self.set_cursor(self.cursor.advance());
                                        } else if let Some(nav) =
                                            self.cursor.follow_passive_observer_arm(arm.as_u8())
                                        {
                                            let PassiveArmNavigation::WithinArm { entry } = nav;
                                            self.set_cursor(
                                                self.cursor.with_index(state_index_to_usize(entry)),
                                            );
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
                    }
                }
            }

            if let Some(region) = self.cursor.scope_region() {
                if region.kind == ScopeKind::Route && self.cursor.index() == region.start {
                    let scope_id = region.scope_id;
                    let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
                    let existing_arm = self.route_arm_for(lane_wire, scope_id);
                    if let Some(arm) = existing_arm {
                        let recv_idx = self.cursor.route_scope_arm_recv_index(scope_id, arm);
                        if let Some(idx) = recv_idx {
                            self.set_cursor(self.cursor.with_index(idx));
                            self.set_route_arm(lane_wire, scope_id, arm)?;
                            continue;
                        }
                        if let Some(nav) = self.cursor.follow_passive_observer_arm(arm) {
                            let PassiveArmNavigation::WithinArm { entry } = nav;
                            self.set_cursor(self.cursor.with_index(state_index_to_usize(entry)));
                            self.set_route_arm(lane_wire, scope_id, arm)?;
                            continue;
                        }
                        if let Some(cursor) = self.cursor.advance_scope_if_kind(ScopeKind::Route) {
                            self.set_cursor(cursor);
                            continue;
                        }
                    } else {
                        return Err(RecvError::PhaseInvariant);
                    }
                }
            }

            if self.cursor.is_recv() {
                break;
            }

            if let Some(region) = self.cursor.scope_region() {
                if region.kind == ScopeKind::Route
                    && self.can_advance_route_scope(region.scope_id, target_label)
                    && let Some(cursor) = self.cursor.advance_scope_if_kind(ScopeKind::Route)
                {
                    self.set_cursor(cursor);
                    continue;
                }
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

        let sid_raw = self.sid.raw();
        let lane_wire = self.port_for_lane(meta.lane as usize).lane().as_wire();
        let mut binding_buf: [u8; 65536] = [0; 65536];
        let logical_lane = meta.lane;

        let binding_data =
            self.try_recv_from_binding(logical_lane, meta.label, &mut binding_buf)?;

        let payload_bytes: &[u8] = if let Some(n) = binding_data {
            &binding_buf[..n]
        } else {
            'recv_loop: loop {
                let transport_payload_len = {
                    let port = self.port_for_lane(meta.lane as usize);
                    let payload = lane_port::recv_future(port)
                        .await
                        .map_err(RecvError::Transport)?;
                    lane_port::copy_payload_into_scratch(port, &payload)
                        .map_err(|_| RecvError::PhaseInvariant)?
                };

                if let Some(n) =
                    self.try_recv_from_binding(logical_lane, meta.label, &mut binding_buf)?
                {
                    break 'recv_loop &binding_buf[..n];
                }

                if transport_payload_len == 0 {
                    let binding_active = self.binding.policy_signals_provider().is_some();
                    if !binding_active || M::Payload::decode_owned(&[]).is_ok() {
                        break 'recv_loop &[];
                    }
                    continue;
                }

                let port = self.port_for_lane(meta.lane as usize);
                break 'recv_loop &lane_port::scratch(port)[..transport_payload_len];
            }
        };

        let policy_action = self.eval_endpoint_policy(
            Slot::EndpointRx,
            ids::ENDPOINT_RECV,
            sid_raw,
            Self::endpoint_policy_args(
                crate::control::types::Lane::new(meta.lane as u32),
                meta.label,
                FrameFlags::empty(),
            ),
            crate::control::types::Lane::new(meta.lane as u32),
        );
        self.apply_recv_policy(
            policy_action,
            meta.scope,
            crate::control::types::Lane::new(meta.lane as u32),
        )?;

        let logical_meta =
            TapFrameMeta::new(sid_raw, lane_wire, ROLE, meta.label, FrameFlags::empty());
        let payload = M::Payload::decode_owned(payload_bytes).map_err(RecvError::Codec)?;

        let scope_trace = self.scope_trace(meta.scope);
        let event_id = if meta.is_control {
            ids::ENDPOINT_CONTROL
        } else {
            ids::ENDPOINT_RECV
        };
        self.emit_endpoint_event(event_id, logical_meta, scope_trace, meta.lane);

        self.set_cursor(
            self.cursor
                .try_advance_past_jumps()
                .map_err(|_| RecvError::PhaseInvariant)?,
        );

        let recv_lane_idx = meta.lane as usize;
        self.advance_lane_cursor(recv_lane_idx, meta.eff_index);
        self.maybe_skip_remaining_route_arm(meta.scope, meta.lane, meta.route_arm, meta.eff_index);
        self.settle_scope_after_action(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        self.maybe_advance_phase();
        Ok((self, payload))
    }
}
