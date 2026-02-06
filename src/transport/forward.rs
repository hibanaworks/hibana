//! Forwarding operations (relay/splice).
//!
//! Relay forwards frames verbatim while splice performs the two-step hand-off
//! against the rendezvous (begin → commit). Control frames (cancel,
//! checkpoint/rollback) are bridged back into the rendezvous so that the data
//! plane stays affine.
//!
//! # Implementation-Level Optimization (not MPST Type-Level)
//!
//! **Forward is a Port/Transport layer optimization**, distinct from MPST type theory:
//!
//! - **Type level (MPST theory)**: "Intermediary elimination" is expressed via
//!   **Delegation/Linking** using `CapToken` (see [`crate::endpoint::delegate`]). The global
//!   protocol explicitly includes Proxy, then delegates session ownership to
//!   enable direct Client↔Server communication (cut elimination).
//!
//! - **Implementation level (this module)**: `relay` and `splice` are **runtime
//!   optimizations** that forward frames between ports:
//!   - `relay()`: Safe but involves frame copying
//!   - `splice()`: Zero-copy direct connection (fence → gen++ → release)
//!   - **Observationally equivalent**: `relay ≡ splice` (verified via tap
//!     normalization in `examples/forward_lowlevel_test.rs`)
//!
//! # Why Forward is NOT a Global Combinator
//!
//! Standard MPST theory (including AMPST for affine/cancellation safety) does not
//! require "forwarding" as a primitive global type operation. The type-level concept
//! of "intermediary disappearing" is adequately expressed through:
//!
//! 1. **Global type**: Client → Proxy → Server (Proxy is explicit mediator)
//! 2. **Projection**: Each role gets local session type
//! 3. **Delegation**: Proxy sends `Msg<LABEL, CapToken>` to enable direct linking
//!
//! `Forward` is an **implementation strategy** that preserves MPST safety properties
//! (deadlock freedom, progress, cancellation termination) while optimizing runtime
//! performance. It corresponds to the "cut elimination" intuition but is best stated
//! as **implementation equivalence** (observational/trace equivalence) rather than
//! a type-theoretic transformation.
//!
//! See:
//! - `examples/proxy_delegation.rs` - Type-level delegation (CapToken-based)
//! - `examples/forward_lowlevel_test.rs` - Implementation-level equivalence (relay ≡ splice)

use crate::{
    control::{
        CpEffect, CpError,
        cap::{CAP_TOKEN_LEN, CapsMask, EndpointResource},
        cluster::CpCommand,
        types::RendezvousId,
    },
    epf::{self, AbortInfo, Action as PolicyAction, Slot as VmSlot},
    observe::{TapEvent, emit, ids},
    rendezvous::{Generation, Lane, Port, SessionId},
    transport::{Transport, TransportError, TransportMetrics, trace::TapFrame},
};

use core::convert::TryInto;


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardError {
    Cancelled,
    TransportFailed,
    StaleGeneration {
        lane: Lane,
        last: Generation,
        new: Generation,
    },
    LaneOutOfRange {
        lane: Lane,
    },
    UnknownSession {
        sid: SessionId,
    },
    InvalidControlPayload,
    LaneMismatch {
        expected: Lane,
        provided: Lane,
    },
    GenerationOverflow {
        lane: Lane,
        last: Generation,
    },
    InvalidInitial {
        lane: Lane,
        new: Generation,
    },
    SpliceInProgress {
        lane: Lane,
    },
    SpliceNoPending {
        lane: Lane,
    },
    CpFailed(CpError),
}

fn tap_event(ts: u32, id: u16, arg0: u32, arg1: u32) -> TapEvent {
    crate::observe::RawEvent::new(ts, id, arg0, arg1)
}

/// Outcome of a splice operation.
///
/// The `Forward` helper keeps owner state internal, upgrading it to
/// `Committed` on success or restoring `Ckpt` on abort. Nothing is returned to
/// the caller; the rendezvous continues to own the original witness.
///
/// This eliminates `'rv` from the return type, allowing HRTB closures to
/// return `SpliceOutcome` without E0521 errors.
pub enum SpliceOutcome {
    Committed { generation: Generation },
    Aborted { error: ForwardError },
}

