//! Receive-path helpers for deterministic recv.

use core::task::Poll;

use super::{
    core::{
        CommitDelta, CursorEndpoint, PreparedCommitDelta, RecvRuntimeDesc,
        prepare_event_selected_route_commit_rows_from_resident_route_commit_range,
    },
    lane_port,
};
use crate::{
    endpoint::{RecvError, RecvResult, kernel::RecvPayloadMode},
    global::typestate::{CursorInvariantError, StateIndex, state_index_to_usize},
    observe::ids,
    resolver_audit::ResolverSlot,
    transport::{
        Transport,
        trace::TapFrameMeta,
        wire::{CodecError, FrameFlags, Payload},
    },
};

pub(crate) enum RecvPayloadSource<'a> {
    ZeroLength,
    Direct(Payload<'a>),
}

struct RecvCommitPlan<'a> {
    desc: RecvDescriptor,
    payload: Payload<'a>,
    delta: PreparedCommitDelta,
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

impl<'r, const ROLE: u8, T, const MAX_RV: usize> CursorEndpoint<'r, ROLE, T, MAX_RV>
where
    T: Transport + 'r,
{
    fn prepare_recv_descriptor(
        &mut self,
        target_label: u8,
        payload_mode: RecvPayloadMode,
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
        if let Some(arm) = meta.route_arm
            && let Some(selected) = self.selected_arm_for_scope(meta.scope)
            && selected != arm
        {
            return Err(RecvError::PhaseInvariant);
        }
        if self
            .cursor
            .event_enabled(
                idx,
                crate::global::typestate::EventCommitMeta::from(meta),
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
            payload_mode,
        );
        if meta.origin.is_session() {
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
        payload_mode: RecvPayloadMode,
        pending_recv: &mut lane_port::PendingRecv,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RecvPayloadSource<'r>>> {
        let frame = match self.poll_accepted_transport_frame(
            pending_recv,
            desc.lane_idx,
            lane_port::FrameExpectation {
                session_raw: desc.sid_raw,
                lane_wire: desc.lane_wire,
                source_role: desc.meta.peer,
                target_role: ROLE,
                label: desc.meta.frame_label,
            },
            cx,
        ) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(frame)) => frame,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };

        if frame.is_empty() {
            if payload_mode.allows_zero_length() {
                if !frame.into_payload().as_bytes().is_empty() {
                    crate::invariant();
                }
                return Poll::Ready(Ok(RecvPayloadSource::ZeroLength));
            }
            frame.discard_uncommitted();
            return Poll::Ready(Ok(RecvPayloadSource::ZeroLength));
        }

        Poll::Ready(Ok(RecvPayloadSource::Direct(frame.into_payload())))
    }

    fn build_recv_commit_plan(
        &mut self,
        desc: RecvDescriptor,
        payload_source: RecvPayloadSource<'r>,
        erased: RecvRuntimeDesc,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<RecvCommitPlan<'r>> {
        let meta = desc.meta;
        if erased.frame_label() != crate::transport::FrameLabel::new(meta.frame_label) {
            return Err(RecvError::PhaseInvariant);
        }
        if meta.origin.is_session() {
            return Err(RecvError::PhaseInvariant);
        }
        let payload = match payload_source {
            RecvPayloadSource::ZeroLength if erased.payload_mode().allows_zero_length() => {
                Payload::new(&[])
            }
            RecvPayloadSource::ZeroLength => return Err(RecvError::PhaseInvariant),
            RecvPayloadSource::Direct(payload) => payload,
        };
        if let Err(err) = validate(payload) {
            return Err(RecvError::Codec(err));
        }
        let enabled = match self.cursor.event_enabled(
            state_index_to_usize(desc.cursor_index),
            crate::global::typestate::EventCommitMeta::from(meta),
            |scope| self.selected_arm_for_scope(scope),
        ) {
            Ok(enabled) => enabled,
            Err(CursorInvariantError::INVARIANT) => {
                return Err(RecvError::PhaseInvariant);
            }
        };
        let route_rows = if let Some(arm) = meta.route_arm {
            let route_rows = {
                let Self {
                    cursor,
                    decision_state,
                    route_commit_rows,
                    ..
                } = &mut *self;
                let mut rows = route_commit_rows.begin();
                prepare_event_selected_route_commit_rows_from_resident_route_commit_range(
                    decision_state,
                    cursor,
                    meta.lane,
                    state_index_to_usize(desc.cursor_index),
                    arm,
                    &mut rows,
                )?;
                rows.as_commit_rows(meta.lane)
            };
            if route_rows.is_empty() {
                return Err(RecvError::PhaseInvariant);
            }
            route_rows
        } else {
            super::SelectedRouteCommitRowsRef::EMPTY
        };
        let delta = CommitDelta::from_recv_meta(
            meta,
            route_rows,
            enabled.cursor_after(),
            enabled.progress_step(),
        );
        let delta = match self.prepare_commit_delta(delta) {
            Ok(delta) => delta,
            Err(CursorInvariantError::INVARIANT) => {
                return Err(RecvError::PhaseInvariant);
            }
        };

        Ok(RecvCommitPlan {
            desc,
            payload,
            delta,
        })
    }

    fn publish_recv_commit_plan(&mut self, plan: RecvCommitPlan<'r>) -> Payload<'r> {
        let RecvCommitPlan {
            desc,
            payload,
            delta,
        } = plan;
        let meta = desc.meta;

        self.emit_endpoint_resolver_audit(
            ResolverSlot::EndpointRx,
            ids::ENDPOINT_RECV,
            desc.sid_raw,
            Self::endpoint_resolver_args(
                crate::session::types::Lane::new(meta.lane as u32),
                meta.label,
                FrameFlags::empty(),
            ),
            crate::session::types::Lane::new(meta.lane as u32),
        );

        let logical_meta = TapFrameMeta::new(
            desc.sid_raw,
            desc.lane_wire,
            ROLE,
            meta.label,
            FrameFlags::empty(),
        );
        let event_id = if meta.origin.is_session() {
            ids::ENDPOINT_SESSION
        } else {
            ids::ENDPOINT_RECV
        };
        self.emit_endpoint_event(event_id, logical_meta, meta.lane);

        self.commit_prepared_delta(delta);
        payload
    }

    fn finish_recv_payload(
        &mut self,
        desc: RecvDescriptor,
        payload_source: RecvPayloadSource<'r>,
        erased: RecvRuntimeDesc,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>> {
        let plan = self.build_recv_commit_plan(desc, payload_source, erased, validate)?;
        Ok(self.publish_recv_commit_plan(plan))
    }
}

impl<'r, const ROLE: u8, T, const MAX_RV: usize> super::core::RecvKernelEndpoint<'r>
    for CursorEndpoint<'r, ROLE, T, MAX_RV>
where
    T: Transport + 'r,
{
    #[inline]
    fn prepare_recv_kernel_descriptor(
        &mut self,
        label: u8,
        payload_mode: RecvPayloadMode,
    ) -> RecvResult<PreparedRecv> {
        self.prepare_recv_descriptor(label, payload_mode)
    }

    #[inline]
    fn poll_recv_kernel_payload_source(
        &mut self,
        desc: RecvDescriptor,
        payload_mode: RecvPayloadMode,
        state: &mut RecvState,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<RecvResult<RecvPayloadSource<'r>>> {
        let pending_recv = &mut state.pending_recv;
        self.poll_recv_payload_source(desc, payload_mode, pending_recv, cx)
    }

    #[inline]
    fn finish_recv_kernel_payload(
        &mut self,
        desc: RecvDescriptor,
        payload_source: RecvPayloadSource<'r>,
        erased: RecvRuntimeDesc,
        validate: for<'a> fn(Payload<'a>) -> Result<(), CodecError>,
    ) -> RecvResult<Payload<'r>> {
        self.finish_recv_payload(desc, payload_source, erased, validate)
    }
}
