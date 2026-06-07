//! Receive-path helpers for deterministic recv.

use core::task::Poll;

use super::{
    core::{
        CommitDelta, CursorEndpoint, LoopCommitRow, PreparedCommitDelta, RecvRuntimeDesc,
        prepare_event_selected_route_commit_row_from_parts,
    },
    lane_port,
};
use crate::{
    control::cap::mint::{EpochTable, MintConfigMarker},
    endpoint::{RecvError, RecvResult},
    global::{
        ControlDesc,
        typestate::{LoopMetadata, LoopRole, RecvMeta, StateIndex, state_index_to_usize},
    },
    observe::ids,
    policy_runtime::PolicySlot,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{
        Transport,
        trace::TapFrameMeta,
        wire::{CodecError, FrameFlags, Payload},
    },
};

pub(crate) enum RecvPayloadSource<'a> {
    Empty,
    Direct(Payload<'a>),
}

impl RecvPayloadSource<'_> {
    #[inline]
    fn discard_terminal(self) {}
}

enum RecvCommitEffect {
    None,
}

impl RecvCommitEffect {
    #[inline]
    fn discard_uncommitted(self) {}
}

struct RecvCommitPlan<'a> {
    desc: RecvDescriptor,
    payload: Payload<'a>,
    delta: PreparedCommitDelta,
    commit_effect: RecvCommitEffect,
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
    pub(crate) cursor_index: StateIndex,
    pub(crate) sid_raw: u32,
    pub(crate) lane_idx: usize,
    pub(crate) lane_wire: u8,
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    fn recv_loop_ack_row(&self, meta: RecvMeta) -> RecvResult<LoopCommitRow> {
        if !meta.semantic.is_loop() {
            return Ok(LoopCommitRow::EMPTY);
        }
        let Some(LoopMetadata {
            scope: scope_id,
            controller,
            target,
            role,
            ..
        }) = self.cursor.loop_metadata_inner()
        else {
            return Ok(LoopCommitRow::EMPTY);
        };
        if role != LoopRole::Target || target != ROLE {
            return Err(RecvError::PhaseInvariant);
        }
        if meta.peer != controller {
            return Err(RecvError::PeerMismatch {
                expected: controller,
                actual: meta.peer,
            });
        }
        let lane_idx = meta.lane as usize;
        if lane_idx >= self.cursor.logical_lane_count() {
            return Err(RecvError::PhaseInvariant);
        }
        let idx = Self::loop_index(scope_id).ok_or(RecvError::PhaseInvariant)?;
        let port = self.port_for_lane(lane_idx);
        let lane = port.lane();
        Ok(LoopCommitRow::ack(
            scope_id,
            idx,
            meta.lane,
            ROLE,
            port.loop_table().has_decision(lane, idx),
        ))
    }

    fn prepare_recv_descriptor(
        &mut self,
        target_label: u8,
        expects_control: bool,
        accepts_empty_payload: bool,
    ) -> RecvResult<PreparedRecv> {
        let idx = self
            .cursor
            .recv_descriptor_index_for_label(target_label, |scope| {
                self.selected_arm_for_scope(scope)
            })
            .ok_or(RecvError::PhaseInvariant)?;

        let meta = self
            .cursor
            .try_recv_meta_at(idx)
            .ok_or(RecvError::PhaseInvariant)?;
        let cursor_index = StateIndex::from_usize(idx);
        if meta.label != target_label {
            return Err(RecvError::LabelMismatch {
                expected: meta.label,
                actual: target_label,
            });
        }
        if let Some(arm) = meta.route_arm {
            if let Some(selected) = self.selected_arm_for_scope(meta.scope)
                && selected != arm
            {
                return Err(RecvError::PhaseInvariant);
            }
        }
        if self
            .cursor
            .event_enabled(
                idx,
                meta.eff_index,
                meta.label,
                meta.is_control,
                meta.scope,
                meta.route_arm,
                meta.lane,
                |scope| self.selected_arm_for_scope(scope),
            )
            .is_err()
        {
            return Err(RecvError::PhaseInvariant);
        }

        let lane_idx = meta.lane as usize;
        let lane_wire = self.port_for_lane(lane_idx).lane().as_wire();
        let descriptor = RecvDescriptor {
            meta,
            cursor_index,
            sid_raw: self.sid.raw(),
            lane_idx,
            lane_wire,
        };
        let runtime = RecvRuntimeDesc::new(
            target_label,
            crate::transport::FrameLabel::new(meta.frame_label),
            expects_control,
            accepts_empty_payload,
        );
        if meta.is_control != runtime.expects_control() {
            return Err(RecvError::PhaseInvariant);
        }
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
        loop {
            let frame = match self.poll_accepted_transport_frame(
                pending_recv,
                desc.lane_idx,
                desc.sid_raw,
                desc.lane_wire,
                desc.meta.peer,
                ROLE,
                desc.meta.frame_label,
                cx,
            ) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(frame)) => frame,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            };

            if frame.is_empty() {
                if accepts_empty_payload {
                    let _ = frame.into_payload();
                    return Poll::Ready(Ok(RecvPayloadSource::Empty));
                }
                frame.discard_uncommitted();
                return Poll::Ready(Ok(RecvPayloadSource::Empty));
            }

            return Poll::Ready(Ok(RecvPayloadSource::Direct(frame.into_payload())));
        }
    }

    fn build_recv_commit_plan(
        &self,
        desc: RecvDescriptor,
        payload_source: RecvPayloadSource<'r>,
        erased: RecvRuntimeDesc,
        control: Option<ControlDesc>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<RecvCommitPlan<'r>> {
        let meta = desc.meta;
        if erased.frame_label() != crate::transport::FrameLabel::new(meta.frame_label) {
            payload_source.discard_terminal();
            return Err(RecvError::PhaseInvariant);
        }
        if meta.is_control != erased.expects_control() {
            payload_source.discard_terminal();
            return Err(RecvError::PhaseInvariant);
        }
        let (payload, commit_effect) = match payload_source {
            RecvPayloadSource::Empty if erased.accepts_empty_payload() => {
                (Payload::new(&[]), RecvCommitEffect::None)
            }
            RecvPayloadSource::Empty => return Err(RecvError::PhaseInvariant),
            RecvPayloadSource::Direct(payload) => (payload, RecvCommitEffect::None),
        };
        if let Err(err) = validate(payload) {
            commit_effect.discard_uncommitted();
            return Err(RecvError::Codec(err));
        }
        if let Err(err) = self.validate_inbound_explicit_wire_control(desc, control, payload) {
            commit_effect.discard_uncommitted();
            return Err(err);
        }
        let enabled = match self.cursor.event_enabled(
            state_index_to_usize(desc.cursor_index),
            meta.eff_index,
            meta.label,
            meta.is_control,
            meta.scope,
            meta.route_arm,
            meta.lane,
            |scope| self.selected_arm_for_scope(scope),
        ) {
            Ok(enabled) => enabled,
            Err(_) => {
                commit_effect.discard_uncommitted();
                return Err(RecvError::PhaseInvariant);
            }
        };
        let mut delta =
            CommitDelta::from_recv_meta(meta, enabled.cursor_after(), enabled.progress_step());
        if let Some(arm) = meta.route_arm {
            let Some(row) = prepare_event_selected_route_commit_row_from_parts(
                &self.decision_state,
                &self.cursor,
                meta.lane,
                meta.scope,
                arm,
            ) else {
                commit_effect.discard_uncommitted();
                return Err(RecvError::PhaseInvariant);
            };
            delta = delta.with_selected_route(row);
        }
        delta = delta.with_loop_row(match self.recv_loop_ack_row(meta) {
            Ok(row) => row,
            Err(err) => {
                commit_effect.discard_uncommitted();
                return Err(err);
            }
        });
        let delta = match self.prepare_commit_delta(delta) {
            Ok(delta) => delta,
            Err(_) => {
                commit_effect.discard_uncommitted();
                return Err(RecvError::PhaseInvariant);
            }
        };

        Ok(RecvCommitPlan {
            desc,
            payload,
            delta,
            commit_effect,
        })
    }

    fn publish_recv_commit_plan(&mut self, plan: RecvCommitPlan<'r>) -> RecvResult<Payload<'r>> {
        let RecvCommitPlan {
            desc,
            payload,
            delta,
            commit_effect,
        } = plan;
        let meta = desc.meta;

        let _ = commit_effect;

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

        self.commit_prepared_delta(delta);
        Ok(payload)
    }

    fn finish_recv_payload(
        &mut self,
        desc: RecvDescriptor,
        payload_source: RecvPayloadSource<'r>,
        erased: RecvRuntimeDesc,
        control: Option<ControlDesc>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>> {
        let plan = self.build_recv_commit_plan(desc, payload_source, erased, control, validate)?;
        let payload = self.publish_recv_commit_plan(plan)?;
        Ok(payload)
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint> super::core::RecvKernelEndpoint<'r>
    for CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    #[inline]
    fn prepare_recv_kernel_descriptor(
        &mut self,
        label: u8,
        expects_control: bool,
        accepts_empty_payload: bool,
    ) -> RecvResult<PreparedRecv> {
        self.prepare_recv_descriptor(label, expects_control, accepts_empty_payload)
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
        control: Option<ControlDesc>,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>> {
        self.finish_recv_payload(desc, payload_source, erased, control, validate)
    }
}