/// Full splice outcome for distributed (two-Rendezvous) splice.
///
/// Both source and destination `Forward` instances manage owner state
/// internally, promoting to `Committed` on success or restoring `Ckpt` on
/// abort. No owner witness escapes, eliminating `'src` / `'dst` complications
/// for higher-ranked closures.
pub enum SpliceOutcomeFull {
    Committed { generation: Generation },
    Aborted { error: ForwardError },
}

impl core::fmt::Debug for SpliceOutcome {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SpliceOutcome::Committed { generation } => f
                .debug_struct("SpliceOutcome::Committed")
                .field("generation", generation)
                .finish(),
            SpliceOutcome::Aborted { error } => f
                .debug_struct("SpliceOutcome::Aborted")
                .field("error", error)
                .finish(),
        }
    }
}

impl core::fmt::Debug for SpliceOutcomeFull {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SpliceOutcomeFull::Committed { generation } => f
                .debug_struct("SpliceOutcomeFull::Committed")
                .field("generation", generation)
                .finish(),
            SpliceOutcomeFull::Aborted { error } => f
                .debug_struct("SpliceOutcomeFull::Aborted")
                .field("error", error)
                .finish(),
        }
    }
}

/// Forward is a Port/Transport optimisation that never stores owner state.
///
/// The helper is intentionally reusable: every call borrows a fresh lane
/// witness, mints capability tokens on demand through the rendezvous brand, and
/// releases everything once the operation completes. This avoids the
/// `Ckpt → Committed → RolledBack` ownership bookkeeping that would otherwise
/// leak lifetimes out of the rendezvous core while remaining compatible with
/// higher-ranked closures.
pub struct Forward<
    'r,
    T: Transport,
    E: crate::control::cap::EpochTable = crate::control::cap::EpochInit,
> {
    port: Port<'r, T, E>,
    sid: SessionId,
    epoch: crate::control::cap::EndpointEpoch<'r, E>,
    pending_fences: Option<(u32, u32)>,
}

/// Decomposed pieces returned by [`Forward::into_parts`].
///
/// ForwardParts contains only the port, session ID, epoch witness, and any
/// pending fence counters. Owner tokens are never stored; callers mint them on
/// demand via a rendezvous brand when invoking forwarding operations again.
pub struct ForwardParts<
    'r,
    T: Transport,
    E: crate::control::cap::EpochTable = crate::control::cap::EpochInit,
> {
    pub port: Port<'r, T, E>,
    pub sid: SessionId,
    pub epoch: crate::control::cap::EndpointEpoch<'r, E>,
    pub pending_fences: Option<(u32, u32)>,
}

impl<'r, T: Transport, E: crate::control::cap::EpochTable> Forward<'r, T, E> {
    fn emit_policy(&self, id: u16, arg0: u32, arg1: u32) {
        let ts = self.port.clock().now32();
        emit(self.port.tap(), crate::observe::RawEvent::new(ts, id, arg0, arg1));
    }

    fn eval_policy(&self, event_id: u16, arg0: u32, arg1: u32) -> PolicyAction {
        let ts = self.port.clock().now32();
        let event = crate::observe::RawEvent::new(ts, event_id, arg0, arg1);
        let _ = self.port.flush_transport_events();
        let transport_metrics = self.port.transport().metrics().snapshot();
        epf::run_with(
            self.port.host_slots(),
            VmSlot::Forward,
            &event,
            self.port.caps_mask(),
            Some(self.sid),
            Some(self.port.lane),
            move |ctx| {
                ctx.set_transport_snapshot(transport_metrics);
            },
        )
    }

    fn apply_policy_decision(
        &self,
        action: PolicyAction,
        sid: SessionId,
        allow_continue_on_abort: bool,
    ) -> Result<(), ForwardError> {
        match action {
            PolicyAction::Proceed => Ok(()),
            PolicyAction::Abort(info) => self.handle_abort(info, sid, allow_continue_on_abort),
            PolicyAction::Ra(_) => {
                // Should be rejected by dispatch::ensure_allowed; treat as trap defensively.
                self.emit_policy(crate::observe::policy_trap(), 0xFFFF, sid.raw());
                self.emit_policy(crate::observe::policy_abort(), 0xFFFF, sid.raw());
                if allow_continue_on_abort {
                    Ok(())
                } else {
                    Err(ForwardError::Cancelled)
                }
            }
            PolicyAction::Tap { id, arg0, arg1 } => {
                self.emit_policy(id, arg0, arg1);
                Ok(())
            }
            PolicyAction::Route { .. } => {
                // Route decisions are only valid from Slot::Route; ignore elsewhere.
                Ok(())
            }
        }
    }

    fn handle_abort(
        &self,
        info: AbortInfo,
        sid: SessionId,
        allow_continue_on_abort: bool,
    ) -> Result<(), ForwardError> {
        if let Some(_trap) = info.trap {
            self.emit_policy(crate::observe::policy_trap(), info.reason as u32, sid.raw());
        }
        self.emit_policy(
            crate::observe::policy_abort(),
            info.reason as u32,
            sid.raw(),
        );
        if allow_continue_on_abort {
            Ok(())
        } else {
            Err(ForwardError::Cancelled)
        }
    }

    fn decode_control_frame(
        bytes: &[u8],
    ) -> Result<crate::control::ControlFrame<'static, EndpointResource>, ForwardError> {
        if bytes.len() != CAP_TOKEN_LEN {
            return Err(ForwardError::InvalidControlPayload);
        }
        let array: [u8; CAP_TOKEN_LEN] = bytes
            .try_into()
            .map_err(|_| ForwardError::InvalidControlPayload)?;
        Ok(crate::control::ControlFrame::from_wire_bytes(array))
    }

    /// Create a new Forward instance from a port and session identifier.
    ///
    /// The forwarder keeps no persistent owner; later splice/relay operations
    /// accept a rendezvous brand to mint the required witnesses on demand.
    pub fn new(port: Port<'r, T, E>, sid: SessionId) -> Self {
        Self {
            port,
            sid,
            epoch: crate::control::cap::EndpointEpoch::new(),
            pending_fences: None,
        }
    }

    #[inline]
    pub fn caps_mask(&self) -> CapsMask {
        self.port.caps_mask()
    }

    /// Supply fence counters to be recorded on the next splice operation.
    pub fn set_fences(&mut self, tx: u32, rx: u32) {
        self.pending_fences = Some((tx, rx));
    }

    /// Disassemble the forwarder back into its constituent parts.
    ///
    /// This is primarily useful for higher-level orchestration layers (e.g.
    /// endpoint reroute) that temporarily move a [`Port`] into the forwarding
    /// helper and need to regain ownership afterwards.
    pub fn into_parts(self) -> ForwardParts<'r, T, E> {
        let Self {
            port,
            sid,
            epoch,
            pending_fences,
        } = self;
        ForwardParts {
            port,
            sid,
            epoch,
            pending_fences,
        }
    }

    pub fn from_parts(parts: ForwardParts<'r, T, E>) -> Self {
        Self {
            port: parts.port,
            sid: parts.sid,
            epoch: parts.epoch,
            pending_fences: parts.pending_fences,
        }
    }

    /// Relay simply forwards frames; callers are expected to manage sequencing.
    ///
    /// This is the "safe but involves copying" forwarding mechanism. Each frame is
    /// forwarded verbatim through the transport layer.
    ///
    /// # Observational behavior
    ///
    /// With the `trace` feature enabled, each `relay()` call generates a single
    /// `RELAY_FORWARD` tap event containing:
    /// - Session ID
    /// - Lane, role, label, flags from the frame metadata
    ///
    /// This contrasts with [`splice()`](Self::splice), which generates
    /// `SPLICE_BEGIN`/`SPLICE_COMMIT` pairs. Despite different tap patterns,
    /// both mechanisms are observationally equivalent when normalized via
    /// `normalise::forward_trace()` - they collapse to the same `ForwardEvent` sequence.
    ///
    /// # Thread safety
    ///
    /// `&mut self` ensures that a single forwarder cannot issue overlapping
    /// sends that would alias the underlying transport handle.
    ///
    /// ```ignore
    /// use hibana::transport::{trace::{TapFrame, TapFrameMeta}, wire::{FrameFlags, Payload}};
    /// use hibana::transport::forward::Forward;
    /// use hibana::transport::Transport;
    ///
    /// async fn overlap<'r, T, U, C, const MAX_RV: usize>(
    ///     mut fwd: Forward<'r, T>,
    ///     cluster: &mut hibana::runtime::SessionCluster<'r, T, U, C, MAX_RV>,
    ///     rv_id: hibana::control::types::RendezvousId,
    /// )
    /// where
    ///     T: Transport,
    /// {
    ///     let fut1 = fwd.relay(
    ///         cluster,
    ///         rv_id,
    ///         TapFrame::new(TapFrameMeta::new(0, 0, 0, FrameFlags::EMPTY), Payload::new(&[])),
    ///     );
    ///     let fut2 = fwd.relay(
    ///         cluster,
    ///         rv_id,
    ///         TapFrame::new(TapFrameMeta::new(0, 0, 1, FrameFlags::EMPTY), Payload::new(&[])),
    ///     );
    ///     let _ = (fut1, fut2);
    /// }
    /// ```
    pub async fn relay<'a, 'f, U, C, const MAX_RV: usize>(
        &mut self,
        cluster: &mut crate::runtime::SessionCluster<'a, T, U, C, MAX_RV>,
        rv_id: RendezvousId,
        frame: TapFrame<'f>,
    ) -> core::result::Result<(), ForwardError>
    where
        'r: 'f,
        U: crate::runtime::consts::LabelUniverse + 'a,
        C: crate::runtime::config::Clock + 'a,
    {
        let meta = frame.meta;
        let flags = meta.flags;
        #[cfg(not(any(test, feature = "std")))]
        let _ = flags;

        if meta.label == crate::runtime::consts::LABEL_SPLICE_INTENT
            || meta.label == crate::runtime::consts::LABEL_SPLICE_ACK
        {
            let control_frame = Self::decode_control_frame(frame.payload.as_bytes())?;
            let tag = control_frame.as_generic().resource_tag();

            let packed = ((meta.role as u32) << 24)
                | ((meta.lane as u32) << 16)
                | ((meta.label as u32) << 8)
                | tag as u32;
            let action = self.eval_policy(ids::FORWARD_CONTROL, meta.sid, packed);
            self.apply_policy_decision(action, self.sid, false)?;

            if let Err(err) = cluster.dispatch_typed_control_frame(rv_id, control_frame, None) {
                match err {
                    CpError::Authorisation {
                        effect: CpEffect::SpliceAck,
                    } => {}
                    CpError::ReplayDetected { .. } => {}
                    other => return Err(ForwardError::CpFailed(other)),
                }
            }

            let ts = self.port.clock().now32();
            emit(
                self.port.tap(),
                tap_event(ts, ids::FORWARD_CONTROL, meta.sid, packed),
            );

            return Ok(());
        }

        if meta.label == crate::runtime::consts::LABEL_REROUTE {
            let control_frame = Self::decode_control_frame(frame.payload.as_bytes())?;
            let tag = control_frame.as_generic().resource_tag();

            let packed = ((meta.role as u32) << 24)
                | ((meta.lane as u32) << 16)
                | ((meta.label as u32) << 8)
                | tag as u32;
            let action = self.eval_policy(ids::FORWARD_CONTROL, meta.sid, packed);
            self.apply_policy_decision(action, self.sid, false)?;

            if let Err(err) = cluster.dispatch_typed_control_frame(rv_id, control_frame, None) {
                match err {
                    CpError::Authorisation {
                        effect: CpEffect::SpliceAck,
                    } => {}
                    CpError::ReplayDetected { .. } => {}
                    other => return Err(ForwardError::CpFailed(other)),
                }
            }

            let ts = self.port.clock().now32();
            emit(
                self.port.tap(),
                tap_event(ts, ids::FORWARD_CONTROL, meta.sid, packed),
            );

            return Ok(());
        }

        if meta.label == crate::runtime::consts::LABEL_CANCEL {
            let control_frame = Self::decode_control_frame(frame.payload.as_bytes())?;
            let tag = control_frame.as_generic().resource_tag();

            let packed = ((meta.role as u32) << 24)
                | ((meta.lane as u32) << 16)
                | ((meta.label as u32) << 8)
                | tag as u32;
            let action = self.eval_policy(ids::FORWARD_CONTROL, meta.sid, packed);
            self.apply_policy_decision(action, self.sid, true)?;

            cluster
                .dispatch_typed_control_frame(rv_id, control_frame, None)
                .map_err(|_| ForwardError::UnknownSession { sid: self.sid })?;

            let ts = self.port.clock().now32();
            emit(
                self.port.tap(),
                tap_event(ts, ids::FORWARD_CONTROL, meta.sid, packed),
            );

            return Err(ForwardError::Cancelled);
        }

        if meta.label == crate::runtime::consts::LABEL_CHECKPOINT {
            let control_frame = Self::decode_control_frame(frame.payload.as_bytes())?;
            let tag = control_frame.as_generic().resource_tag();

            let packed = ((meta.role as u32) << 24)
                | ((meta.lane as u32) << 16)
                | ((meta.label as u32) << 8)
                | tag as u32;
            let action = self.eval_policy(ids::FORWARD_CONTROL, meta.sid, packed);
            self.apply_policy_decision(action, self.sid, false)?;

            cluster
                .dispatch_typed_control_frame(rv_id, control_frame, None)
                .map_err(|_| ForwardError::UnknownSession { sid: self.sid })?;

            let ts = self.port.clock().now32();
            emit(
                self.port.tap(),
                tap_event(ts, ids::FORWARD_CONTROL, meta.sid, packed),
            );

            return Ok(());
        }

        if meta.label == crate::runtime::consts::LABEL_COMMIT {
            let control_frame = Self::decode_control_frame(frame.payload.as_bytes())?;
            let tag = control_frame.as_generic().resource_tag();

            let packed = ((meta.role as u32) << 24)
                | ((meta.lane as u32) << 16)
                | ((meta.label as u32) << 8)
                | tag as u32;
            let action = self.eval_policy(ids::FORWARD_CONTROL, meta.sid, packed);
            self.apply_policy_decision(action, self.sid, false)?;

            cluster
                .dispatch_typed_control_frame(rv_id, control_frame, None)
                .map_err(ForwardError::CpFailed)?;

            let ts = self.port.clock().now32();
            emit(
                self.port.tap(),
                tap_event(ts, ids::FORWARD_CONTROL, meta.sid, packed),
            );

            return Ok(());
        }

        if meta.label == crate::runtime::consts::LABEL_ROLLBACK {
            let control_frame = Self::decode_control_frame(frame.payload.as_bytes())?;
            let tag = control_frame.as_generic().resource_tag();

            let packed = ((meta.role as u32) << 24)
                | ((meta.lane as u32) << 16)
                | ((meta.label as u32) << 8)
                | tag as u32;
            let action = self.eval_policy(ids::FORWARD_CONTROL, meta.sid, packed);
            self.apply_policy_decision(action, self.sid, false)?;

            let _ = cluster.dispatch_typed_control_frame(rv_id, control_frame, None);

            let ts = self.port.clock().now32();
            emit(
                self.port.tap(),
                tap_event(ts, ids::FORWARD_CONTROL, meta.sid, packed),
            );

            return Ok(());
        }

        let packed = ((meta.role as u32) << 24)
            | ((meta.lane as u32) << 16)
            | ((meta.label as u32) << 8)
            | flags.bits() as u32;
        let action = self.eval_policy(ids::RELAY_FORWARD, meta.sid, packed);
        self.apply_policy_decision(action, self.sid, false)?;
        let ts = self.port.clock().now32();
        emit(
            self.port.tap(),
            tap_event(ts, ids::RELAY_FORWARD, self.sid.raw(), packed),
        );

        let transport = self.port.transport();
        let tx_ptr = self.port.tx_ptr();
        let payload_view = frame.payload;
        unsafe {
            transport
                .send(&mut *tx_ptr, payload_view, self.port.role())
                .await
                .map_err(|err| match err.into() {
                    TransportError::Offline => ForwardError::Cancelled,
                    TransportError::Failed => ForwardError::TransportFailed,
                })?
        }

        Ok(())
    }

    /// Splice performs a two-step direct connection (fence → gen++ → release).
    ///
    /// This is the "zero-copy" forwarding mechanism that directly connects the source
    /// and destination rendezvous without copying frame data. The generation must be
    /// strictly increasing.
    ///
    /// # Two-step protocol
    ///
    /// 1. **Begin phase**: Validates generation ordering and records fences
    /// 2. **Commit phase**: Atomically advances generation and releases old lane state
    ///
    /// # Observational behavior
    ///
    /// With the `trace` feature enabled, each `splice()` call generates two tap events:
    /// - `SPLICE_BEGIN`: Records the start of the splice operation
    /// - `SPLICE_COMMIT`: Records successful completion and generation advancement
    ///
    /// This contrasts with [`relay()`](Self::relay), which generates single
    /// `RELAY_FORWARD` events. Despite different tap patterns, both mechanisms are
    /// observationally equivalent when normalized via `normalise::forward_trace()` -
    /// the `BEGIN`/`COMMIT` pair collapses to a single `ForwardEvent::Splice`.
    ///
    /// # Generation ordering
    ///
    /// The `gen` parameter must be strictly greater than the last generation for this lane.
    /// Violations return `ForwardError::StaleGeneration` or `ForwardError::InvalidInitial`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Initial splice to establish lane
    /// forward.splice(&rendezvous_src, Generation(0)).await?;
    ///
    /// // Subsequent splices with advancing generations
    /// forward.splice(&rendezvous_src, Generation(1)).await?;
    /// forward.splice(&rendezvous_src, Generation(2)).await?;
    /// ```
    ///
    /// See `examples/relay_splice_equivalence.rs` for a complete demonstration of
    /// relay ≡ splice observational equivalence.
    pub async fn splice<'a, U, C, const MAX_RV: usize>(
        &mut self,
        cluster: &mut crate::runtime::SessionCluster<'a, T, U, C, MAX_RV>,
        rv_id: RendezvousId,
        r#gen: Generation,
    ) -> core::result::Result<(), ForwardError>
    where
        U: crate::runtime::consts::LabelUniverse + 'a,
        C: crate::runtime::config::Clock + 'a,
    {
        let fences = self.pending_fences.take();
        let lane = self.port.lane;
        let sid = self.sid;

        let envelope = CpCommand::splice_local_begin(sid, lane, r#gen, fences);

        cluster
            .run_effect(rv_id, envelope)
            .map_err(ForwardError::CpFailed)?;

        let envelope = CpCommand::splice_local_commit(sid, lane);

        cluster
            .run_effect(rv_id, envelope)
            .map_err(ForwardError::CpFailed)?;

        Ok(())
    }

    /// Attempt a two-step splice while owning the lane witness.
    ///
    /// On success, returns a [`SpliceOutcome::Committed`] that carries the
    /// advanced owner (`Ckpt → Committed`) and the generation that was committed.
    /// The caller is expected to continue with the committed owner (typically to
    /// resume endpoint execution on the new lane).
    ///
    /// On failure, returns [`SpliceOutcome::Aborted`] with the original
    /// `Owner<Ckpt>` so the caller can decide whether to retry or continue
    /// relaying. The internal owner slot is cleared; callers wishing to retry must
    /// re-insert the owner via [`restore_owner`](Self::restore_owner) before
    /// High-level splice attempt (data + control plane coordination).
    ///
    /// Forward never holds owner state; commit/rollback is driven internally via
    /// the rendezvous `SpliceDelegate`, which controls the brand needed to mint
    /// or retire capability tokens.
    ///
    /// This design eliminates both `'rv` and Owner from Forward's persistent state,
    /// solving E0521 errors in HRTB closures (e.g., `Rendezvous::with_forward`).
    ///
    /// # Implementation Note
    ///
    /// The caller (Rendezvous) performs the commit via `commit_splice()` which is
    /// invoked inside `splice()`. Forward remains stateless between operations and
    /// can be reused across multiple splice attempts.
    pub async fn try_splice<'a, U, C, const MAX_RV: usize>(
        &mut self,
        cluster: &mut crate::runtime::SessionCluster<'a, T, U, C, MAX_RV>,
        rv_id: RendezvousId,
        r#gen: Generation,
    ) -> SpliceOutcome
    where
        U: crate::runtime::consts::LabelUniverse + 'a,
        C: crate::runtime::config::Clock + 'a,
    {
        match self.splice(cluster, rv_id, r#gen).await {
            Ok(()) => SpliceOutcome::Committed { generation: r#gen },
            Err(err) => {
                #[cfg(feature = "std")]
                if matches!(
                    err,
                    ForwardError::StaleGeneration { .. }
                        | ForwardError::InvalidInitial { .. }
                        | ForwardError::GenerationOverflow { .. }
                        | ForwardError::SpliceInProgress { .. }
                        | ForwardError::SpliceNoPending { .. }
                        | ForwardError::LaneMismatch { .. }
                        | ForwardError::CpFailed(_)
                ) {
                    use std::io::Write;
                    let _ = writeln!(&mut ::std::io::stderr(), "try_splice: aborted: {:?}", err);
                }
                SpliceOutcome::Aborted { error: err }
            }
        }
    }

}

/// Convenience alias to reuse endpoint error vocabulary.
pub type ResultForward<T> = core::result::Result<T, ForwardError>;
