//! Internal endpoint kernel built on top of `PhaseCursor`.
//!
//! The kernel endpoint owns the rendezvous port outright and advances
//! according to the typestate cursor obtained from `RoleProgram` projection.

use core::{convert::TryFrom, future::poll_fn, ops::ControlFlow, task::Poll};

use super::flow::CapFlow;
use crate::binding::{BindingSlot, NoBinding};
use crate::eff::EffIndex;
use crate::global::const_dsl::{PolicyMode, ScopeId, ScopeKind};
use crate::global::role_program::MAX_LANES;
use crate::global::typestate::{
    ARM_SHARED, JumpReason, LoopMetadata, LoopRole, MAX_FIRST_RECV_DISPATCH, PassiveArmNavigation,
    PhaseCursor, RecvMeta, SendMeta, StateIndex, state_index_to_usize,
};
use crate::global::{
    CanonicalControl, ControlHandling, ControlPayloadKind, ExternalControl, MessageSpec, NoControl,
    SendableLabel,
};
use crate::runtime::config::Clock;
use crate::{
    control::types::{Lane, RendezvousId, SessionId},
    control::{
        cap::resource_kinds::{
            CancelAckKind, CancelKind, CheckpointKind, CommitKind, LoopBreakKind, LoopContinueKind,
            LoopDecisionHandle, RerouteKind, RollbackKind, RouteDecisionHandle, RouteDecisionKind,
            SpliceAckKind, SpliceHandle, SpliceIntentKind, splice_flags,
        },
        cap::{
            mint::{
                CAP_TOKEN_LEN, CapShot, ControlMint, E0, EndpointEpoch, EpochTable, EpochTbl,
                GenericCapToken, MintConfigMarker, Owner, ResourceKind,
            },
            typed_tokens::{CapFlowToken, CapRegisteredToken},
        },
        cluster::{
            core::{DynamicResolution, SpliceOperands},
            effects::CpEffect,
            error::CpError,
        },
    },
    endpoint::{
        RecvError, RecvResult, SendError, SendResult,
        affine::LaneGuard,
        control::{ControlOutcome, SessionControlCtx},
    },
    epf::{self, AbortInfo, Action, vm::Slot},
    observe::core::{TapEvent, emit},
    observe::scope::ScopeTrace,
    observe::{events, ids, policy_abort, policy_trap},
    rendezvous::{port::Port, tables::LoopDisposition},
    runtime::consts::{
        LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE, LABEL_REROUTE, LABEL_SPLICE_ACK,
        LABEL_SPLICE_INTENT, LabelUniverse,
    },
    transport::{
        Transport, TransportMetrics,
        trace::TapFrameMeta,
        wire::{FrameFlags, Payload, WireDecodeOwned, WireEncode},
    },
};

/// Classification of control labels for dynamic policy evaluation dispatch.
///
/// This enum provides a clean abstraction over the raw label constants,
/// grouping them by their evaluation semantics:
/// - `Loop`: Labels that require loop-specific evaluation (continue/break decisions)
/// - `SpliceOrReroute`: Labels validated later in `mint_control_token_with_handle`
/// - `Route`: Standard route arm evaluation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DynamicLabelClass {
    /// Loop control labels (LABEL_LOOP_CONTINUE, LABEL_LOOP_BREAK)
    Loop,
    /// Splice and reroute labels (validated in mint_control_token_with_handle)
    SpliceOrReroute,
    /// Standard route decision labels
    Route,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoutePolicyDecision {
    RouteArm(u8),
    DelegateResolver,
    Abort(u16),
    Defer { retry_hint: u8, source: DeferSource },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeferSource {
    Epf,
    Resolver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeferReason {
    Unsupported = 1,
    NoEvidence = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrontierKind {
    Route,
    Loop,
    Parallel,
    PassiveObserver,
}

impl FrontierKind {
    #[inline]
    const fn as_audit_tag(self) -> u8 {
        match self {
            Self::Route => 1,
            Self::Loop => 2,
            Self::Parallel => 3,
            Self::PassiveObserver => 4,
        }
    }

    #[inline]
    const fn bit(self) -> u8 {
        match self {
            Self::Route => 1 << 0,
            Self::Loop => 1 << 1,
            Self::Parallel => 1 << 2,
            Self::PassiveObserver => 1 << 3,
        }
    }
}

#[inline]
fn checked_state_index(idx: usize) -> Option<StateIndex> {
    u16::try_from(idx).ok().map(StateIndex::new)
}

/// Classify a label for dynamic policy evaluation dispatch.
///
/// This function maps raw label constants to their semantic classification,
/// providing a single point of truth for label-based dispatch logic.
#[inline]
const fn classify_dynamic_label(label: u8) -> DynamicLabelClass {
    match label {
        LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK => DynamicLabelClass::Loop,
        LABEL_SPLICE_INTENT | LABEL_SPLICE_ACK | LABEL_REROUTE => {
            DynamicLabelClass::SpliceOrReroute
        }
        _ => DynamicLabelClass::Route,
    }
}

#[inline]
fn route_policy_input_arg0(input: &[u32; 4]) -> u32 {
    input[0]
}

#[inline]
fn route_policy_decision_from_action(action: Action, policy_id: u16) -> RoutePolicyDecision {
    match action {
        Action::Route { arm } if arm <= 1 => RoutePolicyDecision::RouteArm(arm),
        Action::Route { .. } => RoutePolicyDecision::Abort(policy_id),
        Action::Abort(info) => RoutePolicyDecision::Abort(info.reason),
        Action::Defer { retry_hint } => RoutePolicyDecision::Defer {
            retry_hint,
            source: DeferSource::Epf,
        },
        Action::Proceed | Action::Tap { .. } => RoutePolicyDecision::DelegateResolver,
    }
}

#[inline]
fn stage_transport_payload(scratch: &mut [u8], payload: &[u8]) -> RecvResult<usize> {
    if payload.len() > scratch.len() {
        return Err(RecvError::PhaseInvariant);
    }
    scratch[..payload.len()].copy_from_slice(payload);
    Ok(payload.len())
}

#[inline]
fn decode_phase_invariant() -> RecvError {
    RecvError::PhaseInvariant
}

#[inline]
fn validate_route_decision_scope(scope: ScopeId, policy_scope: ScopeId) -> SendResult<()> {
    if scope.is_none() {
        return Err(SendError::PhaseInvariant);
    }
    if !policy_scope.is_none() && scope != policy_scope {
        return Err(SendError::PhaseInvariant);
    }
    Ok(())
}

#[cfg(test)]
fn resolve_route_decision_handle_with_policy<F>(
    scope: ScopeId,
    policy_scope: ScopeId,
    policy_decision: RoutePolicyDecision,
    delegate_resolver: F,
) -> SendResult<RouteDecisionHandle>
where
    F: FnOnce() -> SendResult<RouteDecisionHandle>,
{
    validate_route_decision_scope(scope, policy_scope)?;
    match policy_decision {
        RoutePolicyDecision::RouteArm(arm) => Ok(RouteDecisionHandle { scope, arm }),
        RoutePolicyDecision::Abort(reason) => Err(SendError::PolicyAbort { reason }),
        RoutePolicyDecision::Defer { .. } => delegate_resolver(),
        RoutePolicyDecision::DelegateResolver => delegate_resolver(),
    }
}

#[cfg(test)]
fn endpoint_scope_label_meta<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>(
    endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    scope_id: ScopeId,
    loop_meta: ScopeLoopMeta,
) -> ScopeLabelMeta
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint, B>::scope_label_meta(
        &endpoint.cursor,
        scope_id,
        loop_meta,
    )
}

#[cfg(test)]
mod route_policy_tests {
    use super::*;

    #[test]
    fn route_policy_input_arg0_defaults_to_zero() {
        assert_eq!(route_policy_input_arg0(&[0; 4]), 0);
    }

    #[test]
    fn route_policy_input_arg0_reads_arg0() {
        assert_eq!(
            route_policy_input_arg0(&[0xABCD_1234, 0, 0, 0]),
            0xABCD_1234
        );
    }

    #[test]
    fn route_policy_action_mapping_is_explicit() {
        assert_eq!(
            route_policy_decision_from_action(Action::Route { arm: 1 }, 44),
            RoutePolicyDecision::RouteArm(1)
        );
        assert_eq!(
            route_policy_decision_from_action(Action::Route { arm: 2 }, 44),
            RoutePolicyDecision::Abort(44)
        );
        assert_eq!(
            route_policy_decision_from_action(
                Action::Abort(AbortInfo {
                    reason: 77,
                    trap: None,
                }),
                44
            ),
            RoutePolicyDecision::Abort(77)
        );
        assert_eq!(
            route_policy_decision_from_action(Action::Proceed, 44),
            RoutePolicyDecision::DelegateResolver
        );
        assert_eq!(
            route_policy_decision_from_action(
                Action::Tap {
                    id: 1,
                    arg0: 2,
                    arg1: 3
                },
                44
            ),
            RoutePolicyDecision::DelegateResolver
        );
        assert_eq!(
            route_policy_decision_from_action(Action::Defer { retry_hint: 9 }, 44),
            RoutePolicyDecision::Defer {
                retry_hint: 9,
                source: DeferSource::Epf
            }
        );
    }

    #[test]
    fn route_policy_delegates_to_resolver_result() {
        let scope = ScopeId::generic(10);
        let handle = resolve_route_decision_handle_with_policy(
            scope,
            scope,
            RoutePolicyDecision::DelegateResolver,
            || Ok(RouteDecisionHandle { scope, arm: 1 }),
        )
        .expect("delegation should use resolver");
        assert_eq!(handle.arm, 1);
    }

    #[test]
    fn route_policy_route_arm_skips_resolver_delegation() {
        let scope = ScopeId::generic(14);
        let mut delegate_called = false;
        let handle = resolve_route_decision_handle_with_policy(
            scope,
            scope,
            RoutePolicyDecision::RouteArm(1),
            || {
                delegate_called = true;
                Ok(RouteDecisionHandle { scope, arm: 0 })
            },
        )
        .expect("route arm should be accepted directly");
        assert_eq!(handle.arm, 1);
        assert!(!delegate_called);
    }

    #[test]
    fn route_policy_abort_skips_resolver_delegation() {
        let scope = ScopeId::generic(15);
        let mut delegate_called = false;
        let err = resolve_route_decision_handle_with_policy(
            scope,
            scope,
            RoutePolicyDecision::Abort(77),
            || {
                delegate_called = true;
                Ok(RouteDecisionHandle { scope, arm: 0 })
            },
        )
        .expect_err("abort should short-circuit");
        assert!(matches!(err, SendError::PolicyAbort { reason: 77 }));
        assert!(!delegate_called);
    }

    #[test]
    fn route_policy_delegation_propagates_resolver_abort() {
        let scope = ScopeId::generic(11);
        let err = resolve_route_decision_handle_with_policy(
            scope,
            scope,
            RoutePolicyDecision::DelegateResolver,
            || Err(SendError::PolicyAbort { reason: 99 }),
        )
        .expect_err("resolver abort should propagate");
        assert!(matches!(err, SendError::PolicyAbort { reason: 99 }));
    }

    #[test]
    fn route_policy_enforces_scope_match_before_route_handle() {
        let scope = ScopeId::generic(12);
        let err = resolve_route_decision_handle_with_policy(
            scope,
            ScopeId::generic(13),
            RoutePolicyDecision::RouteArm(0),
            || Ok(RouteDecisionHandle { scope, arm: 0 }),
        )
        .expect_err("scope mismatch must fail");
        assert!(matches!(err, SendError::PhaseInvariant));
    }

    #[test]
    fn route_policy_scope_mismatch_blocks_resolver_delegation() {
        let scope = ScopeId::generic(16);
        let mut delegate_called = false;
        let err = resolve_route_decision_handle_with_policy(
            scope,
            ScopeId::generic(17),
            RoutePolicyDecision::DelegateResolver,
            || {
                delegate_called = true;
                Ok(RouteDecisionHandle { scope, arm: 1 })
            },
        )
        .expect_err("scope mismatch must fail before resolver delegation");
        assert!(matches!(err, SendError::PhaseInvariant));
        assert!(!delegate_called);
    }

    #[test]
    fn route_policy_allows_static_route_scope_without_policy_scope() {
        let scope = ScopeId::generic(18);
        let handle = resolve_route_decision_handle_with_policy(
            scope,
            ScopeId::none(),
            RoutePolicyDecision::RouteArm(1),
            || Ok(RouteDecisionHandle { scope, arm: 0 }),
        )
        .expect("static route scope should remain valid without policy scope");
        assert_eq!(handle, RouteDecisionHandle { scope, arm: 1 });
    }
}

#[cfg(test)]
mod offer_regression_tests {
    use super::*;
    use crate::binding::{Channel, IncomingClassification, TransportOpsError};
    use crate::control::cap::mint::{
        CapError, CapShot, CapsMask, ControlResourceKind, GenericCapToken, ResourceKind,
        SessionScopedKind,
    };
    use crate::control::cap::resource_kinds::{RouteDecisionHandle, RouteDecisionKind};
    use crate::control::cluster::core::SessionCluster;
    use crate::g::{self, Msg, Role};
    use crate::global::const_dsl::{ControlScopeKind, ScopeId};
    use crate::global::role_program::{RoleProgram, project};
    use crate::global::steps::{ProjectRole, SendStep, SeqSteps, StepConcat, StepCons, StepNil};
    use crate::global::{CanonicalControl, ControlHandling};
    use crate::observe::core::TapEvent;
    use crate::runtime::config::{Config, CounterClock};
    use crate::runtime::consts::{DefaultLabelUniverse, LABEL_ROUTE_DECISION, RING_EVENTS};
    use crate::transport::{Transport, TransportError, wire::Payload};
    use core::{
        cell::Cell,
        future::{Future, Ready, ready},
        mem::ManuallyDrop,
        pin::pin,
        task::{Context, Poll},
    };
    use futures::task::noop_waker_ref;
    use std::{
        collections::VecDeque,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        task::Waker,
        vec::Vec,
    };

    #[derive(Default)]
    struct TestBinding {
        incoming: VecDeque<IncomingClassification>,
        recv_payloads: VecDeque<Vec<u8>>,
        polls: Cell<usize>,
    }

    impl TestBinding {
        fn with_incoming(incoming: &[IncomingClassification]) -> Self {
            let mut queue = VecDeque::new();
            queue.extend(incoming.iter().copied());
            Self {
                incoming: queue,
                recv_payloads: VecDeque::new(),
                polls: Cell::new(0),
            }
        }

        fn with_incoming_and_payloads(
            incoming: &[IncomingClassification],
            recv_payloads: &[&[u8]],
        ) -> Self {
            let mut queue = VecDeque::new();
            queue.extend(incoming.iter().copied());
            let mut payloads = VecDeque::new();
            payloads.extend(recv_payloads.iter().map(|payload| payload.to_vec()));
            Self {
                incoming: queue,
                recv_payloads: payloads,
                polls: Cell::new(0),
            }
        }

        fn poll_count(&self) -> usize {
            self.polls.get()
        }
    }

    struct LaneAwareTestBinding {
        incoming: [VecDeque<IncomingClassification>; MAX_LANES],
        polls: [usize; MAX_LANES],
    }

    impl LaneAwareTestBinding {
        fn with_lane_incoming(incoming: &[(u8, IncomingClassification)]) -> Self {
            let mut binding = Self {
                incoming: core::array::from_fn(|_| VecDeque::new()),
                polls: [0; MAX_LANES],
            };
            for (lane, classification) in incoming.iter().copied() {
                let lane_idx = lane as usize;
                if lane_idx < MAX_LANES {
                    binding.incoming[lane_idx].push_back(classification);
                }
            }
            binding
        }

        fn poll_count_for_lane(&self, lane_idx: usize) -> usize {
            self.polls.get(lane_idx).copied().unwrap_or(0)
        }
    }

    impl BindingSlot for LaneAwareTestBinding {
        fn poll_incoming_for_lane(&mut self, logical_lane: u8) -> Option<IncomingClassification> {
            let lane_idx = logical_lane as usize;
            if lane_idx >= MAX_LANES {
                return None;
            }
            self.polls[lane_idx] = self.polls[lane_idx].saturating_add(1);
            self.incoming[lane_idx].pop_front()
        }

        fn on_recv(
            &mut self,
            _channel: Channel,
            _buf: &mut [u8],
        ) -> Result<usize, TransportOpsError> {
            Ok(0)
        }

        fn policy_signals_provider(
            &self,
        ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
            None
        }
    }

    impl BindingSlot for TestBinding {
        fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
            self.polls.set(self.polls.get().saturating_add(1));
            self.incoming.pop_front()
        }

        fn on_recv(
            &mut self,
            _channel: Channel,
            buf: &mut [u8],
        ) -> Result<usize, TransportOpsError> {
            let Some(payload) = self.recv_payloads.pop_front() else {
                return Ok(0);
            };
            let len = core::cmp::min(buf.len(), payload.len());
            buf[..len].copy_from_slice(&payload[..len]);
            Ok(len)
        }

        fn policy_signals_provider(
            &self,
        ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
            None
        }
    }

    const HINT_NONE: u8 = u8::MAX;

    #[derive(Clone, Copy)]
    struct HintOnlyTransport {
        worker_hint: u8,
    }

    impl HintOnlyTransport {
        const fn new(worker_hint: u8) -> Self {
            Self { worker_hint }
        }
    }

    struct HintOnlyRx {
        hint: Cell<u8>,
    }

    impl Transport for HintOnlyTransport {
        type Error = TransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = HintOnlyRx
        where
            Self: 'a;
        type Send<'a>
            = Ready<Result<(), Self::Error>>
        where
            Self: 'a;
        type Recv<'a>
            = Ready<Result<Payload<'a>, Self::Error>>
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            let hint = if local_role == 1 {
                self.worker_hint
            } else {
                HINT_NONE
            };
            (
                (),
                HintOnlyRx {
                    hint: Cell::new(hint),
                },
            )
        }

        fn send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: crate::transport::Outgoing<'f>,
        ) -> Self::Send<'a>
        where
            'a: 'f,
        {
            ready(Ok(()))
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            ready(Ok(Payload::new(&[0u8; 1])))
        }

        fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

        fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

        fn recv_label_hint<'a>(&'a self, rx: &'a Self::Rx<'a>) -> Option<u8> {
            let hint = rx.hint.get();
            if hint == HINT_NONE {
                None
            } else {
                rx.hint.set(HINT_NONE);
                Some(hint)
            }
        }

        fn metrics(&self) -> Self::Metrics {
            ()
        }

        fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
    }

    #[derive(Clone)]
    struct PendingTransport {
        state: Arc<PendingTransportState>,
    }

    impl PendingTransport {
        fn new() -> Self {
            Self {
                state: Arc::new(PendingTransportState::default()),
            }
        }

        fn poll_count(&self) -> usize {
            self.state.polls.load(Ordering::SeqCst)
        }
    }

    #[derive(Default)]
    struct PendingTransportState {
        polls: AtomicUsize,
        ready: AtomicBool,
        waker: Mutex<Option<Waker>>,
    }

    #[derive(Default)]
    struct DeferredIngressState {
        incoming: Mutex<VecDeque<IncomingClassification>>,
        recv_payloads: Mutex<VecDeque<Vec<u8>>>,
        available: AtomicUsize,
    }

    struct DeferredIngressBinding {
        state: Arc<DeferredIngressState>,
        polls: Cell<usize>,
    }

    impl DeferredIngressBinding {
        fn with_incoming_and_payloads(
            state: Arc<DeferredIngressState>,
            incoming: &[IncomingClassification],
            recv_payloads: &[&[u8]],
        ) -> Self {
            {
                let mut queue = state.incoming.lock().expect("deferred ingress incoming lock");
                queue.extend(incoming.iter().copied());
            }
            {
                let mut payloads = state
                    .recv_payloads
                    .lock()
                    .expect("deferred ingress payload lock");
                payloads.extend(recv_payloads.iter().map(|payload| payload.to_vec()));
            }
            Self {
                state,
                polls: Cell::new(0),
            }
        }
    }

    impl BindingSlot for DeferredIngressBinding {
        fn poll_incoming_for_lane(&mut self, _logical_lane: u8) -> Option<IncomingClassification> {
            self.polls.set(self.polls.get().saturating_add(1));
            if self.state.available.load(Ordering::SeqCst) == 0 {
                return None;
            }
            let mut queue = self
                .state
                .incoming
                .lock()
                .expect("deferred ingress incoming lock");
            let classification = queue.pop_front()?;
            self.state.available.fetch_sub(1, Ordering::SeqCst);
            Some(classification)
        }

        fn on_recv(
            &mut self,
            _channel: Channel,
            buf: &mut [u8],
        ) -> Result<usize, TransportOpsError> {
            let mut payloads = self
                .state
                .recv_payloads
                .lock()
                .expect("deferred ingress payload lock");
            let Some(payload) = payloads.pop_front() else {
                return Ok(0);
            };
            let len = core::cmp::min(buf.len(), payload.len());
            buf[..len].copy_from_slice(&payload[..len]);
            Ok(len)
        }

        fn policy_signals_provider(
            &self,
        ) -> Option<&dyn crate::transport::context::PolicySignalsProvider> {
            None
        }
    }

    #[derive(Clone)]
    struct DeferredIngressTransport {
        state: Arc<DeferredIngressState>,
    }

    impl DeferredIngressTransport {
        fn new(state: Arc<DeferredIngressState>) -> Self {
            Self { state }
        }
    }

    struct DeferredIngressRx;

    struct PendingRx;

    struct PendingRecv<'a> {
        state: &'a PendingTransportState,
    }

    impl<'a> Future for PendingRecv<'a> {
        type Output = Result<Payload<'a>, TransportError>;

        fn poll(
            self: core::pin::Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Self::Output> {
            self.state.polls.fetch_add(1, Ordering::SeqCst);
            if self.state.ready.load(Ordering::SeqCst) {
                Poll::Ready(Ok(Payload::new(&[])))
            } else {
                *self.state.waker.lock().expect("pending transport waker lock") =
                    Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    impl Transport for PendingTransport {
        type Error = TransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = PendingRx
        where
            Self: 'a;
        type Send<'a>
            = Ready<Result<(), Self::Error>>
        where
            Self: 'a;
        type Recv<'a>
            = PendingRecv<'a>
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), PendingRx)
        }

        fn send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: crate::transport::Outgoing<'f>,
        ) -> Self::Send<'a>
        where
            'a: 'f,
        {
            ready(Ok(()))
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            PendingRecv { state: &self.state }
        }

        fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

        fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

        fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
            None
        }

        fn metrics(&self) -> Self::Metrics {
            ()
        }

        fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
    }

    impl Transport for DeferredIngressTransport {
        type Error = TransportError;
        type Tx<'a>
            = ()
        where
            Self: 'a;
        type Rx<'a>
            = DeferredIngressRx
        where
            Self: 'a;
        type Send<'a>
            = Ready<Result<(), Self::Error>>
        where
            Self: 'a;
        type Recv<'a>
            = Ready<Result<Payload<'a>, Self::Error>>
        where
            Self: 'a;
        type Metrics = ();

        fn open<'a>(&'a self, _local_role: u8, _session_id: u32) -> (Self::Tx<'a>, Self::Rx<'a>) {
            ((), DeferredIngressRx)
        }

        fn send<'a, 'f>(
            &'a self,
            _tx: &'a mut Self::Tx<'a>,
            _outgoing: crate::transport::Outgoing<'f>,
        ) -> Self::Send<'a>
        where
            'a: 'f,
        {
            ready(Ok(()))
        }

        fn recv<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) -> Self::Recv<'a> {
            self.state.available.fetch_add(1, Ordering::SeqCst);
            ready(Ok(Payload::new(&[])))
        }

        fn requeue<'a>(&'a self, _rx: &'a mut Self::Rx<'a>) {}

        fn drain_events(&self, _emit: &mut dyn FnMut(crate::transport::TransportEvent)) {}

        fn recv_label_hint<'a>(&'a self, _rx: &'a Self::Rx<'a>) -> Option<u8> {
            None
        }

        fn metrics(&self) -> Self::Metrics {
            ()
        }

        fn apply_pacing_update(&self, _interval_us: u32, _burst_bytes: u16) {}
    }

    #[derive(Clone, Copy, Debug)]
    struct RouteHintRightKind;

    impl ResourceKind for RouteHintRightKind {
        type Handle = RouteDecisionHandle;
        const TAG: u8 = RouteDecisionKind::TAG;
        const NAME: &'static str = "RouteHintRightDecision";
        const AUTO_MINT_EXTERNAL: bool = false;

        fn encode_handle(handle: &Self::Handle) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN] {
            handle.encode()
        }

        fn decode_handle(
            data: [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
        ) -> Result<Self::Handle, CapError> {
            RouteDecisionHandle::decode(data)
        }

        fn zeroize(handle: &mut Self::Handle) {
            handle.arm = 0;
            handle.scope = ScopeId::generic(0);
        }

        fn caps_mask(_handle: &Self::Handle) -> CapsMask {
            CapsMask::empty()
        }

        fn scope_id(handle: &Self::Handle) -> Option<ScopeId> {
            Some(handle.scope)
        }
    }

    impl SessionScopedKind for RouteHintRightKind {
        fn handle_for_session(_sid: crate::control::types::SessionId, _lane: Lane) -> Self::Handle {
            RouteDecisionHandle::default()
        }

        fn shot() -> CapShot {
            CapShot::One
        }
    }

    impl crate::control::cap::mint::ControlResourceKind for RouteHintRightKind {
        const LABEL: u8 = 99;
        const SCOPE: ControlScopeKind = ControlScopeKind::Route;
        const TAP_ID: u16 =
            <RouteDecisionKind as crate::control::cap::mint::ControlResourceKind>::TAP_ID;
        const SHOT: CapShot = CapShot::One;
        const HANDLING: ControlHandling = ControlHandling::Canonical;
    }

    impl crate::control::cap::mint::ControlMint for RouteHintRightKind {
        fn mint_handle(_sid: SessionId, _lane: Lane, scope: ScopeId) -> Self::Handle {
            RouteDecisionHandle { scope, arm: 0 }
        }
    }

    const HINT_ROUTE_POLICY_ID: u16 = 601;
    const HINT_LEFT_ARM: g::Program<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_ROUTE_DECISION },
                        GenericCapToken<RouteDecisionKind>,
                        CanonicalControl<RouteDecisionKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>,
        >,
    > = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_ROUTE_DECISION },
                GenericCapToken<RouteDecisionKind>,
                CanonicalControl<RouteDecisionKind>,
            >,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<100, u8>, 0>(),
    );
    const HINT_RIGHT_ARM: g::Program<
        SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        99,
                        GenericCapToken<RouteHintRightKind>,
                        CanonicalControl<RouteHintRightKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>,
        >,
    > = g::seq(
        g::send::<
            Role<0>,
            Role<0>,
            Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>,
            0,
        >()
        .policy::<HINT_ROUTE_POLICY_ID>(),
        g::send::<Role<0>, Role<1>, Msg<101, u8>, 0>(),
    );
    const HINT_ROUTE_PROGRAM: g::Program<
        <SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_ROUTE_DECISION },
                        GenericCapToken<RouteDecisionKind>,
                        CanonicalControl<RouteDecisionKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>,
        > as StepConcat<
            SeqSteps<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<
                            99,
                            GenericCapToken<RouteHintRightKind>,
                            CanonicalControl<RouteHintRightKind>,
                        >,
                    >,
                    StepNil,
                >,
                StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>,
            >,
        >>::Output,
    > = g::route(HINT_LEFT_ARM, HINT_RIGHT_ARM);
    static HINT_CONTROLLER_PROGRAM: RoleProgram<
        'static,
        0,
        <<SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_ROUTE_DECISION },
                        GenericCapToken<RouteDecisionKind>,
                        CanonicalControl<RouteDecisionKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>,
        > as StepConcat<
            SeqSteps<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<
                            99,
                            GenericCapToken<RouteHintRightKind>,
                            CanonicalControl<RouteHintRightKind>,
                        >,
                    >,
                    StepNil,
                >,
                StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>,
            >,
        >>::Output as ProjectRole<Role<0>>>::Output,
    > = project(&HINT_ROUTE_PROGRAM);
    static HINT_WORKER_PROGRAM: RoleProgram<
        'static,
        1,
        <<SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_ROUTE_DECISION },
                        GenericCapToken<RouteDecisionKind>,
                        CanonicalControl<RouteDecisionKind>,
                    >,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<100, u8>>, StepNil>,
        > as StepConcat<
            SeqSteps<
                StepCons<
                    SendStep<
                        Role<0>,
                        Role<0>,
                        Msg<
                            99,
                            GenericCapToken<RouteHintRightKind>,
                            CanonicalControl<RouteHintRightKind>,
                        >,
                    >,
                    StepNil,
                >,
                StepCons<SendStep<Role<0>, Role<1>, Msg<101, u8>>, StepNil>,
            >,
        >>::Output as ProjectRole<Role<1>>>::Output,
    > = project(&HINT_ROUTE_PROGRAM);
    const HINT_LEFT_DATA_LABEL: u8 = 100;
    const HINT_RIGHT_DATA_LABEL: u8 = 101;

    const ENTRY_ARM0_PROGRAM: g::Program<
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
                StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
            >,
        >,
    > = g::seq(
        g::send::<Role<0>, Role<0>, Msg<102, u8>, 0>(),
        g::seq(
            g::send::<Role<0>, Role<1>, Msg<103, u8>, 0>(),
            g::send::<Role<1>, Role<0>, Msg<104, u8>, 0>(),
        ),
    );
    const ENTRY_ARM1_PROGRAM: g::Program<
        SeqSteps<
            StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<1>, Msg<106, u8>>, StepNil>,
                StepCons<SendStep<Role<1>, Role<0>, Msg<107, u8>>, StepNil>,
            >,
        >,
    > = g::seq(
        g::send::<Role<0>, Role<0>, Msg<105, u8>, 0>(),
        g::seq(
            g::send::<Role<0>, Role<1>, Msg<106, u8>, 0>(),
            g::send::<Role<1>, Role<0>, Msg<107, u8>, 0>(),
        ),
    );
    const ENTRY_ROUTE_PROGRAM: g::Program<
        <SeqSteps<
            StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
                StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
            >,
        > as StepConcat<
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
                SeqSteps<
                    StepCons<SendStep<Role<0>, Role<1>, Msg<106, u8>>, StepNil>,
                    StepCons<SendStep<Role<1>, Role<0>, Msg<107, u8>>, StepNil>,
                >,
            >,
        >>::Output,
    > = g::route(ENTRY_ARM0_PROGRAM, ENTRY_ARM1_PROGRAM);
    static ENTRY_CONTROLLER_PROGRAM: RoleProgram<
        'static,
        0,
        <<SeqSteps<
            StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
                StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
            >,
        > as StepConcat<
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
                SeqSteps<
                    StepCons<SendStep<Role<0>, Role<1>, Msg<106, u8>>, StepNil>,
                    StepCons<SendStep<Role<1>, Role<0>, Msg<107, u8>>, StepNil>,
                >,
            >,
        >>::Output as ProjectRole<Role<0>>>::Output,
    > = project(&ENTRY_ROUTE_PROGRAM);
    static ENTRY_WORKER_PROGRAM: RoleProgram<
        'static,
        1,
        <<SeqSteps<
            StepCons<SendStep<Role<0>, Role<0>, Msg<102, u8>>, StepNil>,
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<1>, Msg<103, u8>>, StepNil>,
                StepCons<SendStep<Role<1>, Role<0>, Msg<104, u8>>, StepNil>,
            >,
        > as StepConcat<
            SeqSteps<
                StepCons<SendStep<Role<0>, Role<0>, Msg<105, u8>>, StepNil>,
                SeqSteps<
                    StepCons<SendStep<Role<0>, Role<1>, Msg<106, u8>>, StepNil>,
                    StepCons<SendStep<Role<1>, Role<0>, Msg<107, u8>>, StepNil>,
                >,
            >,
        >>::Output as ProjectRole<Role<1>>>::Output,
    > = project(&ENTRY_ROUTE_PROGRAM);
    const ENTRY_ARM0_SIGNAL_LABEL: u8 = 103;

    #[test]
    fn binding_inbox_take_is_one_shot() {
        let classification = IncomingClassification {
            label: 7,
            instance: 3,
            has_fin: false,
            channel: Channel::new(1),
        };
        let mut binding = TestBinding::with_incoming(&[classification]);
        let mut inbox = BindingInbox::EMPTY;

        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(classification));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), None);

        inbox.put_back(0, classification);
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(classification));
    }

    #[test]
    fn binding_inbox_take_matching_skips_head_mismatch() {
        let head = IncomingClassification {
            label: 7,
            instance: 3,
            has_fin: false,
            channel: Channel::new(1),
        };
        let expected = IncomingClassification {
            label: 9,
            instance: 4,
            has_fin: false,
            channel: Channel::new(2),
        };
        let mut binding = TestBinding::with_incoming(&[head, expected]);
        let mut inbox = BindingInbox::EMPTY;

        let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.label);
        assert_eq!(picked, Some(expected));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(head));
    }

    #[test]
    fn binding_inbox_take_matching_scans_buffered_entries() {
        let first = IncomingClassification {
            label: 3,
            instance: 1,
            has_fin: false,
            channel: Channel::new(11),
        };
        let second = IncomingClassification {
            label: 4,
            instance: 2,
            has_fin: false,
            channel: Channel::new(12),
        };
        let expected = IncomingClassification {
            label: 5,
            instance: 3,
            has_fin: false,
            channel: Channel::new(13),
        };
        let mut binding = TestBinding::default();
        let mut inbox = BindingInbox::EMPTY;
        assert!(inbox.push_back(0, first));
        assert!(inbox.push_back(0, second));
        assert!(inbox.push_back(0, expected));

        let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.label);
        assert_eq!(picked, Some(expected));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(second));
    }

    #[test]
    fn binding_inbox_nonempty_mask_tracks_buffered_lanes() {
        let first = IncomingClassification {
            label: 3,
            instance: 1,
            has_fin: false,
            channel: Channel::new(11),
        };
        let second = IncomingClassification {
            label: 4,
            instance: 2,
            has_fin: false,
            channel: Channel::new(12),
        };
        let mut binding = TestBinding::default();
        let mut inbox = BindingInbox::EMPTY;
        assert!(!inbox.has_buffered_for_lane_mask((1u8 << 0) | (1u8 << 2)));

        assert!(inbox.push_back(0, first));
        assert!(inbox.has_buffered_for_lane_mask(1u8 << 0));
        assert!(!inbox.has_buffered_for_lane_mask(1u8 << 2));

        assert!(inbox.push_back(2, second));
        assert!(inbox.has_buffered_for_lane_mask((1u8 << 0) | (1u8 << 2)));

        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert!(!inbox.has_buffered_for_lane_mask(1u8 << 0));
        assert!(inbox.has_buffered_for_lane_mask(1u8 << 2));

        assert_eq!(
            inbox.take_matching_or_poll(&mut binding, 2, second.label),
            Some(second)
        );
        assert!(!inbox.has_buffered_for_lane_mask(1u8 << 2));
    }

    #[test]
    fn binding_inbox_label_masks_track_buffered_labels_exactly() {
        let first = IncomingClassification {
            label: 3,
            instance: 1,
            has_fin: false,
            channel: Channel::new(11),
        };
        let second = IncomingClassification {
            label: 4,
            instance: 2,
            has_fin: false,
            channel: Channel::new(12),
        };
        let third = IncomingClassification {
            label: 7,
            instance: 3,
            has_fin: false,
            channel: Channel::new(13),
        };
        let mut binding = TestBinding::default();
        let mut inbox = BindingInbox::EMPTY;

        assert!(inbox.push_back(0, first));
        assert!(inbox.push_back(0, second));
        assert!(inbox.push_back(2, third));
        assert_eq!(
            inbox.label_masks[0],
            ScopeLabelMeta::label_bit(first.label) | ScopeLabelMeta::label_bit(second.label)
        );
        assert_eq!(inbox.label_masks[2], ScopeLabelMeta::label_bit(third.label));
        assert_eq!(
            inbox.buffered_label_lane_masks[first.label as usize],
            1u8 << 0
        );
        assert_eq!(
            inbox.buffered_label_lane_masks[second.label as usize],
            1u8 << 0
        );
        assert_eq!(
            inbox.buffered_label_lane_masks[third.label as usize],
            1u8 << 2
        );

        assert_eq!(
            inbox.take_matching_or_poll(&mut binding, 0, second.label),
            Some(second)
        );
        assert_eq!(inbox.label_masks[0], ScopeLabelMeta::label_bit(first.label));
        assert_eq!(inbox.buffered_label_lane_masks[second.label as usize], 0);
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_eq!(inbox.label_masks[0], 0);
        assert_eq!(inbox.buffered_label_lane_masks[first.label as usize], 0);
    }

    #[test]
    fn binding_inbox_take_matching_mask_drops_buffered_loop_control_labels() {
        let loop_control = IncomingClassification {
            label: LABEL_LOOP_CONTINUE,
            instance: 1,
            has_fin: false,
            channel: Channel::new(11),
        };
        let deferred = IncomingClassification {
            label: 33,
            instance: 2,
            has_fin: false,
            channel: Channel::new(12),
        };
        let expected = IncomingClassification {
            label: 55,
            instance: 3,
            has_fin: false,
            channel: Channel::new(13),
        };
        let mut binding = TestBinding::with_incoming(&[expected]);
        let mut inbox = BindingInbox::EMPTY;

        assert!(inbox.push_back(0, loop_control));
        assert!(inbox.push_back(0, deferred));

        let picked = inbox.take_matching_mask_or_poll(
            &mut binding,
            0,
            ScopeLabelMeta::label_bit(expected.label),
            ScopeLabelMeta::label_bit(LABEL_LOOP_CONTINUE)
                | ScopeLabelMeta::label_bit(LABEL_LOOP_BREAK),
            |label| matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK),
        );
        assert_eq!(picked, Some(expected));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(deferred));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), None);
    }

    #[test]
    fn binding_mismatch_scan_finds_later_matching_label() {
        let first = IncomingClassification {
            label: 11,
            instance: 1,
            has_fin: false,
            channel: Channel::new(21),
        };
        let second = IncomingClassification {
            label: 12,
            instance: 2,
            has_fin: false,
            channel: Channel::new(22),
        };
        let expected = IncomingClassification {
            label: 13,
            instance: 3,
            has_fin: false,
            channel: Channel::new(23),
        };
        let mut binding = TestBinding::with_incoming(&[first, second, expected]);
        let mut inbox = BindingInbox::EMPTY;

        let picked = inbox.take_matching_or_poll(&mut binding, 0, expected.label);
        assert_eq!(
            picked,
            Some(expected),
            "scan must continue past mismatched head entries"
        );
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(first));
        assert_eq!(inbox.take_or_poll(&mut binding, 0), Some(second));
    }

    #[test]
    fn stage_transport_payload_copies_bytes() {
        let mut scratch = [0u8; 8];
        let src = [1u8, 2, 3, 4];
        let len = stage_transport_payload(&mut scratch, &src).expect("stage payload");
        assert_eq!(len, src.len());
        assert_eq!(&scratch[..len], &src);
    }

    #[test]
    fn stage_transport_payload_rejects_oversize() {
        let mut scratch = [0u8; 2];
        let src = [1u8, 2, 3];
        let err = stage_transport_payload(&mut scratch, &src).expect_err("oversize");
        assert!(matches!(err, RecvError::PhaseInvariant));
    }

    #[test]
    fn offer_select_priority_is_deterministic() {
        assert_eq!(
            choose_offer_priority(true, 1, 1, 2),
            Some(OfferSelectPriority::CurrentOfferEntry)
        );
        assert_eq!(
            choose_offer_priority(false, 1, 2, 2),
            Some(OfferSelectPriority::DynamicControllerUnique)
        );
        assert_eq!(
            choose_offer_priority(false, 0, 1, 2),
            Some(OfferSelectPriority::ControllerUnique)
        );
        assert_eq!(
            choose_offer_priority(false, 0, 2, 1),
            Some(OfferSelectPriority::CandidateUnique)
        );
        assert_eq!(choose_offer_priority(false, 0, 2, 2), None);
    }

    #[test]
    fn static_controller_current_is_not_preempted() {
        let selected = choose_offer_priority(true, 1, 1, 2);
        assert_eq!(selected, Some(OfferSelectPriority::CurrentOfferEntry));
    }

    #[test]
    fn hint_filter_does_not_override_priority() {
        // Stage A applies filter; Stage B ordering is still fixed.
        let current_is_candidate_after_filter = true;
        let selected = choose_offer_priority(current_is_candidate_after_filter, 1, 1, 1);
        assert_eq!(selected, Some(OfferSelectPriority::CurrentOfferEntry));
    }

    #[test]
    fn offer_priority_has_no_liveness_override() {
        // Stage B priority is fixed and independent from liveness signals.
        assert_eq!(
            choose_offer_priority(false, 1, 1, 1),
            Some(OfferSelectPriority::DynamicControllerUnique)
        );
        assert_eq!(
            choose_offer_priority(false, 0, 1, 1),
            Some(OfferSelectPriority::ControllerUnique)
        );
    }

    #[test]
    fn current_scope_selection_meta_non_route_defaults_do_not_block_current() {
        let meta = CurrentScopeSelectionMeta::EMPTY;
        assert!(!meta.is_route_entry());
        assert!(meta.has_offer_lanes());
        assert!(!meta.is_controller());
    }

    #[test]
    fn current_scope_selection_meta_route_entry_flags_roundtrip() {
        let meta = CurrentScopeSelectionMeta {
            flags: CurrentScopeSelectionMeta::FLAG_ROUTE_ENTRY
                | CurrentScopeSelectionMeta::FLAG_HAS_OFFER_LANES
                | CurrentScopeSelectionMeta::FLAG_CONTROLLER,
        };
        assert!(meta.is_route_entry());
        assert!(meta.has_offer_lanes());
        assert!(meta.is_controller());
    }

    #[test]
    fn current_frontier_selection_state_loop_controller_without_evidence_is_exact() {
        let base = CurrentFrontierSelectionState {
            frontier: FrontierKind::Loop,
            parallel_root: ScopeId::none(),
            ready: true,
            has_progress_evidence: false,
            flags: CurrentFrontierSelectionState::FLAG_CONTROLLER,
        };
        assert!(base.loop_controller_without_evidence());
        assert!(
            !CurrentFrontierSelectionState {
                ready: false,
                ..base
            }
            .loop_controller_without_evidence()
        );
        assert!(
            !CurrentFrontierSelectionState {
                has_progress_evidence: true,
                ..base
            }
            .loop_controller_without_evidence()
        );
        assert!(
            !CurrentFrontierSelectionState { flags: 0, ..base }.loop_controller_without_evidence()
        );
    }

    #[test]
    fn current_frontier_selection_state_updates_only_current_candidate() {
        let mut state = CurrentFrontierSelectionState {
            frontier: FrontierKind::Parallel,
            parallel_root: ScopeId::generic(3),
            ready: false,
            has_progress_evidence: false,
            flags: 0,
        };
        state.observe_candidate(
            ScopeId::generic(11),
            7,
            FrontierCandidate {
                scope_id: ScopeId::generic(12),
                entry_idx: 9,
                parallel_root: ScopeId::generic(3),
                frontier: FrontierKind::Parallel,
                is_controller: false,
                is_dynamic: false,
                has_evidence: true,
                ready: true,
            },
        );
        assert!(!state.ready);
        assert!(!state.has_progress_evidence);

        state.observe_candidate(
            ScopeId::generic(11),
            7,
            FrontierCandidate {
                scope_id: ScopeId::generic(11),
                entry_idx: 7,
                parallel_root: ScopeId::generic(3),
                frontier: FrontierKind::Parallel,
                is_controller: false,
                is_dynamic: false,
                has_evidence: true,
                ready: true,
            },
        );
        assert!(state.ready);
        assert!(state.has_progress_evidence);
    }

    #[test]
    fn scope_loop_meta_recvless_ready_requires_active_or_linger() {
        assert!(!ScopeLoopMeta::EMPTY.recvless_ready());
        assert!(
            ScopeLoopMeta {
                flags: ScopeLoopMeta::FLAG_SCOPE_ACTIVE,
            }
            .recvless_ready()
        );
        assert!(
            ScopeLoopMeta {
                flags: ScopeLoopMeta::FLAG_SCOPE_LINGER | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
            }
            .recvless_ready()
        );
        assert!(
            !ScopeLoopMeta {
                flags: ScopeLoopMeta::FLAG_SCOPE_ACTIVE
                    | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV
                    | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
            }
            .recvless_ready()
        );
    }

    #[test]
    fn scope_loop_meta_loop_label_scope_and_arm_recv_bits_are_exact() {
        let meta = ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_CONTROL_SCOPE | ScopeLoopMeta::FLAG_BREAK_HAS_RECV,
        };
        assert!(meta.loop_label_scope());
        assert!(!meta.arm_has_recv(0));
        assert!(meta.arm_has_recv(1));

        let linger = ScopeLoopMeta {
            flags: ScopeLoopMeta::FLAG_SCOPE_LINGER | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV,
        };
        assert!(linger.loop_label_scope());
        assert!(linger.arm_has_recv(0));
        assert!(!linger.arm_has_recv(1));
        assert!(!ScopeLoopMeta::EMPTY.loop_label_scope());
    }

    #[test]
    fn scope_label_meta_current_recv_label_and_arm_bits_are_exact() {
        let no_arm = ScopeLabelMeta {
            recv_label: 7,
            recv_arm: 1,
            hint_label_mask: ScopeLabelMeta::label_bit(7),
            flags: ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL,
            ..ScopeLabelMeta::EMPTY
        };
        assert!(no_arm.matches_current_recv_label(7));
        assert!(no_arm.matches_hint_label(7));
        assert_eq!(no_arm.current_recv_arm_for_label(7), None);
        let with_arm = ScopeLabelMeta {
            arm_label_masks: [0, ScopeLabelMeta::label_bit(7)],
            flags: no_arm.flags | ScopeLabelMeta::FLAG_CURRENT_RECV_ARM,
            ..no_arm
        };
        assert_eq!(with_arm.current_recv_arm_for_label(7), Some(1));
        assert_eq!(with_arm.arm_for_label(7), Some(1));
        assert!(!with_arm.matches_current_recv_label(8));
    }

    #[test]
    fn scope_label_meta_controller_labels_map_to_binary_arms_exactly() {
        let meta = ScopeLabelMeta {
            controller_labels: [11, 13],
            hint_label_mask: ScopeLabelMeta::label_bit(11) | ScopeLabelMeta::label_bit(13),
            arm_label_masks: [ScopeLabelMeta::label_bit(11), ScopeLabelMeta::label_bit(13)],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(11),
                ScopeLabelMeta::label_bit(13),
            ],
            flags: ScopeLabelMeta::FLAG_CONTROLLER_ARM0 | ScopeLabelMeta::FLAG_CONTROLLER_ARM1,
            ..ScopeLabelMeta::EMPTY
        };
        assert_eq!(meta.controller_arm_for_label(11), Some(0));
        assert_eq!(meta.controller_arm_for_label(13), Some(1));
        assert_eq!(meta.controller_arm_for_label(17), None);
        assert_eq!(meta.arm_for_label(11), Some(0));
        assert_eq!(meta.arm_for_label(13), Some(1));
    }

    #[test]
    fn scope_label_meta_dispatch_labels_do_not_count_as_ready_evidence() {
        let mut meta = ScopeLabelMeta::EMPTY;
        meta.record_dispatch_arm_label(1, 29);

        assert!(meta.matches_hint_label(29));
        assert_eq!(meta.arm_for_label(29), Some(1));
        assert_eq!(meta.evidence_arm_for_label(29), None);
    }

    #[test]
    fn scope_label_meta_binding_evidence_can_be_stricter_than_hint_evidence() {
        let meta = ScopeLabelMeta {
            recv_label: 41,
            recv_arm: 0,
            hint_label_mask: ScopeLabelMeta::label_bit(41),
            arm_label_masks: [ScopeLabelMeta::label_bit(41), 0],
            evidence_arm_label_masks: [ScopeLabelMeta::label_bit(41), 0],
            flags: ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL
                | ScopeLabelMeta::FLAG_CURRENT_RECV_ARM
                | ScopeLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED,
            ..ScopeLabelMeta::EMPTY
        };

        assert!(meta.matches_hint_label(41));
        assert_eq!(meta.arm_for_label(41), Some(0));
        assert_eq!(meta.evidence_arm_for_label(41), Some(0));
        assert_eq!(meta.binding_evidence_arm_for_label(41), None);
    }

    #[test]
    fn scope_label_meta_preferred_binding_label_is_exact_only_for_singletons() {
        let meta = ScopeLabelMeta {
            recv_label: 41,
            recv_arm: 0,
            arm_label_masks: [
                ScopeLabelMeta::label_bit(41) | ScopeLabelMeta::label_bit(43),
                ScopeLabelMeta::label_bit(47),
            ],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(41) | ScopeLabelMeta::label_bit(43),
                ScopeLabelMeta::label_bit(47),
            ],
            flags: ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL
                | ScopeLabelMeta::FLAG_CURRENT_RECV_ARM
                | ScopeLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED,
            ..ScopeLabelMeta::EMPTY
        };

        assert_eq!(meta.preferred_binding_label(Some(0)), Some(43));
        assert_eq!(meta.preferred_binding_label(Some(1)), Some(47));
        assert_eq!(meta.preferred_binding_label(None), None);

        let singleton = ScopeLabelMeta {
            arm_label_masks: [ScopeLabelMeta::label_bit(53), 0],
            evidence_arm_label_masks: [ScopeLabelMeta::label_bit(53), 0],
            ..ScopeLabelMeta::EMPTY
        };
        assert_eq!(singleton.preferred_binding_label(None), Some(53));
    }

    #[test]
    fn scope_label_meta_preferred_binding_label_mask_respects_authoritative_arm() {
        let meta = ScopeLabelMeta {
            hint_label_mask: ScopeLabelMeta::label_bit(11)
                | ScopeLabelMeta::label_bit(13)
                | ScopeLabelMeta::label_bit(17),
            arm_label_masks: [
                ScopeLabelMeta::label_bit(11) | ScopeLabelMeta::label_bit(13),
                ScopeLabelMeta::label_bit(17),
            ],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(11) | ScopeLabelMeta::label_bit(13),
                ScopeLabelMeta::label_bit(17),
            ],
            ..ScopeLabelMeta::EMPTY
        };

        assert_eq!(
            meta.preferred_binding_label_mask(Some(0)),
            ScopeLabelMeta::label_bit(11) | ScopeLabelMeta::label_bit(13)
        );
        assert_eq!(
            meta.preferred_binding_label_mask(Some(1)),
            ScopeLabelMeta::label_bit(17)
        );
        assert_eq!(
            meta.preferred_binding_label_mask(None),
            meta.hint_label_mask
        );
    }

    #[test]
    fn scope_label_meta_preferred_binding_label_mask_keeps_current_recv_for_demux() {
        let meta = ScopeLabelMeta {
            recv_label: 41,
            recv_arm: 0,
            hint_label_mask: ScopeLabelMeta::label_bit(41) | ScopeLabelMeta::label_bit(43),
            arm_label_masks: [
                ScopeLabelMeta::label_bit(41) | ScopeLabelMeta::label_bit(43),
                ScopeLabelMeta::label_bit(47),
            ],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(43),
                ScopeLabelMeta::label_bit(47),
            ],
            flags: ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL
                | ScopeLabelMeta::FLAG_CURRENT_RECV_ARM
                | ScopeLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED,
            ..ScopeLabelMeta::EMPTY
        };

        assert_eq!(
            meta.preferred_binding_label_mask(Some(0)),
            ScopeLabelMeta::label_bit(41) | ScopeLabelMeta::label_bit(43)
        );
    }

    #[test]
    fn lane_offer_state_roundtrips_static_frontier_flags() {
        let state = LaneOfferState {
            scope: ScopeId::generic(5),
            entry: StateIndex::from_usize(11),
            parallel_root: ScopeId::generic(2),
            frontier: FrontierKind::Parallel,
            loop_meta: ScopeLoopMeta {
                flags: ScopeLoopMeta::FLAG_CONTROL_SCOPE | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV,
            },
            label_meta: ScopeLabelMeta {
                scope_id: ScopeId::generic(5),
                loop_meta: ScopeLoopMeta {
                    flags: ScopeLoopMeta::FLAG_CONTROL_SCOPE
                        | ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV,
                },
                recv_label: 23,
                recv_arm: 0,
                controller_labels: [31, 37],
                hint_label_mask: ScopeLabelMeta::label_bit(23)
                    | ScopeLabelMeta::label_bit(31)
                    | ScopeLabelMeta::label_bit(37),
                arm_label_masks: [
                    ScopeLabelMeta::label_bit(23) | ScopeLabelMeta::label_bit(31),
                    ScopeLabelMeta::label_bit(37),
                ],
                evidence_arm_label_masks: [
                    ScopeLabelMeta::label_bit(23) | ScopeLabelMeta::label_bit(31),
                    ScopeLabelMeta::label_bit(37),
                ],
                flags: ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL
                    | ScopeLabelMeta::FLAG_CURRENT_RECV_ARM
                    | ScopeLabelMeta::FLAG_CONTROLLER_ARM0
                    | ScopeLabelMeta::FLAG_CONTROLLER_ARM1,
            },
            static_ready: true,
            flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
        };
        assert!(state.is_controller());
        assert!(state.is_dynamic());
        assert!(state.static_ready());
        assert_eq!(state.frontier, FrontierKind::Parallel);
        assert!(state.loop_meta.control_scope());
        assert!(state.loop_meta.continue_has_recv());
        assert!(!state.loop_meta.break_has_recv());
        assert_eq!(state.label_meta.scope_id(), ScopeId::generic(5));
        assert_eq!(state.label_meta.current_recv_arm_for_label(23), Some(0));
        assert_eq!(state.label_meta.controller_arm_for_label(31), Some(0));
        assert_eq!(state.label_meta.controller_arm_for_label(37), Some(1));
        assert_eq!(state.label_meta.arm_for_label(23), Some(0));
        assert_eq!(state.label_meta.arm_for_label(31), Some(0));
        assert_eq!(state.label_meta.arm_for_label(37), Some(1));
    }

    #[test]
    fn refresh_lane_offer_state_caches_scope_label_meta() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(997);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        worker.refresh_lane_offer_state(0);
        let cached = worker.lane_offer_state[0].label_meta;
        let entry_idx = state_index_to_usize(worker.lane_offer_state[0].entry);
        let entry_state = worker.offer_entry_state[entry_idx];
        let recv_meta = worker.cursor.try_recv_meta().expect("recv metadata");
        assert_eq!(cached.scope_id(), scope);
        assert_eq!(
            cached.loop_meta().flags,
            worker.lane_offer_state[0].loop_meta.flags
        );
        assert!(cached.matches_current_recv_label(recv_meta.label));
        assert_eq!(
            cached.current_recv_arm_for_label(recv_meta.label),
            recv_meta.route_arm
        );
        assert_eq!(entry_state.scope_id, scope);
        assert_eq!(entry_state.frontier, worker.lane_offer_state[0].frontier);
        assert_eq!(entry_state.label_meta.scope_id(), scope);
        assert!(entry_state.selection_meta.is_route_entry());
        assert_eq!(
            entry_state.selection_meta.is_controller(),
            worker.lane_offer_state[0].is_controller()
        );
        assert_eq!(
            entry_state.summary.frontier_mask,
            worker.lane_offer_state[0].frontier.bit()
        );
        assert_eq!(
            entry_state.summary.is_controller(),
            worker.lane_offer_state[0].is_controller()
        );
        assert_eq!(
            entry_state.summary.is_dynamic(),
            worker.lane_offer_state[0].is_dynamic()
        );
        assert_eq!(
            entry_state.summary.static_ready(),
            worker.lane_offer_state[0].static_ready()
        );
        let observed = worker
            .recompute_offer_entry_observed_state_non_consuming(entry_idx)
            .expect("observed state");
        assert_eq!(worker.offer_entry_state[entry_idx].observed, observed);
        let (offer_lanes, offer_lanes_len) = worker.offer_lanes_for_scope(scope);
        let mut offer_lane_mask = 0u8;
        let mut offer_lane_idx = 0usize;
        while offer_lane_idx < offer_lanes_len {
            offer_lane_mask |= 1u8 << (offer_lanes[offer_lane_idx] as usize);
            offer_lane_idx += 1;
        }
        assert_eq!(entry_state.offer_lanes_len as usize, offer_lanes_len);
        assert_eq!(entry_state.offer_lanes, offer_lanes);
        assert_eq!(entry_state.offer_lane_mask, offer_lane_mask);
        assert_eq!(entry_state.lane_idx, 0);
        assert_eq!(
            worker
                .offer_entry_lane_state(scope, entry_idx)
                .map(|info| info.entry),
            Some(worker.lane_offer_state[0].entry)
        );
        let materialization = entry_state.materialization_meta;
        assert_eq!(
            materialization.arm_count,
            worker.cursor.route_scope_arm_count(scope).unwrap_or(0)
        );
        let mut arm = 0u8;
        while arm <= 1 {
            let expected_controller_recv = worker
                .cursor
                .controller_arm_entry_by_arm(scope, arm)
                .and_then(|(entry, _)| {
                    worker
                        .cursor
                        .with_index(state_index_to_usize(entry))
                        .try_recv_meta()
                })
                .is_some();
            let expected_controller_cross_role_recv = worker
                .cursor
                .controller_arm_entry_by_arm(scope, arm)
                .and_then(|(entry, _)| {
                    worker
                        .cursor
                        .with_index(state_index_to_usize(entry))
                        .try_recv_meta()
                })
                .map(|recv_meta| recv_meta.peer != 1)
                .unwrap_or(false);
            assert_eq!(
                materialization.controller_arm_entry(arm),
                worker.cursor.controller_arm_entry_by_arm(scope, arm)
            );
            assert_eq!(
                materialization.controller_arm_is_recv(arm),
                expected_controller_recv
            );
            assert_eq!(
                materialization.controller_arm_requires_ready_evidence(arm),
                expected_controller_cross_role_recv
            );
            assert_eq!(
                materialization.recv_entry(arm),
                worker
                    .cursor
                    .route_scope_arm_recv_index(scope, arm)
                    .map(StateIndex::from_usize)
            );
            assert_eq!(
                materialization.passive_arm_entry(arm),
                worker
                    .cursor
                    .follow_passive_observer_arm_for_scope(scope, arm)
                    .map(|nav| match nav {
                        PassiveArmNavigation::WithinArm { entry } => entry,
                    })
            );
            let mut expected_binding_demux_lane_mask = 0u8;
            if let Some((entry, _)) = worker.cursor.controller_arm_entry_by_arm(scope, arm)
                && let Some(recv_meta) = worker
                    .cursor
                    .with_index(state_index_to_usize(entry))
                    .try_recv_meta()
            {
                expected_binding_demux_lane_mask |= 1u8 << (recv_meta.lane as usize);
            }
            if let Some(entry) = worker.cursor.route_scope_arm_recv_index(scope, arm)
                && let Some(recv_meta) = worker.cursor.with_index(entry).try_recv_meta()
            {
                expected_binding_demux_lane_mask |= 1u8 << (recv_meta.lane as usize);
            }
            let mut dispatch_idx = 0usize;
            while let Some((_label, dispatch_arm, target)) = worker
                .cursor
                .route_scope_first_recv_dispatch_entry(scope, dispatch_idx)
            {
                if (dispatch_arm == arm || dispatch_arm == ARM_SHARED)
                    && let Some(recv_meta) = worker
                        .cursor
                        .with_index(state_index_to_usize(target))
                        .try_recv_meta()
                {
                    expected_binding_demux_lane_mask |= 1u8 << (recv_meta.lane as usize);
                }
                dispatch_idx += 1;
            }
            assert_eq!(
                materialization.binding_demux_lane_mask(Some(arm)),
                expected_binding_demux_lane_mask
            );
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        let mut dispatch_idx = 0usize;
        while let Some((label, arm, target)) = worker
            .cursor
            .route_scope_first_recv_dispatch_entry(scope, dispatch_idx)
        {
            assert_eq!(
                materialization.first_recv_target(label),
                Some((arm, target))
            );
            dispatch_idx += 1;
        }
        assert_eq!(materialization.first_recv_len as usize, dispatch_idx);

        drop(worker);
    }

    #[test]
    fn selection_materialization_helpers_match_reference_lookup_logic() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(999);
        let mut controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &ENTRY_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &ENTRY_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        controller.refresh_lane_offer_state(0);
        let controller_scope = controller.cursor.node_scope_id();
        let controller_selection = controller.select_scope().expect("controller selection");
        worker.refresh_lane_offer_state(0);
        let worker_scope = worker.cursor.node_scope_id();
        let worker_selection = worker.select_scope().expect("worker selection");

        let mut arm = 0u8;
        while arm <= 1 {
            assert_eq!(
                controller.selection_arm_has_recv(controller_selection, arm),
                controller.arm_has_recv(controller_scope, arm)
            );
            assert_eq!(
                controller.selection_arm_requires_materialization_ready_evidence(
                    controller_selection,
                    true,
                    arm,
                ),
                controller.arm_requires_materialization_ready_evidence(controller_scope, arm)
            );
            assert_eq!(
                worker.selection_arm_has_recv(worker_selection, arm),
                worker.arm_has_recv(worker_scope, arm)
            );
            assert_eq!(
                worker.selection_arm_requires_materialization_ready_evidence(
                    worker_selection,
                    false,
                    arm,
                ),
                if worker_selection.at_route_offer_entry
                    && worker_selection
                        .materialization_meta
                        .passive_arm_entry(arm)
                        .is_some()
                {
                    if worker_selection
                        .materialization_meta
                        .arm_has_first_recv_dispatch(arm)
                    {
                        !worker
                            .selection_arm_dispatch_materializes_without_ready_evidence(
                                worker_selection,
                                arm,
                            )
                    } else {
                        false
                    }
                } else {
                    worker.arm_requires_materialization_ready_evidence(worker_scope, arm)
                }
            );
            assert_eq!(
                controller.selection_non_wire_loop_control_recv(
                    controller_selection,
                    true,
                    arm,
                    LABEL_LOOP_CONTINUE,
                ),
                controller.is_non_wire_loop_control_recv(
                    controller_scope,
                    arm,
                    LABEL_LOOP_CONTINUE,
                )
            );
            assert_eq!(
                controller.selection_non_wire_loop_control_recv(
                    controller_selection,
                    true,
                    arm,
                    LABEL_LOOP_BREAK,
                ),
                controller.is_non_wire_loop_control_recv(controller_scope, arm, LABEL_LOOP_BREAK,)
            );
            assert_eq!(
                worker.selection_non_wire_loop_control_recv(
                    worker_selection,
                    false,
                    arm,
                    LABEL_LOOP_CONTINUE,
                ),
                worker.is_non_wire_loop_control_recv(worker_scope, arm, LABEL_LOOP_CONTINUE)
            );
            assert_eq!(
                worker.selection_non_wire_loop_control_recv(
                    worker_selection,
                    false,
                    arm,
                    LABEL_LOOP_BREAK,
                ),
                worker.is_non_wire_loop_control_recv(worker_scope, arm, LABEL_LOOP_BREAK)
            );
            if arm == 1 {
                break;
            }
            arm += 1;
        }

        drop(worker);
        drop(controller);
    }

    #[test]
    fn scope_arm_materialization_meta_caches_passive_recv_meta_exactly() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(998);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &ENTRY_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        worker.refresh_lane_offer_state(0);
        let offer_lane = worker.offer_lane_for_scope(scope);
        let passive_recv_meta = worker.compute_scope_passive_recv_meta(
            worker.offer_entry_state[state_index_to_usize(worker.lane_offer_state[0].entry)]
                .materialization_meta,
            scope,
            offer_lane,
        );
        let region = worker
            .cursor
            .scope_region_by_id(scope)
            .expect("scope region should exist");

        let mut arm = 0u8;
        while arm <= 1 {
            let expected = worker
                .cursor
                .follow_passive_observer_arm_for_scope(scope, arm)
                .map(|nav| match nav {
                    PassiveArmNavigation::WithinArm { entry } => entry,
                })
                .and_then(|entry| {
                    let target_cursor = worker.cursor.with_index(state_index_to_usize(entry));
                    if let Some(recv_meta) = target_cursor.try_recv_meta() {
                        return Some((target_cursor.index(), recv_meta));
                    }
                    if let Some(send_meta) = target_cursor.try_send_meta() {
                        return Some((
                            target_cursor.index(),
                            RecvMeta {
                                eff_index: send_meta.eff_index,
                                label: send_meta.label,
                                peer: send_meta.peer,
                                resource: send_meta.resource,
                                is_control: send_meta.is_control,
                                next: target_cursor.index(),
                                scope,
                                route_arm: Some(arm),
                                is_choice_determinant: false,
                                shot: send_meta.shot,
                                policy: send_meta.policy(),
                                lane: send_meta.lane,
                            },
                        ));
                    }
                    if target_cursor.is_jump() {
                        let scope_end = target_cursor.jump_target().unwrap_or(0);
                        let scope_end_cursor = worker.cursor.with_index(scope_end);
                        if region.linger {
                            let synthetic_label = match arm {
                                0 => LABEL_LOOP_CONTINUE,
                                1 => LABEL_LOOP_BREAK,
                                _ => return None,
                            };
                            return Some((
                                scope_end,
                                RecvMeta {
                                    eff_index: EffIndex::ZERO,
                                    label: synthetic_label,
                                    peer: 1,
                                    resource: None,
                                    is_control: true,
                                    next: scope_end,
                                    scope,
                                    route_arm: Some(arm),
                                    is_choice_determinant: false,
                                    shot: None,
                                    policy: PolicyMode::static_mode(),
                                    lane: offer_lane,
                                },
                            ));
                        }
                        if let Some(recv_meta) = scope_end_cursor.try_recv_meta() {
                            return Some((scope_end, recv_meta));
                        }
                        if let Some(send_meta) = scope_end_cursor.try_send_meta() {
                            return Some((
                                scope_end,
                                RecvMeta {
                                    eff_index: send_meta.eff_index,
                                    label: send_meta.label,
                                    peer: send_meta.peer,
                                    resource: send_meta.resource,
                                    is_control: send_meta.is_control,
                                    next: scope_end,
                                    scope,
                                    route_arm: Some(arm),
                                    is_choice_determinant: false,
                                    shot: send_meta.shot,
                                    policy: send_meta.policy(),
                                    lane: send_meta.lane,
                                },
                            ));
                        }
                        return None;
                    }
                    if region.linger {
                        let synthetic_label = match arm {
                            0 => LABEL_LOOP_CONTINUE,
                            1 => LABEL_LOOP_BREAK,
                            _ => return None,
                        };
                        return Some((
                            target_cursor.index(),
                            RecvMeta {
                                eff_index: EffIndex::ZERO,
                                label: synthetic_label,
                                peer: 1,
                                resource: None,
                                is_control: true,
                                next: target_cursor.index(),
                                scope,
                                route_arm: Some(arm),
                                is_choice_determinant: false,
                                shot: None,
                                policy: PolicyMode::static_mode(),
                                lane: offer_lane,
                            },
                        ));
                    }
                    None
                });
            let cached = passive_recv_meta
                .get(arm as usize)
                .copied()
                .and_then(CachedRecvMeta::recv_meta);
            assert_eq!(cached, expected);
            if arm == 1 {
                break;
            }
            arm += 1;
        }

        drop(worker);
    }

    #[test]
    fn align_cursor_to_selected_scope_skips_observation_for_single_active_entry() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(998);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        assert!(
            worker
                .active_frontier_entries(None)
                .contains_only(current_idx)
        );
        let observed_epoch = worker.global_frontier_observed_epoch;

        worker
            .align_cursor_to_selected_scope()
            .expect("single current entry should select directly");

        assert_eq!(worker.cursor.index(), current_idx);
        assert_eq!(
            worker.global_frontier_observed_epoch, observed_epoch,
            "single-active fast path must not rebuild observation during align"
        );

        drop(worker);
    }

    #[test]
    fn align_cursor_to_selected_scope_reuses_cached_multi_entry_observation() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(999);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let mut active_entries = ActiveEntrySet::EMPTY;
        assert!(active_entries.insert_entry(current_idx, 0));
        assert!(active_entries.insert_entry(fake_entry_idx, 1));
        worker.global_active_entries = active_entries;
        worker.global_frontier_observed =
            observed_entries_with_ready_current(current_idx, fake_entry_idx);
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 17;

        worker
            .align_cursor_to_selected_scope()
            .expect("fresh cached observation should be reused");

        assert_eq!(worker.cursor.index(), current_idx);
        assert_eq!(
            worker.frontier_observation_epoch, 17,
            "cache hit must not rebuild frontier observation"
        );

        drop(worker);
    }

    #[test]
    fn align_cursor_to_selected_scope_ignores_unrelated_lane_binding_changes() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1000);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let mut active_entries = ActiveEntrySet::EMPTY;
        assert!(active_entries.insert_entry(current_idx, 0));
        assert!(active_entries.insert_entry(fake_entry_idx, 1));
        worker.global_active_entries = active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();
        worker.global_frontier_observed =
            observed_entries_with_ready_current(current_idx, fake_entry_idx);
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 23;

        let unrelated = crate::binding::IncomingClassification {
            label: 91,
            channel: crate::binding::Channel::new(7),
            instance: 7,
            has_fin: false,
        };
        assert!(worker.binding_inbox.push_back(2, unrelated));

        worker
            .align_cursor_to_selected_scope()
            .expect("unrelated binding changes must not invalidate cached observation");

        assert_eq!(worker.cursor.index(), current_idx);
        assert_eq!(
            worker.frontier_observation_epoch, 23,
            "cache hit must survive unrelated-lane binding updates"
        );

        drop(worker);
    }

    #[test]
    fn align_cursor_to_selected_scope_ignores_relevant_lane_binding_content_changes() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1003);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let mut active_entries = ActiveEntrySet::EMPTY;
        assert!(active_entries.insert_entry(current_idx, 0));
        assert!(active_entries.insert_entry(fake_entry_idx, 1));
        worker.global_active_entries = active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();
        worker.global_frontier_observed =
            observed_entries_with_ready_current(current_idx, fake_entry_idx);

        let first = crate::binding::IncomingClassification {
            label: 31,
            channel: crate::binding::Channel::new(3),
            instance: 3,
            has_fin: false,
        };
        let second = crate::binding::IncomingClassification {
            label: 32,
            channel: crate::binding::Channel::new(4),
            instance: 4,
            has_fin: false,
        };
        assert!(worker.binding_inbox.push_back(0, first));
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 27;

        assert!(worker.binding_inbox.push_back(0, second));

        worker
            .align_cursor_to_selected_scope()
            .expect("relevant lane content-only changes must not invalidate cached observation");

        assert_eq!(worker.cursor.index(), current_idx);
        assert_eq!(
            worker.frontier_observation_epoch, 27,
            "cache hit must survive content-only updates on already-nonempty offer lanes"
        );

        drop(worker);
    }

    #[test]
    fn align_cursor_to_selected_scope_ignores_unrelated_scope_evidence_changes() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1001);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        if crate::eff::meta::MAX_EFF_NODES < 2 {
            drop(worker);
            return;
        }

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let mut active_entries = ActiveEntrySet::EMPTY;
        assert!(active_entries.insert_entry(current_idx, 0));
        assert!(active_entries.insert_entry(fake_entry_idx, 1));
        worker.global_active_entries = active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();
        worker.global_frontier_observed =
            observed_entries_with_ready_current(current_idx, fake_entry_idx);
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 29;

        let current_scope_slot = worker
            .scope_slot_for_route(worker.cursor.node_scope_id())
            .expect("current node scope should be a route scope");
        let unrelated_slot = if current_scope_slot == 0 { 1 } else { 0 };
        worker.scope_evidence[unrelated_slot].ready_arm_mask = ScopeEvidence::ARM0_READY;
        worker.bump_scope_evidence_generation(unrelated_slot);

        worker
            .align_cursor_to_selected_scope()
            .expect("unrelated scope evidence must not invalidate cached observation");

        assert_eq!(worker.cursor.index(), current_idx);
        assert_eq!(
            worker.frontier_observation_epoch, 29,
            "cache hit must survive unrelated-scope evidence updates"
        );

        drop(worker);
    }

    #[test]
    fn align_cursor_to_selected_scope_ignores_unrelated_lane_frontier_refresh() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1002);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        if MAX_LANES < 3 {
            drop(worker);
            return;
        }

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let mut active_entries = ActiveEntrySet::EMPTY;
        assert!(active_entries.insert_entry(current_idx, 0));
        assert!(active_entries.insert_entry(fake_entry_idx, 1));
        worker.global_active_entries = active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();
        worker.global_frontier_observed =
            observed_entries_with_ready_current(current_idx, fake_entry_idx);
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 31;

        worker.refresh_lane_offer_state(2);

        worker
            .align_cursor_to_selected_scope()
            .expect("unrelated lane frontier refresh must not invalidate cached observation");

        assert_eq!(worker.cursor.index(), current_idx);
        assert_eq!(
            worker.frontier_observation_epoch, 31,
            "cache hit must survive unrelated-lane frontier refresh"
        );

        drop(worker);
    }

    #[test]
    fn align_cursor_to_selected_scope_keeps_descended_nested_route_entry_authoritative() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let nested_program = g::route(HINT_ROUTE_PROGRAM, ENTRY_ROUTE_PROGRAM);
        let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
            project(&nested_program);
        let program_cursor = worker_program.phase_cursor();
        let nested_scope = program_cursor
            .seek_label(ENTRY_ARM0_SIGNAL_LABEL)
            .expect("nested route recv label must exist")
            .node_scope_id();
        let sid = SessionId::new(1004);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &worker_program, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let outer_scope = worker.cursor.node_scope_id();
        let outer_entry = worker.cursor.index();
        let nested_entry = worker
            .route_scope_offer_entry_index(nested_scope)
            .expect("nested route must have offer entry");

        assert_ne!(outer_entry, nested_entry);
        worker
            .set_route_arm(0, outer_scope, 1)
            .expect("select outer nested arm");
        worker
            .set_route_arm(0, nested_scope, 0)
            .expect("select nested arm");
        worker.set_cursor(worker.cursor.with_index(nested_entry));

        assert_eq!(
            worker.cursor.node_scope_id(),
            nested_scope,
            "cursor must already be positioned at the descended nested route",
        );
        assert_eq!(
            worker.current_offer_scope_id(),
            nested_scope,
            "selected nested route must become the current offer scope",
        );
        assert_eq!(
            worker.lane_offer_state[0].scope, outer_scope,
            "pre-align lane state intentionally still points at the ancestor route",
        );

        worker
            .align_cursor_to_selected_scope()
            .expect("selected nested route entry should remain authoritative");

        assert_eq!(
            worker.cursor.index(),
            nested_entry,
            "align must not bounce a selected nested route entry back to the ancestor scope",
        );
        assert_eq!(worker.current_offer_scope_id(), nested_scope);

        drop(worker);
    }

    #[test]
    fn active_entry_set_orders_entries_by_representative_lane() {
        let mut entries = ActiveEntrySet::EMPTY;
        assert!(entries.insert_entry(9, 4));
        assert!(entries.insert_entry(3, 1));
        assert!(entries.insert_entry(7, 1));
        assert_eq!(entries.entry_at(0), Some(3));
        assert_eq!(entries.entry_at(1), Some(7));
        assert_eq!(entries.entry_at(2), Some(9));

        assert!(entries.remove_entry(3));
        assert_eq!(entries.entry_at(0), Some(7));
        assert_eq!(entries.entry_at(1), Some(9));
        assert_eq!(entries.occupancy_mask(), 0b0000_0011);
    }

    #[test]
    fn current_passive_without_evidence_keeps_priority_with_controller_present() {
        assert!(!current_entry_is_candidate(false, false, false, 0, false,));
        assert!(current_entry_is_candidate(true, false, false, 1, false,));
    }

    #[test]
    fn current_passive_with_evidence_keeps_priority() {
        assert!(current_entry_is_candidate(true, false, true, 1, false,));
    }

    #[test]
    fn current_passive_without_controller_keeps_priority() {
        assert!(current_entry_is_candidate(true, false, false, 1, false,));
    }

    #[test]
    fn current_passive_observer_without_evidence_keeps_priority() {
        assert!(current_entry_is_candidate(true, false, false, 1, false,));
    }

    #[test]
    fn current_candidate_stays_selectable_without_route_lane_metadata() {
        assert!(current_entry_matches_after_filter(true, true, 43, None));
    }

    #[test]
    fn current_candidate_respects_hint_filter() {
        assert!(!current_entry_matches_after_filter(
            true,
            true,
            43,
            Some(47)
        ));
    }

    #[test]
    fn current_without_candidate_stays_blocked() {
        assert!(!current_entry_matches_after_filter(false, true, 43, None));
    }

    #[test]
    fn current_without_offer_lanes_stays_blocked() {
        assert!(!current_entry_matches_after_filter(true, false, 43, None));
    }

    #[test]
    fn offer_entry_observed_state_merges_static_summary_and_dynamic_evidence() {
        let mut summary = OfferEntryStaticSummary::EMPTY;
        summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Parallel,
            flags: LaneOfferState::FLAG_CONTROLLER,
            ..LaneOfferState::EMPTY
        });
        summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Parallel,
            static_ready: true,
            flags: LaneOfferState::FLAG_DYNAMIC,
            ..LaneOfferState::EMPTY
        });
        let observed = offer_entry_observed_state(ScopeId::generic(41), summary, true, false, true);

        assert_eq!(observed.scope_id, ScopeId::generic(41));
        assert!(observed.matches_frontier(FrontierKind::Parallel));
        assert!(observed.is_controller());
        assert!(observed.is_dynamic());
        assert!(observed.has_progress_evidence());
        assert!(observed.has_ready_arm_evidence());
        assert!(observed.binding_ready());
        assert_ne!(observed.flags & OfferEntryObservedState::FLAG_READY, 0);
    }

    #[test]
    fn cached_offer_entry_observed_state_preserves_arbitration_bits() {
        let mut summary = OfferEntryStaticSummary::EMPTY;
        summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::PassiveObserver,
            flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
            ..LaneOfferState::EMPTY
        });
        let observed = offer_entry_observed_state(ScopeId::generic(51), summary, true, false, true);
        let mut observed_entries = ObservedEntrySet::EMPTY;
        let (observed_bit, inserted) = observed_entries.insert_entry(17).expect("insert entry");
        assert!(inserted);
        observed_entries.observe(observed_bit, observed);

        let cached = cached_offer_entry_observed_state(
            ScopeId::generic(51),
            summary,
            observed_entries,
            observed_bit,
        );
        let original_candidate = offer_entry_frontier_candidate(
            17,
            ScopeId::generic(9),
            FrontierKind::PassiveObserver,
            observed,
        );
        let cached_candidate = offer_entry_frontier_candidate(
            17,
            ScopeId::generic(9),
            FrontierKind::PassiveObserver,
            cached,
        );

        assert!(cached.matches_frontier(FrontierKind::PassiveObserver));
        assert!(cached.is_controller());
        assert!(cached.is_dynamic());
        assert!(cached.has_progress_evidence());
        assert!(cached.has_ready_arm_evidence());
        assert!(cached.ready());
        assert_eq!(cached_candidate.scope_id, original_candidate.scope_id);
        assert_eq!(
            cached_candidate.parallel_root,
            original_candidate.parallel_root
        );
        assert_eq!(cached_candidate.frontier, original_candidate.frontier);
        assert_eq!(
            cached_candidate.is_controller,
            original_candidate.is_controller
        );
        assert_eq!(cached_candidate.is_dynamic, original_candidate.is_dynamic);
        assert_eq!(
            cached_candidate.has_evidence,
            original_candidate.has_evidence
        );
        assert_eq!(cached_candidate.ready, original_candidate.ready);
    }

    #[test]
    fn observed_entry_set_entry_bit_tracks_inserted_entries_exactly() {
        let mut observed_entries = ObservedEntrySet::EMPTY;
        let (first_bit, inserted_first) = observed_entries.insert_entry(17).expect("insert first");
        assert!(inserted_first);
        let (second_bit, inserted_second) =
            observed_entries.insert_entry(3).expect("insert second");
        assert!(inserted_second);
        let (reused_bit, inserted_reused) = observed_entries.insert_entry(17).expect("reuse first");
        assert!(!inserted_reused);
        assert_eq!(reused_bit, first_bit);
        assert_eq!(observed_entries.entry_bit(17), first_bit);
        assert_eq!(observed_entries.entry_bit(3), second_bit);
        assert_eq!(observed_entries.entry_bit(9), 0);
    }

    fn observed_entries_with_ready_current(
        current_idx: usize,
        fake_entry_idx: usize,
    ) -> ObservedEntrySet {
        let mut observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        let (fake_bit, inserted_fake) = observed_entries
            .insert_entry(fake_entry_idx)
            .expect("insert fake entry");
        assert!(inserted_fake);
        observed_entries.ready_mask = current_bit;
        observed_entries.route_mask = current_bit | fake_bit;
        observed_entries
    }

    fn observed_entries_with_route_entries(
        current_idx: usize,
        fake_entry_idx: usize,
    ) -> ObservedEntrySet {
        let mut observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        let (fake_bit, inserted_fake) = observed_entries
            .insert_entry(fake_entry_idx)
            .expect("insert fake entry");
        assert!(inserted_fake);
        observed_entries.route_mask = current_bit | fake_bit;
        observed_entries
    }

    #[test]
    fn rebuild_frontier_observed_entries_reuses_cached_entry_after_slot_shift() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1004);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let mut static_summary = OfferEntryStaticSummary::EMPTY;
        static_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Route,
            ..LaneOfferState::EMPTY
        });
        worker.offer_entry_state[current_idx].summary = static_summary;
        worker.offer_entry_state[current_idx].frontier = FrontierKind::Route;
        let current_state = worker.offer_entry_state[current_idx];
        let fake_state = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: 1,
            parallel_root: current_state.parallel_root,
            frontier: FrontierKind::Route,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: static_summary,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[fake_entry_idx] = fake_state;

        let mut cached_active_entries = ActiveEntrySet::EMPTY;
        assert!(cached_active_entries.insert_entry(current_idx, 0));
        assert!(cached_active_entries.insert_entry(fake_entry_idx, 1));
        worker.global_active_entries = cached_active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();
        let cached_key = worker.frontier_observation_key(ScopeId::none(), false);

        let mut cached_observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = cached_observed_entries
            .insert_entry(current_idx)
            .expect("insert current cached entry");
        assert!(inserted_current);
        cached_observed_entries.observe(
            current_bit,
            offer_entry_observed_state(
                current_state.scope_id,
                current_state.summary,
                false,
                false,
                true,
            ),
        );
        let (fake_bit, inserted_fake) = cached_observed_entries
            .insert_entry(fake_entry_idx)
            .expect("insert fake cached entry");
        assert!(inserted_fake);
        cached_observed_entries.observe(
            fake_bit,
            offer_entry_observed_state(
                fake_state.scope_id,
                fake_state.summary,
                false,
                false,
                false,
            ),
        );

        worker.offer_entry_state[current_idx].active_mask = 1u8 << 1;
        worker.offer_entry_state[current_idx].lane_idx = 1;
        worker.offer_entry_state[current_idx].offer_lane_mask = 1u8 << 1;
        worker.offer_entry_state[current_idx].offer_lanes = [1, 0, 0, 0, 0, 0, 0, 0];
        worker.offer_entry_state[current_idx].offer_lanes_len = 1;
        worker.offer_entry_state[fake_entry_idx].active_mask = 1u8 << 0;
        worker.offer_entry_state[fake_entry_idx].lane_idx = 0;
        worker.offer_entry_state[fake_entry_idx].offer_lane_mask = 1u8 << 0;
        worker.offer_entry_state[fake_entry_idx].offer_lanes = [0; MAX_LANES];
        worker.offer_entry_state[fake_entry_idx].offer_lanes_len = 1;

        let mut shifted_active_entries = ActiveEntrySet::EMPTY;
        assert!(shifted_active_entries.insert_entry(fake_entry_idx, 0));
        assert!(shifted_active_entries.insert_entry(current_idx, 1));
        worker.global_active_entries = shifted_active_entries;
        worker.recompute_global_offer_lane_entry_slot_masks();

        let observation_key = worker.frontier_observation_key(ScopeId::none(), false);
        let current_shifted_state = worker.offer_entry_state[current_idx];
        let cached_current = worker.cached_offer_entry_observed_state_for_rebuild(
            current_idx,
            current_shifted_state,
            observation_key,
            cached_key,
            cached_observed_entries,
        );
        assert!(
            cached_current.is_some(),
            "entry cache should survive slot shifts inside the active frontier"
        );

        let rebuilt = worker.refresh_frontier_observed_entries(
            ScopeId::none(),
            false,
            shifted_active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        );
        let current_shifted_bit = rebuilt.entry_bit(current_idx);
        assert_ne!(current_shifted_bit, 0);
        assert_eq!(current_shifted_bit, 1u8 << 1);
        assert_ne!(rebuilt.ready_mask & current_shifted_bit, 0);

        drop(worker);
    }

    #[test]
    fn refresh_frontier_observation_cache_prewarms_after_active_entry_replacement() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1012);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let old_entry_idx = current_idx + 1;
        let new_entry_idx = current_idx + 2;
        assert!(new_entry_idx < crate::global::typestate::MAX_STATES);

        let current_state = worker.offer_entry_state[current_idx];
        worker.offer_entry_state[old_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Route,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: OfferEntryStaticSummary::EMPTY,
            observed: OfferEntryObservedState::EMPTY,
        };

        let mut cached_active_entries = ActiveEntrySet::EMPTY;
        assert!(cached_active_entries.insert_entry(current_idx, 0));
        assert!(cached_active_entries.insert_entry(old_entry_idx, 0));
        worker.global_active_entries = cached_active_entries;
        worker.global_offer_lane_mask = 1u8 << 0;
        worker.recompute_global_offer_lane_entry_slot_masks();
        let mut cached_observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = cached_observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        cached_observed_entries.observe(
            current_bit,
            offer_entry_observed_state(
                current_state.scope_id,
                current_state.summary,
                false,
                false,
                true,
            ),
        );
        let (old_bit, inserted_old) = cached_observed_entries
            .insert_entry(old_entry_idx)
            .expect("insert old entry");
        assert!(inserted_old);
        cached_observed_entries.observe(
            old_bit,
            offer_entry_observed_state(
                current_state.scope_id,
                OfferEntryStaticSummary::EMPTY,
                false,
                false,
                false,
            ),
        );
        worker.global_frontier_observed = cached_observed_entries;
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 37;

        let mut ready_summary = OfferEntryStaticSummary::EMPTY;
        ready_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Loop,
            flags: LaneOfferState::FLAG_DYNAMIC,
            static_ready: true,
            ..LaneOfferState::EMPTY
        });
        worker.offer_entry_state[old_entry_idx] = OfferEntryState::EMPTY;
        worker.offer_entry_state[new_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Loop,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: ready_summary,
            observed: OfferEntryObservedState::EMPTY,
        };

        let mut replaced_active_entries = ActiveEntrySet::EMPTY;
        assert!(replaced_active_entries.insert_entry(current_idx, 0));
        assert!(replaced_active_entries.insert_entry(new_entry_idx, 0));
        worker.global_active_entries = replaced_active_entries;
        worker.global_offer_lane_mask = 1u8 << 0;
        worker.recompute_global_offer_lane_entry_slot_masks();

        let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
        assert!(
            worker
                .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
                .is_none(),
            "entry replacement should invalidate the previous cache key before warm-up",
        );

        worker.refresh_frontier_observation_cache(ScopeId::none(), false);

        assert!(
            worker.global_frontier_observed_key == updated_key,
            "frontier refresh should publish the replaced active-entry observation under the new key",
        );
        assert_eq!(worker.global_frontier_observed.entry_bit(old_entry_idx), 0);
        assert_eq!(
            worker.global_frontier_observed.entry_bit(new_entry_idx),
            1u8 << 1
        );
        assert_ne!(
            worker.global_frontier_observed.ready_mask
                & worker.global_frontier_observed.entry_bit(new_entry_idx),
            0,
        );
        assert_eq!(
            worker.global_frontier_observed.loop_mask
                & worker.global_frontier_observed.entry_bit(new_entry_idx),
            1u8 << 1,
        );
        assert!(
            worker.frontier_observation_epoch > 37,
            "prewarm should publish a fresh frontier observation epoch",
        );

        drop(worker);
    }

    #[test]
    fn patch_frontier_observed_entries_from_cached_structure_handles_cardinality_change() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1024);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let (
            worker_transport,
            worker_tap,
            worker_clock,
            worker_vm_caps,
            worker_loops,
            worker_routes,
            worker_host_slots,
            worker_scratch,
            worker_rv_id,
        ) = {
            let base_port = worker.ports[0]
                .as_ref()
                .expect("worker lane 0 port must exist")
                as *const crate::rendezvous::port::Port<'_, HintOnlyTransport, EpochTbl>;
            unsafe {
                (
                    (*base_port).transport() as *const HintOnlyTransport,
                    (*base_port).tap() as *const crate::observe::core::TapRing<'_>,
                    (*base_port).clock() as *const dyn crate::runtime::config::Clock,
                    (*base_port).vm_caps_table() as *const crate::rendezvous::tables::VmCapsTable,
                    (*base_port).loop_table() as *const crate::rendezvous::tables::LoopTable,
                    (*base_port).route_table() as *const crate::rendezvous::tables::RouteTable,
                    (*base_port).host_slots() as *const crate::epf::host::HostSlots<'_>,
                    (*base_port).scratch_ptr(),
                    (*base_port).rv_id(),
                )
            }
        };
        let worker_transport = unsafe { &*worker_transport };
        let worker_tap = unsafe { &*worker_tap };
        let worker_clock = unsafe { &*worker_clock };
        let worker_vm_caps = unsafe { &*worker_vm_caps };
        let worker_loops = unsafe { &*worker_loops };
        let worker_routes = unsafe { &*worker_routes };
        let worker_host_slots = unsafe { &*worker_host_slots };
        let (worker_tx1, worker_rx1) = worker_transport.open(1, worker.sid.raw());
        worker.ports[1] = Some(crate::rendezvous::port::Port::new(
            worker_transport,
            worker_tap,
            worker_clock,
            worker_vm_caps,
            worker_loops,
            worker_routes,
            worker_host_slots,
            worker_scratch,
            Lane::new(1),
            1,
            worker_rv_id,
            worker_tx1,
            worker_rx1,
        ));

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let middle_entry_idx = current_idx + 1;
        let third_entry_idx = current_idx + 2;
        let last_entry_idx = current_idx + 3;
        let new_loop_entry_idx = current_idx + 4;
        assert!(new_loop_entry_idx < crate::global::typestate::MAX_STATES);

        let current_state = worker.offer_entry_state[current_idx];
        let mut middle_summary = OfferEntryStaticSummary::EMPTY;
        middle_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Parallel,
            flags: LaneOfferState::FLAG_CONTROLLER,
            ..LaneOfferState::EMPTY
        });
        let mut third_summary = OfferEntryStaticSummary::EMPTY;
        third_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Loop,
            flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
            static_ready: true,
            ..LaneOfferState::EMPTY
        });
        let mut last_summary = OfferEntryStaticSummary::EMPTY;
        last_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::PassiveObserver,
            ..LaneOfferState::EMPTY
        });
        let mut new_loop_summary = OfferEntryStaticSummary::EMPTY;
        new_loop_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Loop,
            flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
            static_ready: true,
            ..LaneOfferState::EMPTY
        });

        worker.offer_entry_state[middle_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: 1,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Parallel,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: middle_summary,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[third_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Loop,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0; MAX_LANES],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: third_summary,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[last_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: 1,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::PassiveObserver,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: last_summary,
            observed: OfferEntryObservedState::EMPTY,
        };

        let mut cached_active_entries = ActiveEntrySet::EMPTY;
        assert!(cached_active_entries.insert_entry(current_idx, 0));
        assert!(cached_active_entries.insert_entry(middle_entry_idx, 1));
        assert!(cached_active_entries.insert_entry(third_entry_idx, 0));
        assert!(cached_active_entries.insert_entry(last_entry_idx, 1));
        worker.global_active_entries = cached_active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();
        let cached_key = worker.frontier_observation_key(ScopeId::none(), false);

        let mut cached_observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = cached_observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        cached_observed_entries.observe(
            current_bit,
            offer_entry_observed_state(
                current_state.scope_id,
                current_state.summary,
                false,
                false,
                true,
            ),
        );
        let (middle_bit, inserted_middle) = cached_observed_entries
            .insert_entry(middle_entry_idx)
            .expect("insert middle entry");
        assert!(inserted_middle);
        cached_observed_entries.observe(
            middle_bit,
            offer_entry_observed_state(current_state.scope_id, middle_summary, false, false, false),
        );
        let (third_bit, inserted_third) = cached_observed_entries
            .insert_entry(third_entry_idx)
            .expect("insert third entry");
        assert!(inserted_third);
        cached_observed_entries.observe(
            third_bit,
            offer_entry_observed_state(current_state.scope_id, third_summary, false, true, true),
        );
        let (last_bit, inserted_last) = cached_observed_entries
            .insert_entry(last_entry_idx)
            .expect("insert last entry");
        assert!(inserted_last);
        cached_observed_entries.observe(
            last_bit,
            offer_entry_observed_state(current_state.scope_id, last_summary, false, false, false),
        );

        worker.offer_entry_state[third_entry_idx] = OfferEntryState::EMPTY;
        worker.offer_entry_state[last_entry_idx] = OfferEntryState::EMPTY;
        worker.offer_entry_state[new_loop_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Loop,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0; MAX_LANES],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: new_loop_summary,
            observed: OfferEntryObservedState::EMPTY,
        };
        let mut active_entries = ActiveEntrySet::EMPTY;
        assert!(active_entries.insert_entry(current_idx, 0));
        assert!(active_entries.insert_entry(new_loop_entry_idx, 0));
        assert!(active_entries.insert_entry(middle_entry_idx, 1));
        worker.global_active_entries = active_entries;
        worker.recompute_global_offer_lane_entry_slot_masks();

        let observation_key = worker.frontier_observation_key(ScopeId::none(), false);
        let patched = worker
            .patch_frontier_observed_entries_from_cached_structure(
                active_entries,
                observation_key,
                cached_key,
                cached_observed_entries,
            )
            .expect("cardinality change should patch cached frontier observations");

        assert_eq!(patched.entry_bit(current_idx), 1u8 << 0);
        assert_eq!(patched.entry_bit(new_loop_entry_idx), 1u8 << 1);
        assert_eq!(patched.entry_bit(middle_entry_idx), 1u8 << 2);
        assert_eq!(patched.entry_bit(third_entry_idx), 0);
        assert_eq!(patched.entry_bit(last_entry_idx), 0);
        assert_ne!(patched.loop_mask & patched.entry_bit(new_loop_entry_idx), 0);
        assert_ne!(
            patched.parallel_mask & patched.entry_bit(middle_entry_idx),
            0
        );

        drop(worker);
    }

    #[test]
    fn refresh_frontier_observation_cache_prewarms_after_multi_entry_permutation() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1013);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let (
            worker_transport,
            worker_tap,
            worker_clock,
            worker_vm_caps,
            worker_loops,
            worker_routes,
            worker_host_slots,
            worker_scratch,
            worker_rv_id,
        ) = {
            let base_port = worker.ports[0]
                .as_ref()
                .expect("worker lane 0 port must exist")
                as *const crate::rendezvous::port::Port<'_, HintOnlyTransport, EpochTbl>;
            unsafe {
                (
                    (*base_port).transport() as *const HintOnlyTransport,
                    (*base_port).tap() as *const crate::observe::core::TapRing<'_>,
                    (*base_port).clock() as *const dyn crate::runtime::config::Clock,
                    (*base_port).vm_caps_table() as *const crate::rendezvous::tables::VmCapsTable,
                    (*base_port).loop_table() as *const crate::rendezvous::tables::LoopTable,
                    (*base_port).route_table() as *const crate::rendezvous::tables::RouteTable,
                    (*base_port).host_slots() as *const crate::epf::host::HostSlots<'_>,
                    (*base_port).scratch_ptr(),
                    (*base_port).rv_id(),
                )
            }
        };
        let worker_transport = unsafe { &*worker_transport };
        let worker_tap = unsafe { &*worker_tap };
        let worker_clock = unsafe { &*worker_clock };
        let worker_vm_caps = unsafe { &*worker_vm_caps };
        let worker_loops = unsafe { &*worker_loops };
        let worker_routes = unsafe { &*worker_routes };
        let worker_host_slots = unsafe { &*worker_host_slots };
        let (worker_tx1, worker_rx1) = worker_transport.open(1, worker.sid.raw());
        worker.ports[1] = Some(crate::rendezvous::port::Port::new(
            worker_transport,
            worker_tap,
            worker_clock,
            worker_vm_caps,
            worker_loops,
            worker_routes,
            worker_host_slots,
            worker_scratch,
            Lane::new(1),
            1,
            worker_rv_id,
            worker_tx1,
            worker_rx1,
        ));

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let middle_entry_idx = current_idx + 1;
        let third_entry_idx = current_idx + 2;
        let last_entry_idx = current_idx + 3;
        assert!(last_entry_idx < crate::global::typestate::MAX_STATES);

        let mut current_summary = OfferEntryStaticSummary::EMPTY;
        current_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Route,
            static_ready: true,
            ..LaneOfferState::EMPTY
        });
        worker.offer_entry_state[current_idx].summary = current_summary;
        worker.offer_entry_state[current_idx].frontier = FrontierKind::Route;
        worker.offer_entry_state[current_idx].active_mask = 1u8 << 0;
        worker.offer_entry_state[current_idx].lane_idx = 0;
        worker.offer_entry_state[current_idx].offer_lane_mask = 1u8 << 0;
        worker.offer_entry_state[current_idx].offer_lanes = [0; MAX_LANES];
        worker.offer_entry_state[current_idx].offer_lanes_len = 1;
        let current_state = worker.offer_entry_state[current_idx];
        let mut middle_summary = OfferEntryStaticSummary::EMPTY;
        middle_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Parallel,
            flags: LaneOfferState::FLAG_CONTROLLER,
            ..LaneOfferState::EMPTY
        });
        let mut third_summary = OfferEntryStaticSummary::EMPTY;
        third_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Loop,
            flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
            static_ready: true,
            ..LaneOfferState::EMPTY
        });
        let mut last_summary = OfferEntryStaticSummary::EMPTY;
        last_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::PassiveObserver,
            ..LaneOfferState::EMPTY
        });

        worker.offer_entry_state[middle_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: 1,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Parallel,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: middle_summary,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[third_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Loop,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0; MAX_LANES],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: third_summary,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[last_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: 1,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::PassiveObserver,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: last_summary,
            observed: OfferEntryObservedState::EMPTY,
        };

        let mut cached_active_entries = ActiveEntrySet::EMPTY;
        assert!(cached_active_entries.insert_entry(current_idx, 0));
        assert!(cached_active_entries.insert_entry(middle_entry_idx, 1));
        assert!(cached_active_entries.insert_entry(third_entry_idx, 0));
        assert!(cached_active_entries.insert_entry(last_entry_idx, 1));
        worker.global_active_entries = cached_active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();

        let mut cached_observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = cached_observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        cached_observed_entries.observe(
            current_bit,
            offer_entry_observed_state(current_state.scope_id, current_summary, false, false, true),
        );
        let (middle_bit, inserted_middle) = cached_observed_entries
            .insert_entry(middle_entry_idx)
            .expect("insert middle entry");
        assert!(inserted_middle);
        cached_observed_entries.observe(
            middle_bit,
            offer_entry_observed_state(current_state.scope_id, middle_summary, false, false, false),
        );
        let (third_bit, inserted_third) = cached_observed_entries
            .insert_entry(third_entry_idx)
            .expect("insert third entry");
        assert!(inserted_third);
        cached_observed_entries.observe(
            third_bit,
            offer_entry_observed_state(current_state.scope_id, third_summary, false, true, true),
        );
        let (last_bit, inserted_last) = cached_observed_entries
            .insert_entry(last_entry_idx)
            .expect("insert last entry");
        assert!(inserted_last);
        cached_observed_entries.observe(
            last_bit,
            offer_entry_observed_state(current_state.scope_id, last_summary, false, false, false),
        );
        worker.global_frontier_observed = cached_observed_entries;
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 41;

        worker.offer_entry_state[current_idx].active_mask = 1u8 << 1;
        worker.offer_entry_state[current_idx].lane_idx = 1;
        worker.offer_entry_state[current_idx].offer_lane_mask = 1u8 << 1;
        worker.offer_entry_state[current_idx].offer_lanes = [1, 0, 0, 0, 0, 0, 0, 0];
        worker.offer_entry_state[current_idx].offer_lanes_len = 1;
        worker.offer_entry_state[middle_entry_idx].active_mask = 1u8 << 0;
        worker.offer_entry_state[middle_entry_idx].lane_idx = 0;
        worker.offer_entry_state[middle_entry_idx].offer_lane_mask = 1u8 << 0;
        worker.offer_entry_state[middle_entry_idx].offer_lanes = [0; MAX_LANES];
        worker.offer_entry_state[middle_entry_idx].offer_lanes_len = 1;
        worker.offer_entry_state[third_entry_idx].active_mask = 1u8 << 1;
        worker.offer_entry_state[third_entry_idx].lane_idx = 1;
        worker.offer_entry_state[third_entry_idx].offer_lane_mask = 1u8 << 1;
        worker.offer_entry_state[third_entry_idx].offer_lanes = [1, 0, 0, 0, 0, 0, 0, 0];
        worker.offer_entry_state[third_entry_idx].offer_lanes_len = 1;
        worker.offer_entry_state[last_entry_idx].active_mask = 1u8 << 0;
        worker.offer_entry_state[last_entry_idx].lane_idx = 0;
        worker.offer_entry_state[last_entry_idx].offer_lane_mask = 1u8 << 0;
        worker.offer_entry_state[last_entry_idx].offer_lanes = [0; MAX_LANES];
        worker.offer_entry_state[last_entry_idx].offer_lanes_len = 1;

        let mut permuted_active_entries = ActiveEntrySet::EMPTY;
        assert!(permuted_active_entries.insert_entry(middle_entry_idx, 0));
        assert!(permuted_active_entries.insert_entry(third_entry_idx, 1));
        assert!(permuted_active_entries.insert_entry(last_entry_idx, 0));
        assert!(permuted_active_entries.insert_entry(current_idx, 1));
        worker.global_active_entries = permuted_active_entries;
        worker.recompute_global_offer_lane_entry_slot_masks();

        let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
        worker.refresh_frontier_observation_cache(ScopeId::none(), false);

        assert!(
            worker.global_frontier_observed_key == updated_key,
            "permutation prewarm should publish the permuted frontier observation under the new key",
        );
        assert_eq!(
            worker.global_frontier_observed.entry_bit(middle_entry_idx),
            1u8 << 0
        );
        assert_eq!(
            worker.global_frontier_observed.entry_bit(last_entry_idx),
            1u8 << 1
        );
        assert_eq!(
            worker.global_frontier_observed.entry_bit(current_idx),
            1u8 << 2
        );
        assert_eq!(
            worker.global_frontier_observed.entry_bit(third_entry_idx),
            1u8 << 3
        );
        assert_eq!(
            worker.global_frontier_observed.dynamic_controller_mask,
            1u8 << 3
        );
        assert_eq!(
            worker.global_frontier_observed.controller_mask,
            (1u8 << 0) | (1u8 << 3)
        );
        assert_eq!(worker.global_frontier_observed.progress_mask, 1u8 << 2);
        assert_eq!(
            worker.global_frontier_observed.ready_mask,
            (1u8 << 2) | (1u8 << 3)
        );
        assert_eq!(worker.global_frontier_observed.loop_mask, 1u8 << 3);
        assert_eq!(worker.global_frontier_observed.parallel_mask, 1u8 << 0);
        assert_eq!(
            worker.global_frontier_observed.passive_observer_mask,
            1u8 << 1
        );
        assert_eq!(worker.global_frontier_observed.route_mask, 1u8 << 2);
        assert!(
            worker.frontier_observation_epoch > 41,
            "permutation prewarm should publish a fresh frontier observation epoch",
        );

        drop(worker);
    }

    #[test]
    fn refresh_frontier_observation_cache_prewarms_after_multi_entry_replacement() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1014);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let (
            worker_transport,
            worker_tap,
            worker_clock,
            worker_vm_caps,
            worker_loops,
            worker_routes,
            worker_host_slots,
            worker_scratch,
            worker_rv_id,
        ) = {
            let base_port = worker.ports[0]
                .as_ref()
                .expect("worker lane 0 port must exist")
                as *const crate::rendezvous::port::Port<'_, HintOnlyTransport, EpochTbl>;
            unsafe {
                (
                    (*base_port).transport() as *const HintOnlyTransport,
                    (*base_port).tap() as *const crate::observe::core::TapRing<'_>,
                    (*base_port).clock() as *const dyn crate::runtime::config::Clock,
                    (*base_port).vm_caps_table() as *const crate::rendezvous::tables::VmCapsTable,
                    (*base_port).loop_table() as *const crate::rendezvous::tables::LoopTable,
                    (*base_port).route_table() as *const crate::rendezvous::tables::RouteTable,
                    (*base_port).host_slots() as *const crate::epf::host::HostSlots<'_>,
                    (*base_port).scratch_ptr(),
                    (*base_port).rv_id(),
                )
            }
        };
        let worker_transport = unsafe { &*worker_transport };
        let worker_tap = unsafe { &*worker_tap };
        let worker_clock = unsafe { &*worker_clock };
        let worker_vm_caps = unsafe { &*worker_vm_caps };
        let worker_loops = unsafe { &*worker_loops };
        let worker_routes = unsafe { &*worker_routes };
        let worker_host_slots = unsafe { &*worker_host_slots };
        let (worker_tx1, worker_rx1) = worker_transport.open(1, worker.sid.raw());
        worker.ports[1] = Some(crate::rendezvous::port::Port::new(
            worker_transport,
            worker_tap,
            worker_clock,
            worker_vm_caps,
            worker_loops,
            worker_routes,
            worker_host_slots,
            worker_scratch,
            Lane::new(1),
            1,
            worker_rv_id,
            worker_tx1,
            worker_rx1,
        ));

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let middle_entry_idx = current_idx + 1;
        let third_entry_idx = current_idx + 2;
        let last_entry_idx = current_idx + 3;
        let new_loop_entry_idx = current_idx + 4;
        let new_passive_entry_idx = current_idx + 5;
        assert!(new_passive_entry_idx < crate::global::typestate::MAX_STATES);

        let mut current_summary = OfferEntryStaticSummary::EMPTY;
        current_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Route,
            static_ready: true,
            ..LaneOfferState::EMPTY
        });
        worker.offer_entry_state[current_idx].summary = current_summary;
        worker.offer_entry_state[current_idx].frontier = FrontierKind::Route;
        worker.offer_entry_state[current_idx].active_mask = 1u8 << 0;
        worker.offer_entry_state[current_idx].lane_idx = 0;
        worker.offer_entry_state[current_idx].offer_lane_mask = 1u8 << 0;
        worker.offer_entry_state[current_idx].offer_lanes = [0; MAX_LANES];
        worker.offer_entry_state[current_idx].offer_lanes_len = 1;
        let current_state = worker.offer_entry_state[current_idx];

        let mut middle_summary = OfferEntryStaticSummary::EMPTY;
        middle_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Parallel,
            flags: LaneOfferState::FLAG_CONTROLLER,
            ..LaneOfferState::EMPTY
        });
        let mut third_summary = OfferEntryStaticSummary::EMPTY;
        third_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Loop,
            flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
            static_ready: true,
            ..LaneOfferState::EMPTY
        });
        let mut last_summary = OfferEntryStaticSummary::EMPTY;
        last_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::PassiveObserver,
            ..LaneOfferState::EMPTY
        });
        let mut new_loop_summary = OfferEntryStaticSummary::EMPTY;
        new_loop_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Loop,
            flags: LaneOfferState::FLAG_CONTROLLER | LaneOfferState::FLAG_DYNAMIC,
            static_ready: true,
            ..LaneOfferState::EMPTY
        });
        let mut new_passive_summary = OfferEntryStaticSummary::EMPTY;
        new_passive_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::PassiveObserver,
            ..LaneOfferState::EMPTY
        });

        worker.offer_entry_state[middle_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: 1,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Parallel,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: middle_summary,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[third_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Loop,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0; MAX_LANES],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: third_summary,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[last_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: 1,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::PassiveObserver,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: last_summary,
            observed: OfferEntryObservedState::EMPTY,
        };

        let mut cached_active_entries = ActiveEntrySet::EMPTY;
        assert!(cached_active_entries.insert_entry(current_idx, 0));
        assert!(cached_active_entries.insert_entry(middle_entry_idx, 1));
        assert!(cached_active_entries.insert_entry(third_entry_idx, 0));
        assert!(cached_active_entries.insert_entry(last_entry_idx, 1));
        worker.global_active_entries = cached_active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();

        let mut cached_observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = cached_observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        cached_observed_entries.observe(
            current_bit,
            offer_entry_observed_state(current_state.scope_id, current_summary, false, false, true),
        );
        let (middle_bit, inserted_middle) = cached_observed_entries
            .insert_entry(middle_entry_idx)
            .expect("insert middle entry");
        assert!(inserted_middle);
        cached_observed_entries.observe(
            middle_bit,
            offer_entry_observed_state(current_state.scope_id, middle_summary, false, false, false),
        );
        let (third_bit, inserted_third) = cached_observed_entries
            .insert_entry(third_entry_idx)
            .expect("insert third entry");
        assert!(inserted_third);
        cached_observed_entries.observe(
            third_bit,
            offer_entry_observed_state(current_state.scope_id, third_summary, false, true, true),
        );
        let (last_bit, inserted_last) = cached_observed_entries
            .insert_entry(last_entry_idx)
            .expect("insert last entry");
        assert!(inserted_last);
        cached_observed_entries.observe(
            last_bit,
            offer_entry_observed_state(current_state.scope_id, last_summary, false, false, false),
        );
        worker.global_frontier_observed = cached_observed_entries;
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 53;

        worker.offer_entry_state[third_entry_idx] = OfferEntryState::EMPTY;
        worker.offer_entry_state[last_entry_idx] = OfferEntryState::EMPTY;
        worker.offer_entry_state[new_loop_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Loop,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0; MAX_LANES],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: new_loop_summary,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[new_passive_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: 1,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::PassiveObserver,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: new_passive_summary,
            observed: OfferEntryObservedState::EMPTY,
        };

        let mut replaced_active_entries = ActiveEntrySet::EMPTY;
        assert!(replaced_active_entries.insert_entry(current_idx, 0));
        assert!(replaced_active_entries.insert_entry(middle_entry_idx, 1));
        assert!(replaced_active_entries.insert_entry(new_loop_entry_idx, 0));
        assert!(replaced_active_entries.insert_entry(new_passive_entry_idx, 1));
        worker.global_active_entries = replaced_active_entries;
        worker.recompute_global_offer_lane_entry_slot_masks();

        let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
        assert!(
            worker.refresh_structural_frontier_observation_cache(
                ScopeId::none(),
                false,
                worker.global_active_entries,
                worker.global_frontier_observed_key,
            ),
            "multi-entry replacement should patch the cached frontier observation without falling back to generic rebuild",
        );

        assert!(
            worker.global_frontier_observed_key == updated_key,
            "multi-entry replacement should publish the refreshed frontier observation under the new key",
        );
        assert_eq!(
            worker.global_frontier_observed.entry_bit(current_idx),
            1u8 << 0
        );
        assert_eq!(
            worker
                .global_frontier_observed
                .entry_bit(new_loop_entry_idx),
            1u8 << 1
        );
        assert_eq!(
            worker.global_frontier_observed.entry_bit(middle_entry_idx),
            1u8 << 2
        );
        assert_eq!(
            worker
                .global_frontier_observed
                .entry_bit(new_passive_entry_idx),
            1u8 << 3
        );
        assert_eq!(
            worker.global_frontier_observed.entry_bit(third_entry_idx),
            0
        );
        assert_eq!(worker.global_frontier_observed.entry_bit(last_entry_idx), 0);
        assert_eq!(
            worker.global_frontier_observed.dynamic_controller_mask,
            1u8 << 1
        );
        assert_eq!(
            worker.global_frontier_observed.controller_mask,
            (1u8 << 1) | (1u8 << 2)
        );
        assert_eq!(worker.global_frontier_observed.progress_mask, 1u8 << 0);
        assert_eq!(
            worker.global_frontier_observed.ready_mask,
            (1u8 << 0) | (1u8 << 1)
        );
        assert_eq!(worker.global_frontier_observed.loop_mask, 1u8 << 1);
        assert_eq!(worker.global_frontier_observed.parallel_mask, 1u8 << 2);
        assert_eq!(
            worker.global_frontier_observed.passive_observer_mask,
            1u8 << 3
        );
        assert_eq!(worker.global_frontier_observed.route_mask, 1u8 << 0);
        assert!(
            worker.frontier_observation_epoch > 53,
            "multi-entry replacement prewarm should publish a fresh frontier observation epoch",
        );

        drop(worker);
    }

    #[test]
    fn refresh_cached_frontier_observation_entry_updates_stable_slot_in_place() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1013);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let mut summary = worker.offer_entry_state[current_idx].summary;
        summary.flags &= !OfferEntryStaticSummary::FLAG_STATIC_READY;
        worker.offer_entry_state[current_idx].summary = summary;

        let mut observed_entries = ObservedEntrySet::EMPTY;
        let (observed_bit, inserted) = observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted);
        observed_entries.observe(
            observed_bit,
            offer_entry_observed_state(
                worker.offer_entry_state[current_idx].scope_id,
                summary,
                false,
                false,
                false,
            ),
        );
        worker.global_frontier_observed = observed_entries;
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 41;
        assert_eq!(worker.global_frontier_observed.ready_mask & observed_bit, 0);

        worker.offer_entry_state[current_idx].summary.flags |=
            OfferEntryStaticSummary::FLAG_STATIC_READY;
        let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
        assert!(
            worker
                .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
                .is_none(),
            "summary fingerprint change should invalidate the stale cached key before patching",
        );

        assert!(
            worker.refresh_cached_frontier_observation_entry(ScopeId::none(), false, current_idx),
            "stable active-entry slot should patch the cached frontier observation in place",
        );
        assert!(
            worker.global_frontier_observed_key == updated_key,
            "targeted patch should publish the refreshed observation under the new key",
        );
        let current_bit = worker.global_frontier_observed.entry_bit(current_idx);
        assert_ne!(current_bit, 0);
        assert_ne!(
            worker.global_frontier_observed.ready_mask & current_bit,
            0,
            "patched observation should reflect the updated static ready bit",
        );
        assert!(
            worker.frontier_observation_epoch > 41,
            "targeted patch should publish a fresh frontier observation epoch",
        );

        drop(worker);
    }

    #[test]
    fn observed_entry_set_move_entry_slot_remaps_masks_exactly() {
        let current_idx = 17usize;
        let fake_entry_idx = 23usize;
        let mut observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        observed_entries.observe(
            current_bit,
            OfferEntryObservedState {
                scope_id: ScopeId::generic(7),
                frontier_mask: FrontierKind::Route.bit(),
                flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
            },
        );
        let (fake_bit, inserted_fake) = observed_entries
            .insert_entry(fake_entry_idx)
            .expect("insert fake entry");
        assert!(inserted_fake);
        observed_entries.observe(
            fake_bit,
            OfferEntryObservedState {
                scope_id: ScopeId::generic(8),
                frontier_mask: FrontierKind::Parallel.bit(),
                flags: OfferEntryObservedState::FLAG_CONTROLLER,
            },
        );

        assert!(observed_entries.move_entry_slot(fake_entry_idx, 0));
        assert_eq!(observed_entries.entry_bit(fake_entry_idx), 1u8 << 0);
        assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 1);
        assert_eq!(observed_entries.controller_mask, 1u8 << 0);
        assert_eq!(observed_entries.progress_mask, 1u8 << 1);
        assert_eq!(observed_entries.ready_mask, 1u8 << 1);
        assert_eq!(observed_entries.parallel_mask, 1u8 << 0);
        assert_eq!(observed_entries.route_mask, 1u8 << 1);
    }

    #[test]
    fn observed_entry_set_insert_observation_at_slot_remaps_masks_exactly() {
        let current_idx = 17usize;
        let fake_entry_idx = 23usize;
        let mut observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        observed_entries.observe(
            current_bit,
            OfferEntryObservedState {
                scope_id: ScopeId::generic(7),
                frontier_mask: FrontierKind::Route.bit(),
                flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
            },
        );

        assert!(observed_entries.insert_observation_at_slot(
            fake_entry_idx,
            0,
            OfferEntryObservedState {
                scope_id: ScopeId::generic(8),
                frontier_mask: FrontierKind::Parallel.bit(),
                flags: OfferEntryObservedState::FLAG_CONTROLLER,
            },
        ));
        assert_eq!(observed_entries.entry_bit(fake_entry_idx), 1u8 << 0);
        assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 1);
        assert_eq!(observed_entries.controller_mask, 1u8 << 0);
        assert_eq!(observed_entries.progress_mask, 1u8 << 1);
        assert_eq!(observed_entries.ready_mask, 1u8 << 1);
        assert_eq!(observed_entries.parallel_mask, 1u8 << 0);
        assert_eq!(observed_entries.route_mask, 1u8 << 1);
    }

    #[test]
    fn observed_entry_set_remove_observation_remaps_masks_exactly() {
        let current_idx = 17usize;
        let fake_entry_idx = 23usize;
        let mut observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        observed_entries.observe(
            current_bit,
            OfferEntryObservedState {
                scope_id: ScopeId::generic(7),
                frontier_mask: FrontierKind::Route.bit(),
                flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
            },
        );
        assert!(observed_entries.insert_observation_at_slot(
            fake_entry_idx,
            0,
            OfferEntryObservedState {
                scope_id: ScopeId::generic(8),
                frontier_mask: FrontierKind::Parallel.bit(),
                flags: OfferEntryObservedState::FLAG_CONTROLLER,
            },
        ));

        assert!(observed_entries.remove_observation(fake_entry_idx));
        assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 0);
        assert_eq!(observed_entries.entry_bit(fake_entry_idx), 0);
        assert_eq!(observed_entries.controller_mask, 0);
        assert_eq!(observed_entries.progress_mask, 1u8 << 0);
        assert_eq!(observed_entries.ready_mask, 1u8 << 0);
        assert_eq!(observed_entries.parallel_mask, 0);
        assert_eq!(observed_entries.route_mask, 1u8 << 0);
    }

    #[test]
    fn observed_entry_set_replace_entry_at_slot_remaps_masks_exactly() {
        let current_idx = 17usize;
        let old_entry_idx = 23usize;
        let new_entry_idx = 29usize;
        let mut observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        observed_entries.observe(
            current_bit,
            OfferEntryObservedState {
                scope_id: ScopeId::generic(7),
                frontier_mask: FrontierKind::Route.bit(),
                flags: OfferEntryObservedState::FLAG_READY | OfferEntryObservedState::FLAG_PROGRESS,
            },
        );
        assert!(observed_entries.insert_observation_at_slot(
            old_entry_idx,
            1,
            OfferEntryObservedState {
                scope_id: ScopeId::generic(8),
                frontier_mask: FrontierKind::Parallel.bit(),
                flags: OfferEntryObservedState::FLAG_CONTROLLER,
            },
        ));

        assert!(observed_entries.replace_entry_at_slot(
            old_entry_idx,
            new_entry_idx,
            OfferEntryObservedState {
                scope_id: ScopeId::generic(9),
                frontier_mask: FrontierKind::Loop.bit(),
                flags: OfferEntryObservedState::FLAG_READY_ARM
                    | OfferEntryObservedState::FLAG_DYNAMIC,
            },
        ));
        assert_eq!(observed_entries.entry_bit(current_idx), 1u8 << 0);
        assert_eq!(observed_entries.entry_bit(old_entry_idx), 0);
        assert_eq!(observed_entries.entry_bit(new_entry_idx), 1u8 << 1);
        assert_eq!(observed_entries.controller_mask, 0);
        assert_eq!(observed_entries.dynamic_controller_mask, 1u8 << 1);
        assert_eq!(observed_entries.progress_mask, 1u8 << 0);
        assert_eq!(observed_entries.ready_arm_mask, 1u8 << 1);
        assert_eq!(observed_entries.ready_mask, 1u8 << 0);
        assert_eq!(observed_entries.parallel_mask, 0);
        assert_eq!(observed_entries.loop_mask, 1u8 << 1);
        assert_eq!(observed_entries.route_mask, 1u8 << 0);
    }

    #[test]
    fn frontier_observation_structural_entry_detection_is_exact() {
        let mut cached_entries = ActiveEntrySet::EMPTY;
        assert!(cached_entries.insert_entry(11, 0));
        assert!(cached_entries.insert_entry(17, 0));

        let mut inserted_entries = cached_entries;
        assert!(inserted_entries.insert_entry(23, 0));
        assert_eq!(
            CursorEndpoint::<1, HintOnlyTransport, DefaultLabelUniverse, CounterClock, EpochTbl, 4>::
                structural_inserted_entry_idx(inserted_entries, cached_entries.entries),
            Some(23)
        );
        assert_eq!(
            CursorEndpoint::<1, HintOnlyTransport, DefaultLabelUniverse, CounterClock, EpochTbl, 4>::
                structural_removed_entry_idx(cached_entries, inserted_entries.entries),
            Some(23)
        );

        let mut replaced_entries = ActiveEntrySet::EMPTY;
        assert!(replaced_entries.insert_entry(11, 0));
        assert!(replaced_entries.insert_entry(19, 0));
        assert_eq!(
            CursorEndpoint::<1, HintOnlyTransport, DefaultLabelUniverse, CounterClock, EpochTbl, 4>::
                structural_replaced_entry_idx(replaced_entries, cached_entries.entries),
            Some(19)
        );

        let mut shifted_entries = ActiveEntrySet::EMPTY;
        assert!(shifted_entries.insert_entry(17, 0));
        assert!(shifted_entries.insert_entry(11, 1));
        let mut shifted_cached_entries = ActiveEntrySet::EMPTY;
        assert!(shifted_cached_entries.insert_entry(11, 0));
        assert!(shifted_cached_entries.insert_entry(17, 1));
        assert_eq!(
            CursorEndpoint::<1, HintOnlyTransport, DefaultLabelUniverse, CounterClock, EpochTbl, 4>::
                structural_shifted_entry_idx(shifted_entries, shifted_cached_entries.entries),
            Some(17)
        );
    }

    #[test]
    fn refresh_inserted_frontier_observation_entry_updates_cache_in_place() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1015);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let current_state = worker.offer_entry_state[current_idx];
        let mut current_observed = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = current_observed
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        current_observed.observe(
            current_bit,
            offer_entry_observed_state(
                current_state.scope_id,
                current_state.summary,
                false,
                false,
                true,
            ),
        );
        worker.global_active_entries = ActiveEntrySet::EMPTY;
        assert!(worker.global_active_entries.insert_entry(current_idx, 0));
        worker.global_offer_lane_mask = 1u8 << 0;
        worker.recompute_global_offer_lane_entry_slot_masks();
        worker.global_frontier_observed = current_observed;
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 59;

        worker.offer_entry_state[fake_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: current_state.parallel_root,
            frontier: current_state.frontier,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: OfferEntryStaticSummary::EMPTY,
            ..OfferEntryState::EMPTY
        };
        assert!(worker.global_active_entries.insert_entry(fake_entry_idx, 0));
        worker.recompute_global_offer_lane_entry_slot_masks();

        let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
        assert!(
            worker
                .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
                .is_none(),
            "entry insertion should invalidate the previous cache key before patching",
        );
        assert!(
            worker.refresh_inserted_frontier_observation_entry(
                ScopeId::none(),
                false,
                fake_entry_idx
            ),
            "single entry insertion should patch the cached frontier observation in place",
        );
        assert!(
            worker.global_frontier_observed_key == updated_key,
            "insert patch should publish the refreshed observation under the new key",
        );
        assert_eq!(
            worker.global_frontier_observed.entry_bit(current_idx),
            1u8 << 0
        );
        assert_eq!(
            worker.global_frontier_observed.entry_bit(fake_entry_idx),
            1u8 << 1
        );
        assert_ne!(
            worker.global_frontier_observed.ready_mask
                & worker.global_frontier_observed.entry_bit(current_idx),
            0,
            "existing current observation should survive entry insertion",
        );
        assert!(
            worker.frontier_observation_epoch > 59,
            "insert patch should publish a fresh frontier observation epoch",
        );

        drop(worker);
    }

    #[test]
    fn refresh_replaced_frontier_observation_entry_updates_cache_in_place() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1017);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let old_entry_idx = current_idx + 1;
        let new_entry_idx = current_idx + 2;
        assert!(new_entry_idx < crate::global::typestate::MAX_STATES);

        let current_state = worker.offer_entry_state[current_idx];
        let old_state = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: current_state.parallel_root,
            frontier: current_state.frontier,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: OfferEntryStaticSummary::EMPTY,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[old_entry_idx] = old_state;

        let mut cached_active_entries = ActiveEntrySet::EMPTY;
        assert!(cached_active_entries.insert_entry(current_idx, 0));
        assert!(cached_active_entries.insert_entry(old_entry_idx, 0));
        worker.global_active_entries = cached_active_entries;
        worker.global_offer_lane_mask = 1u8 << 0;
        worker.recompute_global_offer_lane_entry_slot_masks();

        let mut cached_observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = cached_observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        cached_observed_entries.observe(
            current_bit,
            offer_entry_observed_state(
                current_state.scope_id,
                current_state.summary,
                false,
                false,
                true,
            ),
        );
        let (old_bit, inserted_old) = cached_observed_entries
            .insert_entry(old_entry_idx)
            .expect("insert old entry");
        assert!(inserted_old);
        cached_observed_entries.observe(
            old_bit,
            offer_entry_observed_state(old_state.scope_id, old_state.summary, false, false, false),
        );
        worker.global_frontier_observed = cached_observed_entries;
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 67;

        let mut ready_summary = OfferEntryStaticSummary::EMPTY;
        ready_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Loop,
            flags: LaneOfferState::FLAG_DYNAMIC,
            static_ready: true,
            ..LaneOfferState::EMPTY
        });
        worker.offer_entry_state[old_entry_idx] = OfferEntryState::EMPTY;
        worker.offer_entry_state[new_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: current_state.parallel_root,
            frontier: FrontierKind::Loop,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: ready_summary,
            observed: OfferEntryObservedState::EMPTY,
        };
        let mut replaced_active_entries = ActiveEntrySet::EMPTY;
        assert!(replaced_active_entries.insert_entry(current_idx, 0));
        assert!(replaced_active_entries.insert_entry(new_entry_idx, 0));
        worker.global_active_entries = replaced_active_entries;
        worker.global_offer_lane_mask = 1u8 << 0;
        worker.recompute_global_offer_lane_entry_slot_masks();

        let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
        assert!(
            worker
                .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
                .is_none(),
            "entry replacement should invalidate the previous cache key before patching",
        );
        assert!(
            worker.refresh_replaced_frontier_observation_entry(
                ScopeId::none(),
                false,
                new_entry_idx
            ),
            "single slot replacement should patch the cached frontier observation in place",
        );
        assert!(
            worker.global_frontier_observed_key == updated_key,
            "replace patch should publish the refreshed observation under the new key",
        );
        assert_eq!(worker.global_frontier_observed.entry_bit(old_entry_idx), 0);
        assert_eq!(
            worker.global_frontier_observed.entry_bit(new_entry_idx),
            1u8 << 1
        );
        assert_ne!(
            worker.global_frontier_observed.ready_mask
                & worker.global_frontier_observed.entry_bit(new_entry_idx),
            0,
            "replacement observation should reflect the new entry readiness",
        );
        assert_eq!(
            worker.global_frontier_observed.loop_mask
                & worker.global_frontier_observed.entry_bit(new_entry_idx),
            1u8 << 1,
            "replacement observation should publish the new frontier bit",
        );
        assert!(
            worker.frontier_observation_epoch > 67,
            "replace patch should publish a fresh frontier observation epoch",
        );

        drop(worker);
    }

    #[test]
    fn refresh_removed_frontier_observation_entry_updates_cache_in_place() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1016);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let current_state = worker.offer_entry_state[current_idx];
        let fake_state = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: 1,
            parallel_root: current_state.parallel_root,
            frontier: current_state.frontier,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: OfferEntryStaticSummary::EMPTY,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[fake_entry_idx] = fake_state;

        let mut cached_active_entries = ActiveEntrySet::EMPTY;
        assert!(cached_active_entries.insert_entry(current_idx, 0));
        assert!(cached_active_entries.insert_entry(fake_entry_idx, 1));
        worker.global_active_entries = cached_active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();

        let mut cached_observed_entries = ObservedEntrySet::EMPTY;
        let (current_bit, inserted_current) = cached_observed_entries
            .insert_entry(current_idx)
            .expect("insert current entry");
        assert!(inserted_current);
        cached_observed_entries.observe(
            current_bit,
            offer_entry_observed_state(
                current_state.scope_id,
                current_state.summary,
                false,
                false,
                true,
            ),
        );
        let (fake_bit, inserted_fake) = cached_observed_entries
            .insert_entry(fake_entry_idx)
            .expect("insert fake entry");
        assert!(inserted_fake);
        cached_observed_entries.observe(
            fake_bit,
            offer_entry_observed_state(
                fake_state.scope_id,
                fake_state.summary,
                false,
                false,
                false,
            ),
        );
        worker.global_frontier_observed = cached_observed_entries;
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 61;

        worker.offer_entry_state[fake_entry_idx] = OfferEntryState::EMPTY;
        worker.global_active_entries.remove_entry(fake_entry_idx);
        worker.global_offer_lane_mask = 1u8 << 0;
        worker.recompute_global_offer_lane_entry_slot_masks();

        let updated_key = worker.frontier_observation_key(ScopeId::none(), false);
        assert!(
            worker
                .cached_frontier_observed_entries(ScopeId::none(), false, updated_key)
                .is_none(),
            "entry removal should invalidate the previous cache key before patching",
        );
        assert!(
            worker.refresh_removed_frontier_observation_entry(
                ScopeId::none(),
                false,
                fake_entry_idx
            ),
            "single entry removal should patch the cached frontier observation in place",
        );
        assert!(
            worker.global_frontier_observed_key == updated_key,
            "remove patch should publish the refreshed observation under the new key",
        );
        assert_eq!(
            worker.global_frontier_observed.entry_bit(current_idx),
            1u8 << 0
        );
        assert_eq!(worker.global_frontier_observed.entry_bit(fake_entry_idx), 0);
        assert_ne!(
            worker.global_frontier_observed.ready_mask
                & worker.global_frontier_observed.entry_bit(current_idx),
            0,
            "current observation should survive entry removal",
        );
        assert!(
            worker.frontier_observation_epoch > 61,
            "remove patch should publish a fresh frontier observation epoch",
        );

        drop(worker);
    }

    #[test]
    fn scope_evidence_change_prewarms_relevant_frontier_observation_cache() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1013);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let current_scope = worker.cursor.node_scope_id();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let current_state = worker.offer_entry_state[current_idx];
        let mut static_summary = OfferEntryStaticSummary::EMPTY;
        static_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Route,
            ..LaneOfferState::EMPTY
        });
        worker.offer_entry_state[current_idx].summary = static_summary;
        worker.offer_entry_state[current_idx].frontier = FrontierKind::Route;
        worker.offer_entry_state[fake_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 0,
            lane_idx: 0,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Route,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 0,
            offer_lanes: [0, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: static_summary,
            observed: OfferEntryObservedState::EMPTY,
        };

        let mut active_entries = ActiveEntrySet::EMPTY;
        assert!(active_entries.insert_entry(current_idx, 0));
        assert!(active_entries.insert_entry(fake_entry_idx, 1));
        worker.global_active_entries = active_entries;
        worker.global_offer_lane_mask = 1u8 << 0;
        worker.recompute_global_offer_lane_entry_slot_masks();
        worker.global_frontier_observed =
            observed_entries_with_route_entries(current_idx, fake_entry_idx);
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 41;

        worker.mark_scope_ready_arm(current_scope, 0);

        let warmed_key = worker.frontier_observation_key(ScopeId::none(), false);
        assert!(
            worker
                .cached_frontier_observed_entries(ScopeId::none(), false, warmed_key)
                .is_some(),
            "scope evidence update should prewarm the relevant cached observation",
        );
        let current_bit = worker.global_frontier_observed.entry_bit(current_idx);
        assert_ne!(current_bit, 0);
        assert_ne!(
            worker.global_frontier_observed.ready_arm_mask & current_bit,
            0,
            "ready-arm evidence should update the cached observation for the changed scope",
        );
        assert_ne!(
            worker.global_frontier_observed.progress_mask & current_bit,
            0,
            "ready-arm evidence should also publish progress evidence in the cached observation",
        );
        assert!(
            worker.frontier_observation_epoch > 41,
            "targeted cache refresh should publish a new frontier observation epoch",
        );

        let warmed_epoch = worker.frontier_observation_epoch;
        worker
            .align_cursor_to_selected_scope()
            .expect("prewarmed scope evidence should keep align on the cached observation path");
        assert_eq!(
            worker.frontier_observation_epoch, warmed_epoch,
            "align should hit the warmed cache instead of rebuilding the frontier observation",
        );

        drop(worker);
    }

    #[test]
    fn binding_inbox_change_prewarms_relevant_frontier_observation_cache() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1014);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let current_state = worker.offer_entry_state[current_idx];
        let mut static_summary = OfferEntryStaticSummary::EMPTY;
        static_summary.observe_lane(LaneOfferState {
            frontier: FrontierKind::Route,
            ..LaneOfferState::EMPTY
        });
        worker.offer_entry_state[current_idx].summary = static_summary;
        worker.offer_entry_state[current_idx].frontier = FrontierKind::Route;
        worker.offer_entry_state[current_idx].offer_lane_mask = 1u8 << 0;
        worker.offer_entry_state[current_idx].offer_lanes = [0, 0, 0, 0, 0, 0, 0, 0];
        worker.offer_entry_state[current_idx].offer_lanes_len = 1;
        worker.offer_entry_state[fake_entry_idx] = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: 1,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Route,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: static_summary,
            observed: OfferEntryObservedState::EMPTY,
        };

        let mut active_entries = ActiveEntrySet::EMPTY;
        assert!(active_entries.insert_entry(current_idx, 0));
        assert!(active_entries.insert_entry(fake_entry_idx, 1));
        worker.global_active_entries = active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();
        worker.global_frontier_observed =
            observed_entries_with_route_entries(current_idx, fake_entry_idx);
        worker.global_frontier_observed_key =
            worker.frontier_observation_key(ScopeId::none(), false);
        worker.frontier_observation_epoch = 43;

        worker.put_back_binding_for_lane(
            0,
            crate::binding::IncomingClassification {
                label: current_state.label_meta.recv_label,
                instance: 11,
                has_fin: false,
                channel: Channel::new(7),
            },
        );

        let warmed_key = worker.frontier_observation_key(ScopeId::none(), false);
        assert!(
            worker
                .cached_frontier_observed_entries(ScopeId::none(), false, warmed_key)
                .is_some(),
            "binding inbox update should prewarm the relevant cached observation",
        );
        let current_bit = worker.global_frontier_observed.entry_bit(current_idx);
        let fake_bit = worker.global_frontier_observed.entry_bit(fake_entry_idx);
        assert_ne!(
            worker.global_frontier_observed.ready_mask & current_bit,
            0,
            "buffered binding should mark the affected entry ready in the cached observation",
        );
        assert_ne!(
            worker.global_frontier_observed.progress_mask & current_bit,
            0,
            "buffered binding should publish progress evidence for the affected entry",
        );
        assert_eq!(
            worker.global_frontier_observed.ready_mask & fake_bit,
            0,
            "unrelated offer lanes must stay untouched by the targeted binding refresh",
        );
        assert!(
            worker.frontier_observation_epoch > 43,
            "targeted binding refresh should publish a new frontier observation epoch",
        );

        let warmed_epoch = worker.frontier_observation_epoch;
        worker
            .align_cursor_to_selected_scope()
            .expect("prewarmed binding change should keep align on the cached observation path");
        assert_eq!(
            worker.frontier_observation_epoch, warmed_epoch,
            "align should hit the warmed binding cache instead of rebuilding the frontier observation",
        );

        drop(worker);
    }

    #[test]
    fn cached_frontier_changed_entry_slot_mask_ignores_non_representative_route_lane_changes() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1013);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let state = &mut worker.offer_entry_state[current_idx];
        state.offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        state.offer_lanes = [0, 1, 0, 0, 0, 0, 0, 0];
        state.offer_lanes_len = 2;
        state.lane_idx = 0;

        let mut active_entries = ActiveEntrySet::EMPTY;
        assert!(active_entries.insert_entry(current_idx, 0));
        worker.global_active_entries = active_entries;
        worker.global_offer_lane_mask = state.offer_lane_mask;
        worker.recompute_global_offer_lane_entry_slot_masks();

        let cached_key = worker.frontier_observation_key(ScopeId::none(), false);
        let mut observation_key = cached_key;
        observation_key.route_change_epochs[1] =
            observation_key.route_change_epochs[1].wrapping_add(1);
        if observation_key.route_change_epochs[1] == 0 {
            observation_key.route_change_epochs[1] = 1;
        }

        let changed_slot_mask = worker
            .cached_frontier_changed_entry_slot_mask(
                ScopeId::none(),
                false,
                observation_key,
                cached_key,
            )
            .expect("active frontier is unchanged");

        assert_eq!(
            changed_slot_mask, 0,
            "route changes on non-representative offer lanes must not invalidate the entry"
        );

        drop(worker);
    }

    #[test]
    fn refresh_frontier_observed_entries_from_cache_updates_changed_offer_lane_slots() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1008);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");

        worker.refresh_lane_offer_state(0);
        let current_idx = worker.cursor.index();
        let fake_entry_idx = current_idx + 1;
        assert!(fake_entry_idx < crate::global::typestate::MAX_STATES);

        let current_state = worker.offer_entry_state[current_idx];
        let fake_state = OfferEntryState {
            active_mask: 1u8 << 1,
            lane_idx: u8::MAX,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Route,
            scope_id: current_state.scope_id,
            offer_lane_mask: 1u8 << 1,
            offer_lanes: [1, 0, 0, 0, 0, 0, 0, 0],
            offer_lanes_len: 1,
            selection_meta: current_state.selection_meta,
            label_meta: current_state.label_meta,
            materialization_meta: current_state.materialization_meta,
            summary: OfferEntryStaticSummary::EMPTY,
            observed: OfferEntryObservedState::EMPTY,
        };
        worker.offer_entry_state[fake_entry_idx] = fake_state;

        let mut active_entries = ActiveEntrySet::EMPTY;
        assert!(active_entries.insert_entry(current_idx, 0));
        assert!(active_entries.insert_entry(fake_entry_idx, 1));
        worker.global_active_entries = active_entries;
        worker.global_offer_lane_mask = (1u8 << 0) | (1u8 << 1);
        worker.recompute_global_offer_lane_entry_slot_masks();
        let cached_key = worker.frontier_observation_key(ScopeId::none(), false);
        let cached_observed_entries =
            observed_entries_with_ready_current(current_idx, fake_entry_idx);

        let buffered = crate::binding::IncomingClassification {
            label: 41,
            channel: crate::binding::Channel::new(17),
            instance: 0,
            has_fin: false,
        };
        assert!(worker.binding_inbox.push_back(1, buffered));
        let observation_key = worker.frontier_observation_key(ScopeId::none(), false);

        let refreshed = worker
            .refresh_frontier_observed_entries_from_cache(
                ScopeId::none(),
                false,
                active_entries,
                observation_key,
                cached_key,
                cached_observed_entries,
            )
            .expect("same active frontier should refresh changed entry slots in place");

        let current_bit = refreshed.entry_bit(current_idx);
        let fake_bit = refreshed.entry_bit(fake_entry_idx);
        assert_ne!(current_bit, 0);
        assert_ne!(fake_bit, 0);
        assert_ne!(refreshed.ready_mask & current_bit, 0);
        assert_ne!(refreshed.ready_mask & fake_bit, 0);
        assert_ne!(refreshed.progress_mask & fake_bit, 0);

        drop(worker);
    }

    #[test]
    fn offer_entry_reentry_prefers_first_ready_lane_candidate() {
        let current_scope = ScopeId::generic(11);
        let current_parallel_root = ScopeId::generic(7);
        let mut ready_entry_idx = None;
        let mut any_entry_idx = None;
        record_offer_entry_reentry_candidate(
            current_scope,
            3,
            current_parallel_root,
            FrontierCandidate {
                scope_id: ScopeId::generic(20),
                entry_idx: 9,
                parallel_root: current_parallel_root,
                frontier: FrontierKind::Parallel,
                is_controller: false,
                is_dynamic: false,
                has_evidence: false,
                ready: false,
            },
            &mut ready_entry_idx,
            &mut any_entry_idx,
        );
        record_offer_entry_reentry_candidate(
            current_scope,
            3,
            current_parallel_root,
            FrontierCandidate {
                scope_id: ScopeId::generic(21),
                entry_idx: 10,
                parallel_root: current_parallel_root,
                frontier: FrontierKind::Parallel,
                is_controller: false,
                is_dynamic: false,
                has_evidence: true,
                ready: true,
            },
            &mut ready_entry_idx,
            &mut any_entry_idx,
        );
        record_offer_entry_reentry_candidate(
            current_scope,
            3,
            current_parallel_root,
            FrontierCandidate {
                scope_id: ScopeId::generic(20),
                entry_idx: 9,
                parallel_root: current_parallel_root,
                frontier: FrontierKind::Parallel,
                is_controller: false,
                is_dynamic: false,
                has_evidence: true,
                ready: true,
            },
            &mut ready_entry_idx,
            &mut any_entry_idx,
        );

        assert_eq!(any_entry_idx, Some(9));
        assert_eq!(ready_entry_idx, Some(10));
    }

    #[test]
    fn current_controller_without_evidence_yields_to_progress_sibling() {
        assert!(!current_entry_is_candidate(true, true, false, 1, true,));
    }

    #[test]
    fn current_controller_without_evidence_keeps_priority_without_progress_sibling() {
        assert!(current_entry_is_candidate(true, true, false, 1, false,));
    }

    #[test]
    fn current_controller_without_alternative_keeps_priority() {
        assert!(current_entry_is_candidate(true, true, false, 0, true,));
    }

    #[test]
    fn current_controller_with_evidence_keeps_priority() {
        assert!(current_entry_is_candidate(true, true, true, 1, true,));
    }

    #[test]
    fn controller_candidate_with_no_evidence_stays_blocked_when_current_has_offer_lanes() {
        assert!(!controller_candidate_ready(true, 10, 7, false,));
    }

    #[test]
    fn controller_candidate_without_progress_stays_blocked_in_passive_frontier() {
        assert!(!controller_candidate_ready(true, 10, 7, false,));
    }

    #[test]
    fn passive_current_is_suppressed_only_by_controller_progress_sibling() {
        assert!(should_suppress_current_passive_without_evidence(
            FrontierKind::PassiveObserver,
            false,
            false,
            true,
        ));
        assert!(!should_suppress_current_passive_without_evidence(
            FrontierKind::PassiveObserver,
            false,
            false,
            false,
        ));
    }

    #[test]
    fn evidence_less_non_current_candidate_requires_progress_or_unrunnable_current() {
        assert!(!candidate_participates_in_frontier_arbitration(
            10, 7, false, false,
        ));
        assert!(candidate_participates_in_frontier_arbitration(
            10, 7, false, true,
        ));
    }

    #[test]
    fn passive_recv_cursor_is_not_progress_evidence_for_sibling_preempt() {
        assert!(!candidate_has_progress_evidence(false, false, false));
        assert!(candidate_has_progress_evidence(true, false, false));
        assert!(candidate_has_progress_evidence(false, true, false));
        assert!(candidate_has_progress_evidence(false, false, true));
    }

    fn has_progress_controller_sibling(
        snapshot: FrontierSnapshot,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> bool {
        let mut idx = 0usize;
        while idx < snapshot.candidate_len {
            let candidate = snapshot.candidates[idx];
            if snapshot.matches_parallel_root(candidate)
                && candidate.ready
                && candidate.has_evidence
                && candidate.is_controller
                && (candidate.scope_id != scope_id || candidate.entry_idx != entry_idx)
            {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[test]
    fn passive_frontier_detects_progress_controller_sibling() {
        let current_scope = ScopeId::generic(71);
        let controller_scope = ScopeId::generic(72);
        let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: 63,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::PassiveObserver,
            is_controller: false,
            is_dynamic: false,
            has_evidence: false,
            ready: true,
        };
        candidates[1] = FrontierCandidate {
            scope_id: controller_scope,
            entry_idx: 53,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Loop,
            is_controller: true,
            is_dynamic: false,
            has_evidence: true,
            ready: true,
        };
        let snapshot = FrontierSnapshot {
            current_scope,
            current_entry_idx: 63,
            current_parallel_root: ScopeId::none(),
            current_frontier: FrontierKind::PassiveObserver,
            candidates,
            candidate_len: 2,
        };
        assert!(has_progress_controller_sibling(snapshot, current_scope, 63));
    }

    #[test]
    fn passive_frontier_ignores_controller_without_progress_evidence() {
        let current_scope = ScopeId::generic(171);
        let controller_scope = ScopeId::generic(172);
        let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: 63,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::PassiveObserver,
            is_controller: false,
            is_dynamic: false,
            has_evidence: false,
            ready: true,
        };
        candidates[1] = FrontierCandidate {
            scope_id: controller_scope,
            entry_idx: 53,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Loop,
            is_controller: true,
            is_dynamic: false,
            has_evidence: false,
            ready: true,
        };
        let snapshot = FrontierSnapshot {
            current_scope,
            current_entry_idx: 63,
            current_parallel_root: ScopeId::none(),
            current_frontier: FrontierKind::PassiveObserver,
            candidates,
            candidate_len: 2,
        };
        assert!(!has_progress_controller_sibling(
            snapshot,
            current_scope,
            63
        ));
    }

    #[test]
    fn passive_frontier_ignores_non_controller_sibling_for_controller_preemption() {
        let current_scope = ScopeId::generic(81);
        let sibling_scope = ScopeId::generic(82);
        let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: 63,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::PassiveObserver,
            is_controller: false,
            is_dynamic: false,
            has_evidence: false,
            ready: true,
        };
        candidates[1] = FrontierCandidate {
            scope_id: sibling_scope,
            entry_idx: 59,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::PassiveObserver,
            is_controller: false,
            is_dynamic: false,
            has_evidence: false,
            ready: true,
        };
        let snapshot = FrontierSnapshot {
            current_scope,
            current_entry_idx: 63,
            current_parallel_root: ScopeId::none(),
            current_frontier: FrontierKind::PassiveObserver,
            candidates,
            candidate_len: 2,
        };
        assert!(!has_progress_controller_sibling(
            snapshot,
            current_scope,
            63
        ));
    }

    #[test]
    fn frontier_yield_ping_pong_is_bounded() {
        let mut visited = FrontierVisitSet::EMPTY;
        let scope_a = ScopeId::generic(31);
        let scope_b = ScopeId::generic(32);
        visited.record(scope_a);
        visited.record(scope_b);
        visited.record(scope_a);
        assert!(visited.contains(scope_a));
        assert!(visited.contains(scope_b));
        assert_eq!(visited.len, 2);
    }

    #[test]
    fn route_defer_yields_to_sibling_scope() {
        let current_scope = ScopeId::generic(41);
        let sibling_scope = ScopeId::generic(42);
        let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: 10,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Route,
            is_controller: true,
            is_dynamic: true,
            has_evidence: false,
            ready: false,
        };
        candidates[1] = FrontierCandidate {
            scope_id: sibling_scope,
            entry_idx: 12,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Route,
            is_controller: true,
            is_dynamic: true,
            has_evidence: true,
            ready: true,
        };
        let snapshot = FrontierSnapshot {
            current_scope,
            current_entry_idx: 10,
            current_parallel_root: ScopeId::none(),
            current_frontier: FrontierKind::Route,
            candidates,
            candidate_len: 2,
        };
        let picked = snapshot
            .select_yield_candidate(FrontierVisitSet::EMPTY)
            .expect("route frontier must yield to progress sibling");
        assert_eq!(picked.scope_id, sibling_scope);
        assert_eq!(picked.frontier, FrontierKind::Route);
    }

    #[test]
    fn loop_defer_yields_to_sibling_scope() {
        let current_scope = ScopeId::generic(51);
        let sibling_scope = ScopeId::generic(52);
        let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: 20,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Loop,
            is_controller: true,
            is_dynamic: true,
            has_evidence: false,
            ready: false,
        };
        candidates[1] = FrontierCandidate {
            scope_id: sibling_scope,
            entry_idx: 24,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::Loop,
            is_controller: true,
            is_dynamic: true,
            has_evidence: true,
            ready: true,
        };
        let snapshot = FrontierSnapshot {
            current_scope,
            current_entry_idx: 20,
            current_parallel_root: ScopeId::none(),
            current_frontier: FrontierKind::Loop,
            candidates,
            candidate_len: 2,
        };
        let picked = snapshot
            .select_yield_candidate(FrontierVisitSet::EMPTY)
            .expect("loop frontier must yield to progress sibling");
        assert_eq!(picked.scope_id, sibling_scope);
        assert_eq!(picked.frontier, FrontierKind::Loop);
    }

    #[test]
    fn defer_yields_across_frontier_in_same_parallel_root() {
        let root = ScopeId::generic(55);
        let current_scope = ScopeId::generic(56);
        let sibling_scope = ScopeId::generic(57);
        let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: 20,
            parallel_root: root,
            frontier: FrontierKind::Loop,
            is_controller: true,
            is_dynamic: true,
            has_evidence: false,
            ready: false,
        };
        candidates[1] = FrontierCandidate {
            scope_id: sibling_scope,
            entry_idx: 24,
            parallel_root: root,
            frontier: FrontierKind::Route,
            is_controller: true,
            is_dynamic: true,
            has_evidence: true,
            ready: true,
        };
        let snapshot = FrontierSnapshot {
            current_scope,
            current_entry_idx: 20,
            current_parallel_root: root,
            current_frontier: FrontierKind::Loop,
            candidates,
            candidate_len: 2,
        };
        let picked = snapshot
            .select_yield_candidate(FrontierVisitSet::EMPTY)
            .expect("defer must yield to progress sibling in same parallel root");
        assert_eq!(picked.scope_id, sibling_scope);
        assert_eq!(picked.frontier, FrontierKind::Route);
    }

    #[test]
    fn parallel_frontier_prefers_ready_lane_before_phase_join() {
        let current_scope = ScopeId::generic(61);
        let root = ScopeId::generic(60);
        let ready_scope = ScopeId::generic(62);
        let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: 30,
            parallel_root: root,
            frontier: FrontierKind::Parallel,
            is_controller: true,
            is_dynamic: true,
            has_evidence: false,
            ready: false,
        };
        candidates[1] = FrontierCandidate {
            scope_id: ScopeId::generic(63),
            entry_idx: 31,
            parallel_root: root,
            frontier: FrontierKind::Parallel,
            is_controller: false,
            is_dynamic: false,
            has_evidence: false,
            ready: false,
        };
        candidates[2] = FrontierCandidate {
            scope_id: ready_scope,
            entry_idx: 32,
            parallel_root: root,
            frontier: FrontierKind::Parallel,
            is_controller: false,
            is_dynamic: false,
            has_evidence: true,
            ready: true,
        };
        let snapshot = FrontierSnapshot {
            current_scope,
            current_entry_idx: 30,
            current_parallel_root: root,
            current_frontier: FrontierKind::Parallel,
            candidates,
            candidate_len: 3,
        };
        let picked = snapshot
            .select_yield_candidate(FrontierVisitSet::EMPTY)
            .expect("parallel frontier must choose progress sibling");
        assert_eq!(picked.scope_id, ready_scope);
        assert_eq!(picked.entry_idx, 32);
    }

    #[test]
    fn passive_observer_defer_follow_is_progressive() {
        let current_scope = ScopeId::generic(71);
        let sibling_scope = ScopeId::generic(72);
        let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: 40,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::PassiveObserver,
            is_controller: false,
            is_dynamic: false,
            has_evidence: false,
            ready: true,
        };
        candidates[1] = FrontierCandidate {
            scope_id: sibling_scope,
            entry_idx: 44,
            parallel_root: ScopeId::none(),
            frontier: FrontierKind::PassiveObserver,
            is_controller: false,
            is_dynamic: false,
            has_evidence: true,
            ready: true,
        };
        let snapshot = FrontierSnapshot {
            current_scope,
            current_entry_idx: 40,
            current_parallel_root: ScopeId::none(),
            current_frontier: FrontierKind::PassiveObserver,
            candidates,
            candidate_len: 2,
        };
        let mut visited = FrontierVisitSet::EMPTY;
        visited.record(current_scope);
        let picked = snapshot
            .select_yield_candidate(visited)
            .expect("passive observer defer must progress to sibling");
        assert_eq!(picked.scope_id, sibling_scope);
        assert_ne!(picked.scope_id, current_scope);
    }

    #[test]
    fn passive_observer_defer_stops_without_progress_evidence() {
        let root = ScopeId::generic(73);
        let current_scope = ScopeId::generic(74);
        let sibling_scope = ScopeId::generic(75);
        let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
        candidates[0] = FrontierCandidate {
            scope_id: current_scope,
            entry_idx: 50,
            parallel_root: root,
            frontier: FrontierKind::PassiveObserver,
            is_controller: false,
            is_dynamic: false,
            has_evidence: false,
            ready: true,
        };
        candidates[1] = FrontierCandidate {
            scope_id: sibling_scope,
            entry_idx: 53,
            parallel_root: root,
            frontier: FrontierKind::Loop,
            is_controller: true,
            is_dynamic: false,
            has_evidence: false,
            ready: true,
        };
        let snapshot = FrontierSnapshot {
            current_scope,
            current_entry_idx: 50,
            current_parallel_root: root,
            current_frontier: FrontierKind::PassiveObserver,
            candidates,
            candidate_len: 2,
        };
        let mut visited = FrontierVisitSet::EMPTY;
        visited.record(current_scope);
        assert_eq!(snapshot.select_yield_candidate(visited), None);
    }

    #[test]
    fn controller_local_ready_is_not_progress_evidence_for_sibling_preempt() {
        assert!(
            current_entry_is_candidate(true, true, false, 1, false),
            "controller local-ready only must not preempt without progress evidence"
        );
    }

    #[test]
    fn frontier_arbitration_is_uniform_across_route_loop_parallel_observer() {
        let cases = [
            (ScopeId::none(), FrontierKind::Route),
            (ScopeId::none(), FrontierKind::Loop),
            (ScopeId::generic(101), FrontierKind::Parallel),
            (ScopeId::none(), FrontierKind::PassiveObserver),
        ];
        let mut idx = 0usize;
        while idx < cases.len() {
            let (parallel_root, frontier) = cases[idx];
            let current_scope = ScopeId::generic((110 + idx) as u16);
            let sibling_scope = ScopeId::generic((120 + idx) as u16);
            let mut candidates = [FrontierCandidate::EMPTY; MAX_LANES];
            candidates[0] = FrontierCandidate {
                scope_id: current_scope,
                entry_idx: 70 + idx,
                parallel_root,
                frontier,
                is_controller: false,
                is_dynamic: false,
                has_evidence: false,
                ready: true,
            };
            candidates[1] = FrontierCandidate {
                scope_id: sibling_scope,
                entry_idx: 80 + idx,
                parallel_root,
                frontier,
                is_controller: true,
                is_dynamic: true,
                has_evidence: true,
                ready: true,
            };
            let snapshot = FrontierSnapshot {
                current_scope,
                current_entry_idx: 70 + idx,
                current_parallel_root: parallel_root,
                current_frontier: frontier,
                candidates,
                candidate_len: 2,
            };
            let picked = snapshot
                .select_yield_candidate(FrontierVisitSet::EMPTY)
                .expect("uniform frontier defer must pick progress-evidence-bearing sibling");
            assert_eq!(picked.scope_id, sibling_scope);
            assert_eq!(picked.frontier, frontier);
            idx += 1;
        }
    }

    #[test]
    fn dynamic_route_ignores_hint_classification_for_authority() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_LEFT_DATA_LABEL);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(904);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");
        assert!(
            worker
                .cursor
                .first_recv_target(scope, HINT_LEFT_DATA_LABEL)
                .is_none(),
            "dynamic route arm authority must not depend on first-recv dispatch"
        );

        let mut offer = pin!(worker.offer());
        let mut cx = Context::from_waker(noop_waker_ref());
        let first_poll = offer.as_mut().poll(&mut cx);
        let mut branch = match first_poll {
            Poll::Ready(Ok(next_branch)) => Some(next_branch),
            Poll::Ready(Err(err)) => panic!("offer should not fail before decision: {err:?}"),
            Poll::Pending => None,
        };
        controller.port_for_lane(0).record_route_decision(scope, 0);
        if branch.is_none() {
            let mut attempts = 0usize;
            while attempts < 4 {
                match offer.as_mut().poll(&mut cx) {
                    Poll::Ready(Ok(next_branch)) => {
                        branch = Some(next_branch);
                        break;
                    }
                    Poll::Ready(Err(err)) => {
                        panic!("offer should resolve via authoritative decision: {err:?}");
                    }
                    Poll::Pending => {}
                }
                attempts += 1;
            }
        }
        let branch = branch.expect("offer should become ready after authoritative decision");
        assert_eq!(
            branch.label(),
            HINT_LEFT_DATA_LABEL,
            "resolved branch must follow authoritative arm, not hint-derived ACK"
        );
        drop(branch);
        drop(controller);
    }

    #[test]
    fn select_scope_prepass_keeps_pending_scope_evidence_non_consuming() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_LEFT_DATA_LABEL);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9041);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        controller.port_for_lane(0).record_route_decision(scope, 0);
        let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
        worker.refresh_lane_offer_state(0);
        let entry_idx = state_index_to_usize(worker.lane_offer_state[0].entry);
        let entry_state = worker.offer_entry_state[entry_idx];
        let (_binding_ready, has_ack, has_ready_arm_evidence) =
            worker.preview_offer_entry_evidence_non_consuming(entry_state);
        assert!(has_ack, "prepass may observe pending ACK authority");
        assert!(
            !has_ready_arm_evidence,
            "pending demux hints must not be promoted to ready-arm evidence during prepass"
        );

        worker
            .align_cursor_to_selected_scope()
            .expect("scope prepass should succeed without consuming evidence");
        assert!(
            worker.peek_scope_ack(scope).is_none(),
            "prepass must not consume route ACK authority into scope evidence"
        );
        assert!(
            worker.peek_scope_hint(scope).is_none(),
            "prepass must not consume route hints into scope evidence"
        );
        assert_eq!(
            worker.scope_ready_arm_mask(scope),
            0,
            "prepass must not synthesize ready-arm evidence before selected-scope ingest"
        );
        assert_eq!(
            worker.port_for_lane(0).peek_route_decision(scope, 1),
            Some(0),
            "authoritative route ACK must remain pending on the port after prepass"
        );
        assert!(
            worker
                .port_for_lane(0)
                .has_route_hint_matching(|label| label == HINT_LEFT_DATA_LABEL),
            "matching route hint must remain queued on the port after prepass"
        );

        worker.ingest_scope_evidence_for_offer(scope, 0, 1u8 << 0, true, label_meta);

        assert_eq!(
            worker
                .peek_scope_ack(scope)
                .map(|token| token.arm().as_u8()),
            Some(0),
            "selected-scope ingest must materialize the pending ACK exactly once"
        );
        assert!(
            worker.scope_has_ready_arm_evidence(scope),
            "selected-scope ingest must materialize ready-arm evidence from the pending hint"
        );
        assert_eq!(
            worker.port_for_lane(0).peek_route_decision(scope, 1),
            None,
            "selected-scope ingest must consume the pending ACK from the port"
        );
        assert!(
            !worker
                .port_for_lane(0)
                .has_route_hint_matching(|label| label == HINT_LEFT_DATA_LABEL),
            "selected-scope ingest must consume the pending hint from the port"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn preview_offer_entry_evidence_skips_binding_probe_when_ack_already_progresses_scope() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9042);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        controller.port_for_lane(0).record_route_decision(scope, 0);
        worker.refresh_lane_offer_state(0);
        let entry_idx = state_index_to_usize(worker.lane_offer_state[0].entry);
        let entry_state = worker.offer_entry_state[entry_idx];
        let (binding_ready, has_ack, has_ready_arm_evidence) =
            worker.preview_offer_entry_evidence_non_consuming(entry_state);

        assert!(!binding_ready, "empty binding must remain not-ready");
        assert!(has_ack, "pending route decision must count as ACK evidence");
        assert!(
            !has_ready_arm_evidence,
            "ACK-only preview must not synthesize ready-arm evidence"
        );
        assert_eq!(
            worker.binding.poll_count(),
            0,
            "binding probe must be skipped when ACK already supplies progress evidence"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn preview_offer_entry_evidence_defers_binding_poll_until_selected_scope() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9043);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let classification = IncomingClassification {
            label: HINT_LEFT_DATA_LABEL,
            instance: 9,
            has_fin: false,
            channel: Channel::new(3),
        };
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(
                rv_id,
                sid,
                &HINT_WORKER_PROGRAM,
                TestBinding::with_incoming(&[classification]),
            )
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        worker.refresh_lane_offer_state(0);
        let entry_idx = state_index_to_usize(worker.lane_offer_state[0].entry);
        let entry_state = worker.offer_entry_state[entry_idx];
        let (binding_ready, has_ack, has_ready_arm_evidence) =
            worker.preview_offer_entry_evidence_non_consuming(entry_state);

        assert!(
            !binding_ready,
            "prepass must not probe binding to synthesize ready state"
        );
        assert!(
            !has_ack,
            "classification-only prepass must not synthesize ACK authority"
        );
        assert!(
            !has_ready_arm_evidence,
            "classification-only prepass must not synthesize ready-arm evidence"
        );
        assert_eq!(
            worker.binding.poll_count(),
            0,
            "prepass must not touch binding before selected-scope demux"
        );

        let picked = worker.poll_binding_for_offer(
            scope,
            entry_state.lane_idx as usize,
            entry_state.offer_lane_mask,
            entry_state.label_meta,
            entry_state.materialization_meta,
        );
        assert_eq!(
            picked,
            Some((0, classification)),
            "selected-scope poll must still resolve the deferred binding classification"
        );
        assert_eq!(
            worker.binding.poll_count(),
            1,
            "binding must be polled exactly once after scope selection"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn hint_or_classification_never_writes_ack_authority() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_LEFT_DATA_LABEL);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(905);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(
                rv_id,
                sid,
                &HINT_WORKER_PROGRAM,
                TestBinding::with_incoming(&[IncomingClassification {
                    label: HINT_LEFT_DATA_LABEL,
                    instance: 0,
                    has_fin: false,
                    channel: Channel::new(1),
                }]),
            )
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);

        worker.ingest_scope_evidence_for_offer(scope, 0, 1u8 << 0, true, label_meta);
        assert!(
            worker.peek_scope_ack(scope).is_none(),
            "dynamic hint ingest must not mint ack authority"
        );

        let mut binding_classification = None;
        worker.cache_binding_classification_for_offer(
            scope,
            0,
            1u8 << 0,
            label_meta,
            worker.offer_scope_materialization_meta(scope, 0),
            &mut binding_classification,
        );
        assert!(
            binding_classification.is_some(),
            "binding classification should still be staged for decode/demux"
        );
        let classification =
            binding_classification.expect("binding classification should be available");
        worker.ingest_binding_scope_evidence(scope, classification.label, true, label_meta);
        assert!(
            worker.peek_scope_ack(scope).is_none(),
            "classification must not mint ack authority for dynamic route"
        );
        assert_eq!(
            worker.poll_arm_from_ready_mask(scope),
            None,
            "dynamic binding evidence must not materialize Poll authority"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn poll_binding_for_offer_prefers_exact_label_for_ack_arm() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9044);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(
                rv_id,
                sid,
                &HINT_WORKER_PROGRAM,
                TestBinding::with_incoming(&[
                    IncomingClassification {
                        label: HINT_LEFT_DATA_LABEL,
                        instance: 7,
                        has_fin: false,
                        channel: Channel::new(3),
                    },
                    IncomingClassification {
                        label: HINT_RIGHT_DATA_LABEL,
                        instance: 9,
                        has_fin: false,
                        channel: Channel::new(5),
                    },
                ]),
            )
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        worker.refresh_lane_offer_state(0);
        let entry_idx = state_index_to_usize(worker.lane_offer_state[0].entry);
        let entry_state = worker.offer_entry_state[entry_idx];
        let label_meta = ScopeLabelMeta {
            hint_label_mask: ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            ..ScopeLabelMeta::EMPTY
        };
        assert_eq!(
            label_meta.preferred_binding_label(Some(1)),
            Some(HINT_RIGHT_DATA_LABEL)
        );
        worker.record_scope_ack(
            scope,
            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
        );

        let picked = worker.poll_binding_for_offer(
            scope,
            entry_state.lane_idx as usize,
            entry_state.offer_lane_mask,
            label_meta,
            entry_state.materialization_meta,
        );
        assert_eq!(
            picked.map(|(lane_idx, classification)| (lane_idx, classification.label)),
            Some((0, HINT_RIGHT_DATA_LABEL)),
            "authoritative arm should narrow binding demux to the exact matching label"
        );
        let deferred = worker.binding_inbox.take_matching_or_poll(
            &mut worker.binding,
            0,
            HINT_LEFT_DATA_LABEL,
        );
        assert_eq!(
            deferred.map(|classification| classification.label),
            Some(HINT_LEFT_DATA_LABEL),
            "non-authoritative arm classification must remain buffered"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn poll_binding_for_offer_prefers_buffered_matching_lane_before_empty_poll_lane() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9046);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        let buffered = IncomingClassification {
            label: HINT_RIGHT_DATA_LABEL,
            instance: 9,
            has_fin: false,
            channel: Channel::new(5),
        };
        worker.binding_inbox.put_back(2, buffered);

        let label_meta = ScopeLabelMeta {
            hint_label_mask: ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            arm_label_masks: [0, ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)],
            evidence_arm_label_masks: [0, ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)],
            ..ScopeLabelMeta::EMPTY
        };
        worker.record_scope_ack(
            scope,
            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
        );

        let picked = worker.poll_binding_for_offer(
            scope,
            0,
            (1u8 << 0) | (1u8 << 2),
            label_meta,
            worker.offer_scope_materialization_meta(scope, 0),
        );
        assert_eq!(
            picked,
            Some((2, buffered)),
            "buffered matching lane should be selected before probing empty poll lane"
        );
        assert_eq!(
            worker.binding.poll_count(),
            0,
            "buffered cross-lane hit should not poll unrelated empty lanes first"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn poll_binding_for_offer_skips_non_demux_lanes_for_authoritative_arm() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9047);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        let matching = IncomingClassification {
            label: HINT_RIGHT_DATA_LABEL,
            instance: 9,
            has_fin: false,
            channel: Channel::new(5),
        };
        let loop_mismatch = IncomingClassification {
            label: LABEL_LOOP_CONTINUE,
            instance: 1,
            has_fin: false,
            channel: Channel::new(7),
        };
        worker.binding_inbox.put_back(0, loop_mismatch);
        worker.binding_inbox.put_back(2, matching);

        let extra_label = 99;
        let label_meta = ScopeLabelMeta {
            hint_label_mask: ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(extra_label),
            arm_label_masks: [
                0,
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)
                    | ScopeLabelMeta::label_bit(extra_label),
            ],
            evidence_arm_label_masks: [
                0,
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)
                    | ScopeLabelMeta::label_bit(extra_label),
            ],
            ..ScopeLabelMeta::EMPTY
        };
        let materialization_meta = ScopeArmMaterializationMeta {
            binding_demux_lane_mask: [0, 1u8 << 2],
            ..ScopeArmMaterializationMeta::EMPTY
        };
        worker.record_scope_ack(
            scope,
            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
        );

        let picked = worker.poll_binding_for_offer(
            scope,
            0,
            (1u8 << 0) | (1u8 << 2),
            label_meta,
            materialization_meta,
        );
        assert_eq!(picked, Some((2, matching)));
        assert_eq!(
            worker
                .binding_inbox
                .take_matching_or_poll(&mut worker.binding, 0, LABEL_LOOP_CONTINUE,),
            Some(loop_mismatch),
            "authoritative arm demux must not scan unrelated loop-control lane"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn poll_binding_for_offer_prefers_authoritative_arm_label_mask_when_non_singleton() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9045);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(
                rv_id,
                sid,
                &HINT_WORKER_PROGRAM,
                TestBinding::with_incoming(&[
                    IncomingClassification {
                        label: HINT_RIGHT_DATA_LABEL,
                        instance: 9,
                        has_fin: false,
                        channel: Channel::new(5),
                    },
                    IncomingClassification {
                        label: HINT_LEFT_DATA_LABEL,
                        instance: 7,
                        has_fin: false,
                        channel: Channel::new(3),
                    },
                ]),
            )
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        worker.refresh_lane_offer_state(0);
        let entry_idx = state_index_to_usize(worker.lane_offer_state[0].entry);
        let entry_state = worker.offer_entry_state[entry_idx];
        let extra_label = 99;
        let label_meta = ScopeLabelMeta {
            hint_label_mask: ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(extra_label),
            arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                    | ScopeLabelMeta::label_bit(extra_label),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                    | ScopeLabelMeta::label_bit(extra_label),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            ..ScopeLabelMeta::EMPTY
        };
        assert_eq!(label_meta.preferred_binding_label(Some(0)), None);
        assert_eq!(
            label_meta.preferred_binding_label_mask(Some(0)),
            ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(extra_label)
        );
        worker.record_scope_ack(
            scope,
            RouteDecisionToken::from_ack(Arm::new(0).expect("binary route arm")),
        );

        let picked = worker.poll_binding_for_offer(
            scope,
            entry_state.lane_idx as usize,
            entry_state.offer_lane_mask,
            label_meta,
            entry_state.materialization_meta,
        );
        assert_eq!(
            picked.map(|(lane_idx, classification)| (lane_idx, classification.label)),
            Some((0, HINT_LEFT_DATA_LABEL)),
            "authoritative arm mask should skip buffered labels from the other arm"
        );
        let deferred = worker.binding_inbox.take_matching_or_poll(
            &mut worker.binding,
            0,
            HINT_RIGHT_DATA_LABEL,
        );
        assert_eq!(
            deferred.map(|classification| classification.label),
            Some(HINT_RIGHT_DATA_LABEL),
            "non-authoritative arm classification must remain buffered after mask match"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn poll_binding_for_offer_uses_label_mask_to_skip_other_arm_lanes_without_authority() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9048);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        let matching = IncomingClassification {
            label: HINT_RIGHT_DATA_LABEL,
            instance: 9,
            has_fin: false,
            channel: Channel::new(5),
        };
        let loop_mismatch = IncomingClassification {
            label: LABEL_LOOP_CONTINUE,
            instance: 1,
            has_fin: false,
            channel: Channel::new(7),
        };
        worker.binding_inbox.put_back(0, loop_mismatch);
        worker.binding_inbox.put_back(2, matching);

        let label_meta = ScopeLabelMeta {
            hint_label_mask: ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            ..ScopeLabelMeta::EMPTY
        };
        let materialization_meta = ScopeArmMaterializationMeta {
            binding_demux_lane_mask: [1u8 << 0, 1u8 << 2],
            ..ScopeArmMaterializationMeta::EMPTY
        };

        let picked = worker.poll_binding_for_offer(
            scope,
            0,
            (1u8 << 0) | (1u8 << 2),
            label_meta,
            materialization_meta,
        );
        assert_eq!(picked, Some((2, matching)));
        assert_eq!(
            worker
                .binding_inbox
                .take_matching_or_poll(&mut worker.binding, 0, LABEL_LOOP_CONTINUE,),
            Some(loop_mismatch),
            "no-authority demux should still restrict scans to lanes implied by the label mask"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn poll_binding_for_offer_buffered_match_skips_drop_only_preferred_lane_for_non_singleton_mask()
    {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9050);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        let matching = IncomingClassification {
            label: HINT_RIGHT_DATA_LABEL,
            instance: 9,
            has_fin: false,
            channel: Channel::new(5),
        };
        let loop_mismatch = IncomingClassification {
            label: LABEL_LOOP_CONTINUE,
            instance: 1,
            has_fin: false,
            channel: Channel::new(7),
        };
        worker.binding_inbox.put_back(0, loop_mismatch);
        worker.binding_inbox.put_back(2, matching);

        let label_meta = ScopeLabelMeta {
            hint_label_mask: ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            ..ScopeLabelMeta::EMPTY
        };
        let materialization_meta = ScopeArmMaterializationMeta {
            binding_demux_lane_mask: [1u8 << 0, 1u8 << 2],
            ..ScopeArmMaterializationMeta::EMPTY
        };

        let picked = worker.poll_binding_for_offer(
            scope,
            0,
            (1u8 << 0) | (1u8 << 2),
            label_meta,
            materialization_meta,
        );
        assert_eq!(picked, Some((2, matching)));
        assert_eq!(
            worker
                .binding_inbox
                .take_matching_or_poll(&mut worker.binding, 0, LABEL_LOOP_CONTINUE,),
            Some(loop_mismatch),
            "buffered matching lane should win before scanning drop-only preferred lane"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn poll_binding_for_offer_polls_only_selected_lane_for_unbuffered_generic_mask() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9052);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let matching = IncomingClassification {
            label: HINT_RIGHT_DATA_LABEL,
            instance: 9,
            has_fin: false,
            channel: Channel::new(5),
        };
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(
                rv_id,
                sid,
                &HINT_WORKER_PROGRAM,
                LaneAwareTestBinding::with_lane_incoming(&[(2, matching)]),
            )
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        let label_meta = ScopeLabelMeta {
            hint_label_mask: ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            ..ScopeLabelMeta::EMPTY
        };
        let materialization_meta = ScopeArmMaterializationMeta {
            binding_demux_lane_mask: [1u8 << 0, 1u8 << 2],
            ..ScopeArmMaterializationMeta::EMPTY
        };

        let picked = worker.poll_binding_for_offer(
            scope,
            0,
            (1u8 << 0) | (1u8 << 2),
            label_meta,
            materialization_meta,
        );
        assert_eq!(
            picked, None,
            "generic mask path must not probe unbuffered cross-lane bindings before the selected lane"
        );
        assert_eq!(worker.binding.poll_count_for_lane(0), 1);
        assert_eq!(worker.binding.poll_count_for_lane(2), 0);

        let picked = worker.poll_binding_for_offer(
            scope,
            2,
            (1u8 << 0) | (1u8 << 2),
            label_meta,
            materialization_meta,
        );
        assert_eq!(picked, Some((2, matching)));
        assert_eq!(worker.binding.poll_count_for_lane(2), 1);

        drop(worker);
        drop(controller);
    }

    #[test]
    fn poll_binding_for_offer_polls_authoritative_demux_lane_when_current_lane_is_excluded() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9053);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let matching = IncomingClassification {
            label: HINT_RIGHT_DATA_LABEL,
            instance: 11,
            has_fin: false,
            channel: Channel::new(6),
        };
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(
                rv_id,
                sid,
                &HINT_WORKER_PROGRAM,
                LaneAwareTestBinding::with_lane_incoming(&[(2, matching)]),
            )
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");
        worker.record_scope_ack(
            scope,
            RouteDecisionToken::from_ack(Arm::new(1).expect("binary route arm")),
        );
        let label_meta = ScopeLabelMeta {
            hint_label_mask: ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            arm_label_masks: [0, ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)],
            evidence_arm_label_masks: [0, ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)],
            ..ScopeLabelMeta::EMPTY
        };
        let materialization_meta = ScopeArmMaterializationMeta {
            binding_demux_lane_mask: [0, 1u8 << 2],
            ..ScopeArmMaterializationMeta::EMPTY
        };

        let picked = worker.poll_binding_for_offer(
            scope,
            0,
            (1u8 << 0) | (1u8 << 2),
            label_meta,
            materialization_meta,
        );
        assert_eq!(picked, Some((2, matching)));
        assert_eq!(worker.binding.poll_count_for_lane(0), 0);
        assert_eq!(worker.binding.poll_count_for_lane(2), 1);

        drop(worker);
        drop(controller);
    }

    #[test]
    fn take_binding_for_selected_arm_preserves_cached_other_arm_classification() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(9049);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, TestBinding::default())
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        let matching = IncomingClassification {
            label: HINT_LEFT_DATA_LABEL,
            instance: 9,
            has_fin: true,
            channel: Channel::new(5),
        };
        let cached_mismatch = IncomingClassification {
            label: HINT_RIGHT_DATA_LABEL,
            instance: 7,
            has_fin: false,
            channel: Channel::new(3),
        };
        worker.binding_inbox.put_back(0, matching);
        let extra_label = 99;
        let label_meta = ScopeLabelMeta {
            hint_label_mask: ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL)
                | ScopeLabelMeta::label_bit(extra_label),
            arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                    | ScopeLabelMeta::label_bit(extra_label),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            evidence_arm_label_masks: [
                ScopeLabelMeta::label_bit(HINT_LEFT_DATA_LABEL)
                    | ScopeLabelMeta::label_bit(extra_label),
                ScopeLabelMeta::label_bit(HINT_RIGHT_DATA_LABEL),
            ],
            ..ScopeLabelMeta::EMPTY
        };
        let mut binding_classification = Some(cached_mismatch);

        let (channel, instance, has_fin) =
            worker.take_binding_for_selected_arm(0, 0, label_meta, &mut binding_classification);
        assert_eq!(channel, Some(matching.channel));
        assert_eq!(instance, Some(matching.instance));
        assert!(
            has_fin,
            "selected-arm helper should preserve FIN from matching ingress"
        );
        assert!(
            binding_classification.is_none(),
            "cached mismatch should be re-buffered, not left staged"
        );
        let deferred = worker.binding_inbox.take_matching_or_poll(
            &mut worker.binding,
            0,
            HINT_RIGHT_DATA_LABEL,
        );
        assert_eq!(
            deferred,
            Some(cached_mismatch),
            "selected-arm demux must preserve cached other-arm classifications"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn static_passive_binding_label_materializes_poll() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(906);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &ENTRY_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(
                rv_id,
                sid,
                &ENTRY_WORKER_PROGRAM,
                TestBinding::with_incoming(&[IncomingClassification {
                    label: ENTRY_ARM0_SIGNAL_LABEL,
                    instance: 0,
                    has_fin: false,
                    channel: Channel::new(1),
                }]),
            )
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");
        assert!(
            worker
                .cursor
                .first_recv_target(scope, ENTRY_ARM0_SIGNAL_LABEL)
                .is_some(),
            "test requires a static passive recv dispatch target"
        );

        let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);

        let mut binding_classification = None;
        worker.cache_binding_classification_for_offer(
            scope,
            0,
            1u8 << 0,
            label_meta,
            worker.offer_scope_materialization_meta(scope, 0),
            &mut binding_classification,
        );
        let classification =
            binding_classification.expect("binding classification should be staged for poll");
        worker.ingest_scope_evidence_for_offer(scope, 0, 1u8 << 0, false, label_meta);
        worker.ingest_binding_scope_evidence(scope, classification.label, false, label_meta);

        assert!(
            worker.peek_scope_ack(scope).is_none(),
            "binding-backed static dispatch must not mint ack authority"
        );
        let resolved_label = worker.take_scope_hint(scope);
        assert_eq!(
            resolved_label,
            Some(classification.label),
            "binding-backed poll should still preserve the resolved ingress label"
        );
        assert_eq!(
            worker.poll_arm_from_ready_mask(scope),
            Some(Arm::new(0).expect("binary route arm")),
            "exact binding ingress on a static passive route must materialize Poll authority"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn static_passive_staged_transport_hint_materializes_poll() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(ENTRY_ARM0_SIGNAL_LABEL);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(907);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &ENTRY_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &ENTRY_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");
        assert!(
            worker
                .cursor
                .first_recv_target(scope, ENTRY_ARM0_SIGNAL_LABEL)
                .is_some(),
            "test requires a static passive recv dispatch target"
        );

        let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
        worker.ingest_scope_evidence_for_offer(scope, 0, 1u8 << 0, false, label_meta);

        assert_eq!(
            worker.poll_arm_from_ready_mask(scope),
            None,
            "transport hint alone must remain non-authoritative until ingress is staged"
        );
        assert!(
            worker.peek_scope_ack(scope).is_none(),
            "transport-backed static dispatch must not mint ack authority"
        );
        let resolved_label = worker.take_scope_hint(scope);
        assert_eq!(
            resolved_label,
            Some(ENTRY_ARM0_SIGNAL_LABEL),
            "transport-backed poll should still preserve the resolved ingress label"
        );
        worker.mark_scope_ready_arm_from_label(
            scope,
            resolved_label.expect("transport hint must resolve"),
            label_meta,
        );
        assert_eq!(
            worker.poll_arm_from_ready_mask(scope),
            Some(Arm::new(0).expect("binary route arm")),
            "staged exact transport ingress on a static passive route must materialize Poll authority"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn nested_static_passive_binding_dispatch_materializes_poll_on_ancestor_scopes() {
        type OuterLeftMsg = Msg<0x50, u8>;
        type LeafLeftMsg = Msg<0x51, u8>;
        type LeafRightMsg = Msg<0x52, u8>;
        type MiddleRightMsg = Msg<0x53, u8>;
        type StaticRouteLeftMsg = Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg =
            Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

        let inner = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, LeafLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                g::send::<Role<0>, Role<1>, LeafRightMsg, 0>(),
            ),
        );
        let middle = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                inner,
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                g::send::<Role<0>, Role<1>, MiddleRightMsg, 0>(),
            ),
        );
        let program = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, OuterLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                middle,
            ),
        );
        let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
            project(&program);
        let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
            project(&program);

        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(909);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &worker_program, NoBinding)
            .expect("attach worker endpoint");

        let outer_scope = worker.cursor.node_scope_id();
        let middle_scope = worker
            .cursor
            .passive_arm_scope_by_arm(outer_scope, 1)
            .expect("outer right arm should enter middle route");
        let inner_scope = worker
            .cursor
            .passive_arm_scope_by_arm(middle_scope, 0)
            .expect("middle left arm should enter inner route");

        assert_eq!(
            worker.cursor.first_recv_target(outer_scope, 0x51).map(|(arm, _)| arm),
            Some(1),
            "outer scope must resolve the leaf reply through first-recv dispatch"
        );
        assert_eq!(
            worker.cursor.first_recv_target(middle_scope, 0x51).map(|(arm, _)| arm),
            Some(0),
            "middle scope must resolve the leaf reply through first-recv dispatch"
        );

        for (scope, expected_arm) in [(outer_scope, 1u8), (middle_scope, 0u8), (inner_scope, 0u8)] {
            let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
            worker.ingest_scope_evidence_for_offer(scope, 0, 1u8 << 0, false, label_meta);
            worker.ingest_binding_scope_evidence(scope, 0x51, false, label_meta);
            assert_eq!(
                worker.poll_arm_from_ready_mask(scope),
                Some(Arm::new(expected_arm).expect("binary route arm")),
                "exact nested leaf ingress must materialize Poll for scope {scope:?}"
            );
        }

        drop(worker);
        drop(controller);
    }

    #[test]
    fn deep_right_nested_static_passive_binding_dispatch_materializes_poll_on_all_ancestor_scopes() {
        type OuterLeftMsg = Msg<0x50, u8>;
        type MiddleLeftMsg = Msg<0x51, u8>;
        type ThirdLeftMsg = Msg<0x52, u8>;
        type FinalLeftMsg = Msg<0x53, u8>;
        type FinalRightMsg = Msg<0x55, u8>;
        type StaticRouteLeftMsg = Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg =
            Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

        let final_decision = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, FinalLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                g::send::<Role<0>, Role<1>, FinalRightMsg, 0>(),
            ),
        );
        let third = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, ThirdLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                final_decision,
            ),
        );
        let middle = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, MiddleLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                third,
            ),
        );
        let program = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, OuterLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                middle,
            ),
        );
        let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
            project(&program);
        let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
            project(&program);

        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(910);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &worker_program, NoBinding)
            .expect("attach worker endpoint");

        let outer_scope = worker.cursor.node_scope_id();
        let middle_scope = worker
            .cursor
            .passive_arm_scope_by_arm(outer_scope, 1)
            .expect("outer right arm should enter middle route");
        let third_scope = worker
            .cursor
            .passive_arm_scope_by_arm(middle_scope, 1)
            .expect("middle right arm should enter third route");
        let final_scope = worker
            .cursor
            .passive_arm_scope_by_arm(third_scope, 1)
            .expect("third right arm should enter final route");

        for scope in [outer_scope, middle_scope, third_scope] {
            assert_eq!(
                worker.cursor.first_recv_target(scope, 0x55).map(|(arm, _)| arm),
                Some(1),
                "ancestor scope must resolve the deep final reply through first-recv dispatch"
            );
        }

        let label_meta = endpoint_scope_label_meta(&worker, outer_scope, ScopeLoopMeta::EMPTY);
        worker.ingest_scope_evidence_for_offer(outer_scope, 0, 1u8 << 0, false, label_meta);
        worker.ingest_binding_scope_evidence(outer_scope, 0x55, false, label_meta);

        for scope in [outer_scope, middle_scope, third_scope, final_scope] {
            assert_eq!(
                worker.poll_arm_from_ready_mask(scope),
                Some(Arm::new(1).expect("binary route arm")),
                "exact deep final ingress must materialize Poll for scope {scope:?}"
            );
            assert_eq!(
                worker.preview_selected_arm_for_scope(scope),
                Some(1),
                "exact deep final ingress must seed descendant preview selection for scope {scope:?}"
            );
        }

        drop(worker);
        drop(controller);
    }

    #[test]
    fn deep_right_nested_final_reply_offer_materializes_leaf_label() {
        type OuterLeftMsg = Msg<0x50, u8>;
        type MiddleLeftMsg = Msg<0x51, u8>;
        type ThirdLeftMsg = Msg<0x52, u8>;
        type FinalLeftMsg = Msg<0x53, u8>;
        type FinalRightMsg = Msg<0x55, u8>;
        type StaticRouteLeftMsg = Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg =
            Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

        let final_decision = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, FinalLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                g::send::<Role<0>, Role<1>, FinalRightMsg, 0>(),
            ),
        );
        let third = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, ThirdLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                final_decision,
            ),
        );
        let middle = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, MiddleLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                third,
            ),
        );
        let program = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, OuterLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                middle,
            ),
        );
        let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
            project(&program);
        let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
            project(&program);

        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(911);
        let payload = 0x55u8;
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
            .expect("attach controller endpoint");
        let worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(
                rv_id,
                sid,
                &worker_program,
                TestBinding::with_incoming_and_payloads(
                    &[IncomingClassification {
                        label: 0x55,
                        instance: 17,
                        has_fin: false,
                        channel: Channel::new(4),
                    }],
                    &[&[payload]],
                ),
            )
            .expect("attach worker endpoint");

        let waker = noop_waker_ref();
        let mut cx = Context::from_waker(waker);

        let mut route_right = pin!(
            controller
                .flow::<StaticRouteRightMsg>()
                .expect("open outer route-right")
                .send(())
        );
        let (controller, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("outer route-right failed: {err:?}"),
            Poll::Pending => panic!("outer route-right unexpectedly pending"),
        };
        let mut route_right = pin!(
            controller
                .flow::<StaticRouteRightMsg>()
                .expect("open middle route-right")
                .send(())
        );
        let (controller, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("middle route-right failed: {err:?}"),
            Poll::Pending => panic!("middle route-right unexpectedly pending"),
        };
        let mut route_right = pin!(
            controller
                .flow::<StaticRouteRightMsg>()
                .expect("open third route-right")
                .send(())
        );
        let (controller, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("third route-right failed: {err:?}"),
            Poll::Pending => panic!("third route-right unexpectedly pending"),
        };
        let mut route_right = pin!(
            controller
                .flow::<StaticRouteRightMsg>()
                .expect("open final route-right")
                .send(())
        );
        let (controller, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("final route-right failed: {err:?}"),
            Poll::Pending => panic!("final route-right unexpectedly pending"),
        };
        let mut reply_send = pin!(
            controller
                .flow::<FinalRightMsg>()
                .expect("open final right reply")
                .send(&payload)
        );
        let (_controller, _) = match reply_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("final right reply failed: {err:?}"),
            Poll::Pending => panic!("final right reply unexpectedly pending"),
        };

        let mut offer = pin!(worker.offer());
        let branch = match offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("worker deep final offer failed: {err:?}"),
            Poll::Pending => panic!("worker deep final offer unexpectedly pending"),
        };
        assert_eq!(branch.label(), 0x55, "worker must materialize the deep final reply");
        let mut decode = pin!(branch.decode::<FinalRightMsg>());
        match decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok((_worker, reply))) => assert_eq!(reply, payload),
            Poll::Ready(Err(err)) => panic!("worker deep final decode failed: {err:?}"),
            Poll::Pending => panic!("worker deep final decode unexpectedly pending"),
        }
    }

    #[test]
    fn deep_right_nested_final_reply_offer_materializes_leaf_label_with_deferred_binding_ingress() {
        type OuterLeftMsg = Msg<0x50, u8>;
        type MiddleLeftMsg = Msg<0x51, u8>;
        type ThirdLeftMsg = Msg<0x52, u8>;
        type FinalLeftMsg = Msg<0x53, u8>;
        type FinalRightMsg = Msg<0x55, u8>;
        type StaticRouteLeftMsg = Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg =
            Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

        let final_decision = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, FinalLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                g::send::<Role<0>, Role<1>, FinalRightMsg, 0>(),
            ),
        );
        let third = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, ThirdLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                final_decision,
            ),
        );
        let middle = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, MiddleLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                third,
            ),
        );
        let program = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, OuterLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                middle,
            ),
        );
        let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
            project(&program);
        let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
            project(&program);

        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let deferred_state = Arc::new(DeferredIngressState::default());
        let cluster: ManuallyDrop<
            SessionCluster<'_, DeferredIngressTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = DeferredIngressTransport::new(deferred_state.clone());
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(912);
        let payload = 0x55u8;
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
            .expect("attach controller endpoint");
        let worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(
                rv_id,
                sid,
                &worker_program,
                DeferredIngressBinding::with_incoming_and_payloads(
                    deferred_state,
                    &[IncomingClassification {
                        label: 0x55,
                        instance: 17,
                        has_fin: false,
                        channel: Channel::new(4),
                    }],
                    &[&[payload]],
                ),
            )
            .expect("attach worker endpoint");

        let waker = noop_waker_ref();
        let mut cx = Context::from_waker(waker);

        let mut route_right = pin!(
            controller
                .flow::<StaticRouteRightMsg>()
                .expect("open outer route-right")
                .send(())
        );
        let (controller, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("outer route-right failed: {err:?}"),
            Poll::Pending => panic!("outer route-right unexpectedly pending"),
        };
        let mut route_right = pin!(
            controller
                .flow::<StaticRouteRightMsg>()
                .expect("open middle route-right")
                .send(())
        );
        let (controller, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("middle route-right failed: {err:?}"),
            Poll::Pending => panic!("middle route-right unexpectedly pending"),
        };
        let mut route_right = pin!(
            controller
                .flow::<StaticRouteRightMsg>()
                .expect("open third route-right")
                .send(())
        );
        let (controller, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("third route-right failed: {err:?}"),
            Poll::Pending => panic!("third route-right unexpectedly pending"),
        };
        let mut route_right = pin!(
            controller
                .flow::<StaticRouteRightMsg>()
                .expect("open final route-right")
                .send(())
        );
        let (controller, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("final route-right failed: {err:?}"),
            Poll::Pending => panic!("final route-right unexpectedly pending"),
        };
        let mut reply_send = pin!(
            controller
                .flow::<FinalRightMsg>()
                .expect("open final right reply")
                .send(&payload)
        );
        let (_controller, _) = match reply_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("final right reply failed: {err:?}"),
            Poll::Pending => panic!("final right reply unexpectedly pending"),
        };

        let mut offer = pin!(worker.offer());
        let branch = match offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("worker deep final deferred offer failed: {err:?}"),
            Poll::Pending => panic!("worker deep final deferred offer unexpectedly pending"),
        };
        assert_eq!(
            branch.label(),
            0x55,
            "worker must materialize the deep final reply after deferred binding ingress"
        );
        let mut decode = pin!(branch.decode::<FinalRightMsg>());
        match decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok((_worker, reply))) => assert_eq!(reply, payload),
            Poll::Ready(Err(err)) => panic!("worker deep final deferred decode failed: {err:?}"),
            Poll::Pending => panic!("worker deep final deferred decode unexpectedly pending"),
        }
    }

    #[test]
    fn unique_ready_arm_materializes_poll_without_hint() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(908);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        assert_eq!(
            worker.poll_arm_from_ready_mask(scope),
            None,
            "no ready arm evidence must not materialize a poll arm"
        );

        worker.mark_scope_ready_arm(scope, 1);
        assert_eq!(
            worker.poll_arm_from_ready_mask(scope).map(Arm::as_u8),
            Some(1),
            "a unique ready arm should materialize a poll arm"
        );

        worker.mark_scope_ready_arm(scope, 0);
        assert_eq!(
            worker.poll_arm_from_ready_mask(scope),
            None,
            "ambiguous ready-arm evidence must not materialize a poll arm"
        );

        drop(worker);
    }

    #[test]
    fn select_scope_recovers_route_state_from_current_arm_position() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(907);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &ENTRY_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        let Some(PassiveArmNavigation::WithinArm { entry }) = worker
            .cursor
            .follow_passive_observer_arm_for_scope(scope, 0)
        else {
            panic!("worker should expose passive arm entry");
        };
        worker.set_cursor(worker.cursor.with_index(state_index_to_usize(entry)));
        assert_eq!(
            worker.selected_arm_for_scope(scope),
            None,
            "test requires missing runtime route state"
        );
        assert_eq!(
            worker
                .cursor
                .typestate_node(worker.cursor.index())
                .route_arm(),
            Some(0),
            "current node must carry structural arm annotation"
        );

        let recovered = worker
            .ensure_current_route_arm_state()
            .expect("route-state recovery should not fail");
        assert_eq!(
            recovered,
            Some(true),
            "current arm position should recover missing route state"
        );
        assert_eq!(
            worker.selected_arm_for_scope(scope),
            Some(0),
            "current arm position should restore selected arm state"
        );
    }

    #[test]
    fn route_decision_source_domain_is_closed() {
        assert!(matches!(
            RouteDecisionSource::from_tap_seq(1),
            Some(RouteDecisionSource::Ack)
        ));
        assert!(matches!(
            RouteDecisionSource::from_tap_seq(2),
            Some(RouteDecisionSource::Resolver)
        ));
        assert!(matches!(
            RouteDecisionSource::from_tap_seq(3),
            Some(RouteDecisionSource::Poll)
        ));
        assert!(RouteDecisionSource::from_tap_seq(0).is_none());
        assert!(RouteDecisionSource::from_tap_seq(4).is_none());
    }

    #[test]
    fn defer_without_new_evidence_is_capped() {
        let mut liveness = OfferLivenessState::new(crate::runtime::config::LivenessPolicy {
            max_defer_per_offer: 8,
            max_no_evidence_defer: 1,
            force_poll_on_exhaustion: false,
            max_forced_poll_attempts: 0,
            exhaust_reason: 1,
        });
        let fingerprint = EvidenceFingerprint::new(false, false, false);
        assert_eq!(liveness.on_defer(fingerprint), DeferBudgetOutcome::Continue);
        assert_eq!(liveness.on_defer(fingerprint), DeferBudgetOutcome::Continue);
        assert_eq!(
            liveness.on_defer(fingerprint),
            DeferBudgetOutcome::Exhausted
        );
    }

    #[test]
    fn defer_budget_exhaustion_forces_poll_then_abort() {
        let mut liveness = OfferLivenessState::new(crate::runtime::config::LivenessPolicy {
            max_defer_per_offer: 1,
            max_no_evidence_defer: 1,
            force_poll_on_exhaustion: true,
            max_forced_poll_attempts: 1,
            exhaust_reason: crate::epf::ENGINE_LIVENESS_EXHAUSTED,
        });
        let fingerprint = EvidenceFingerprint::new(false, false, false);
        assert_eq!(liveness.on_defer(fingerprint), DeferBudgetOutcome::Continue);
        assert_eq!(
            liveness.on_defer(fingerprint),
            DeferBudgetOutcome::Exhausted
        );
        assert!(liveness.can_force_poll());
        liveness.mark_forced_poll();
        assert!(!liveness.can_force_poll());
        assert_eq!(
            liveness.exhaust_reason(),
            crate::epf::ENGINE_LIVENESS_EXHAUSTED
        );
    }

    #[test]
    fn defer_never_promotes_to_route_authority() {
        let scope = ScopeId::generic(24);
        let mut delegate_called = false;
        let decision = route_policy_decision_from_action(Action::Defer { retry_hint: 7 }, 88);
        assert!(matches!(
            decision,
            RoutePolicyDecision::Defer {
                retry_hint: 7,
                source: DeferSource::Epf
            }
        ));
        let handle = resolve_route_decision_handle_with_policy(scope, scope, decision, || {
            delegate_called = true;
            Ok(RouteDecisionHandle { scope, arm: 1 })
        })
        .expect("defer must delegate to resolver");
        assert_eq!(handle.arm, 1);
        assert!(delegate_called);
        assert!(RouteDecisionSource::from_tap_seq(4).is_none());
    }

    #[test]
    fn scope_evidence_is_one_shot_per_offer() {
        let token = RouteDecisionToken::from_ack(Arm::new(1).expect("arm"));
        let mut evidence = ScopeEvidence {
            ack: Some(token),
            hint_label: 7,
            ready_arm_mask: ScopeEvidence::ARM1_READY,
            poll_ready_arm_mask: ScopeEvidence::ARM1_READY,
            flags: 0,
        };
        let first = {
            let ack = evidence.ack;
            evidence.ack = None;
            ack
        };
        let second = evidence.ack;
        assert_eq!(first, Some(token));
        assert_eq!(second, None);
    }

    #[test]
    fn resolver_poll_token_requires_ready_arm_evidence_for_controller_and_observer() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(990);
        let mut controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        let resolver_token = RouteDecisionToken::from_resolver(Arm::new(0).expect("arm"));
        assert!(
            !worker.route_token_has_materialization_evidence(scope, resolver_token),
            "resolver token must not materialize without arm-ready evidence"
        );

        worker.mark_scope_ready_arm(scope, 0);
        assert!(
            worker.route_token_has_materialization_evidence(scope, resolver_token),
            "resolver token may materialize only when selected arm has ready evidence"
        );

        let poll_token = RouteDecisionToken::from_poll(Arm::new(1).expect("arm"));
        assert!(
            !worker.route_token_has_materialization_evidence(scope, poll_token),
            "poll token must not materialize for unready arm"
        );

        worker.mark_scope_ready_arm(scope, 1);
        assert!(
            worker.route_token_has_materialization_evidence(scope, poll_token),
            "poll token may materialize when selected arm has ready evidence"
        );

        let controller_scope = controller.cursor.node_scope_id();
        assert!(
            !controller_scope.is_none(),
            "controller must start at route scope"
        );
        let controller_recv_arm = if controller.arm_has_recv(controller_scope, 0) {
            Some(0)
        } else if controller.arm_has_recv(controller_scope, 1) {
            Some(1)
        } else {
            None
        };
        if let Some(controller_arm) = controller_recv_arm {
            let controller_resolver_token =
                RouteDecisionToken::from_resolver(Arm::new(controller_arm).expect("arm"));
            assert!(
                !controller.route_token_has_materialization_evidence(
                    controller_scope,
                    controller_resolver_token
                ),
                "controller resolver token must not materialize without arm-ready evidence when recv is required"
            );
            controller.mark_scope_ready_arm(controller_scope, controller_arm);
            assert!(
                controller.route_token_has_materialization_evidence(
                    controller_scope,
                    controller_resolver_token
                ),
                "controller resolver token requires selected arm evidence as well"
            );
        }

        drop(worker);
        drop(controller);
    }

    #[test]
    fn recv_required_arm_needs_ready_arm_evidence_for_all_sources() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(993);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");
        let recv_arm = if worker.arm_has_recv(scope, 0) {
            0
        } else if worker.arm_has_recv(scope, 1) {
            1
        } else {
            drop(worker);
            return;
        };
        let ack_token = RouteDecisionToken::from_ack(Arm::new(recv_arm).expect("arm"));
        let resolver_token = RouteDecisionToken::from_resolver(Arm::new(recv_arm).expect("arm"));
        let poll_token = RouteDecisionToken::from_poll(Arm::new(recv_arm).expect("arm"));
        assert!(
            !worker.route_token_has_materialization_evidence(scope, ack_token),
            "ack token must not materialize recv-required arm without ready-arm evidence"
        );
        assert!(
            !worker.route_token_has_materialization_evidence(scope, resolver_token),
            "resolver token must not materialize recv-required arm without ready-arm evidence"
        );
        assert!(
            !worker.route_token_has_materialization_evidence(scope, poll_token),
            "poll token must not materialize recv-required arm without ready-arm evidence"
        );
        worker.mark_scope_ready_arm(scope, recv_arm);
        assert!(
            worker.route_token_has_materialization_evidence(scope, ack_token),
            "ack token may materialize recv-required arm when selected arm is ready"
        );
        assert!(
            worker.route_token_has_materialization_evidence(scope, resolver_token),
            "resolver token may materialize recv-required arm when selected arm is ready"
        );
        assert!(
            worker.route_token_has_materialization_evidence(scope, poll_token),
            "poll token may materialize recv-required arm when selected arm is ready"
        );
        drop(worker);
    }

    #[test]
    fn route_ack_does_not_imply_ready_arm_evidence() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(994);
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");
        let arm = if worker.arm_has_recv(scope, 0) { 0 } else { 1 };
        worker.record_scope_ack(
            scope,
            RouteDecisionToken::from_ack(Arm::new(arm).expect("arm")),
        );
        assert!(
            worker.peek_scope_ack(scope).is_some(),
            "ack authority should be preserved"
        );
        assert!(
            !worker.scope_has_ready_arm(scope, arm),
            "ack authority must not become recv-ready evidence"
        );
        drop(worker);
    }

    #[test]
    fn ready_arm_mask_is_one_shot_and_cleared_on_scope_exit() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(991);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        worker.mark_scope_ready_arm(scope, 0);
        assert!(worker.scope_has_ready_arm(scope, 0));
        worker.consume_scope_ready_arm(scope, 0);
        assert!(
            !worker.scope_has_ready_arm(scope, 0),
            "arm-ready evidence must be one-shot once consumed"
        );

        worker.mark_scope_ready_arm(scope, 1);
        assert_ne!(worker.scope_ready_arm_mask(scope), 0);
        worker.clear_scope_evidence(scope);
        assert_eq!(
            worker.scope_ready_arm_mask(scope),
            0,
            "scope exit must clear arm-ready evidence"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn send_entry_arm_with_later_recv_does_not_require_ready_evidence_to_materialize() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(995);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &ENTRY_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let scope = controller.cursor.node_scope_id();
        assert!(!scope.is_none(), "controller must start at route scope");

        let mut arm = 0u8;
        let mut found = false;
        while arm <= 1 {
            if controller.arm_has_recv(scope, arm)
                && let Some((entry, _label)) =
                    controller.cursor.controller_arm_entry_by_arm(scope, arm)
                && controller
                    .cursor
                    .with_index(state_index_to_usize(entry))
                    .try_recv_meta()
                    .is_none()
            {
                let token = RouteDecisionToken::from_resolver(Arm::new(arm).expect("arm"));
                assert!(
                    controller.route_token_has_materialization_evidence(scope, token),
                    "send/local arm entry must materialize without ready-arm evidence even when recv appears later"
                );
                found = true;
                break;
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        assert!(
            found,
            "expected a controller arm with send/local entry and later recv in the same arm"
        );
        drop(controller);
    }

    #[test]
    fn lane_offer_state_reenters_same_route_scope_using_offer_entry() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(996);
        let mut controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let scope = controller.cursor.node_scope_id();
        assert!(!scope.is_none(), "controller must start at route scope");
        let offer_entry = controller
            .cursor
            .route_scope_offer_entry(scope)
            .expect("offer entry");
        assert!(!offer_entry.is_max(), "test requires concrete offer entry");
        let next_idx = state_index_to_usize(offer_entry) + 1;
        controller.set_cursor(controller.cursor.with_index(next_idx));
        let region = controller
            .cursor
            .scope_region_by_id(scope)
            .expect("route scope region");
        assert!(
            next_idx >= region.start && next_idx < region.end,
            "test cursor must remain inside the same route scope"
        );

        controller.refresh_lane_offer_state(0);
        assert_ne!(
            controller.active_offer_mask & 0b0000_0001,
            0,
            "lane must remain pending while re-entering the same route scope"
        );
        assert_eq!(
            controller.lane_offer_state[0].entry, offer_entry,
            "lane offer state must normalize to canonical route offer_entry"
        );
        assert_eq!(
            controller.offer_entry_state[state_index_to_usize(offer_entry)].lane_idx,
            0,
            "offer entry index must cache a representative lane for direct lookup"
        );
        assert_ne!(
            controller.offer_entry_active_mask(state_index_to_usize(offer_entry)) & 0b0000_0001,
            0,
            "offer entry index must track active lanes while the route remains pending"
        );
        assert_eq!(
            controller.global_active_entries.entry_at(0),
            Some(state_index_to_usize(offer_entry)),
            "global active-entry index must point at the canonical offer entry"
        );
        controller.clear_lane_offer_state(0);
        assert_eq!(
            controller.offer_entry_active_mask(state_index_to_usize(offer_entry)) & 0b0000_0001,
            0,
            "clearing lane offer state must detach the lane from the offer entry index"
        );
        assert_eq!(
            controller.offer_entry_state[state_index_to_usize(offer_entry)].lane_idx,
            u8::MAX,
            "detaching the last lane must clear the representative lane cache"
        );
        assert_eq!(
            controller.global_active_entries.occupancy_mask(),
            0,
            "detaching the last lane must clear the global active-entry index"
        );
        drop(controller);
    }

    #[test]
    fn loop_continue_then_nested_custom_route_right_send_stays_well_scoped() {
        type LoopContinueMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_CONTINUE },
            GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
        >;
        type LoopBreakMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_BREAK },
            GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
        >;
        type StaticRouteLeftMsg = Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg = Msg<
            99,
            GenericCapToken<RouteHintRightKind>,
            CanonicalControl<RouteHintRightKind>,
        >;

        let inner_left = g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, Msg<110, u8>, 0>(),
        );
        let inner_right = g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, Msg<111, u8>, 0>(),
        );
        let inner_route = g::route(inner_left, inner_right);
        let continue_arm = g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueMsg, 0>(),
            inner_route,
        );
        let break_arm = g::send::<Role<0>, Role<0>, LoopBreakMsg, 0>();
        let loop_program = g::route(continue_arm, break_arm);
        let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
            project(&loop_program);

        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1006);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
            .expect("attach controller endpoint");

        let (controller, continue_meta) = controller
            .prepare_flow::<LoopContinueMsg>()
            .expect("open loop continue send");
        let waker = noop_waker_ref();
        let mut cx = Context::from_waker(waker);
        let mut continue_send = pin!(controller.send_with_meta::<LoopContinueMsg>(&continue_meta, None));
        let controller = match continue_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok((endpoint, _))) => endpoint,
            Poll::Ready(Err(err)) => panic!("loop continue send failed: {err:?}"),
            Poll::Pending => panic!("loop continue send unexpectedly pending"),
        };

        let (controller, route_right_meta) = controller
            .prepare_flow::<StaticRouteRightMsg>()
            .expect("open nested route-right send after continue");
        let offer_lane = controller.port_for_lane(route_right_meta.lane as usize).lane();
        let policy = controller
            .control
            .cluster()
            .expect("cluster must remain attached")
            .policy_mode_for(
                RendezvousId::new(controller.rendezvous_id().raw()),
                Lane::new(offer_lane.raw()),
                route_right_meta.eff_index,
                RouteHintRightKind::TAG,
            )
            .expect("resolve route-right policy mode");
        let controller_policy = controller
            .cursor
            .route_scope_controller_policy(route_right_meta.scope);

        assert!(!route_right_meta.scope.is_none(), "nested route-right send must stay scoped");
        assert_eq!(
            route_right_meta.route_arm,
            Some(1),
            "nested route-right send must preserve the selected inner arm after loop continue: meta={route_right_meta:?} policy={policy:?} controller_policy={controller_policy:?}"
        );
        assert!(
            controller
                .canonical_control_token::<RouteHintRightKind>(&route_right_meta)
                .map(|token| token.into_bytes())
                .is_ok(),
            "nested route-right canonical mint must succeed after loop continue: meta={route_right_meta:?} policy={policy:?} controller_policy={controller_policy:?} cursor_idx={} node_scope={:?}",
            controller.cursor.index(),
            controller.cursor.node_scope_id(),
        );

        let mut route_right_send =
            pin!(controller.send_with_meta::<StaticRouteRightMsg>(&route_right_meta, None));
        match route_right_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok((_endpoint, _))) => {}
            Poll::Ready(Err(err)) => panic!(
                "nested route-right send failed after loop continue: {err:?}; meta={route_right_meta:?} policy={policy:?} controller_policy={controller_policy:?}"
            ),
            Poll::Pending => panic!("nested route-right send unexpectedly pending"),
        }
    }

    #[test]
    fn passive_offer_descends_into_nested_route_after_loop_continue_and_custom_route_right() {
        type LoopContinueMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_CONTINUE },
            GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
        >;
        type LoopBreakMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_BREAK },
            GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
        >;
        type StaticRouteLeftMsg = Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg = Msg<
            99,
            GenericCapToken<RouteHintRightKind>,
            CanonicalControl<RouteHintRightKind>,
        >;
        const RIGHT_REPLY_LABEL: u8 = 0x51;

        let inner_left = g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
            g::send::<Role<0>, Role<1>, Msg<110, u8>, 0>(),
        );
        let inner_right = g::seq(
            g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
            g::send::<Role<0>, Role<1>, Msg<RIGHT_REPLY_LABEL, u8>, 0>(),
        );
        let inner_route = g::route(inner_left, inner_right);
        let continue_arm = g::seq(
            g::send::<Role<0>, Role<0>, LoopContinueMsg, 0>(),
            inner_route,
        );
        let break_arm = g::send::<Role<0>, Role<0>, LoopBreakMsg, 0>();
        let loop_program = g::route(continue_arm, break_arm);
        let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
            project(&loop_program);
        let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
            project(&loop_program);

        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1007);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
            .expect("attach controller endpoint");
        let worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(
                rv_id,
                sid,
                &worker_program,
                TestBinding::with_incoming(&[IncomingClassification {
                    label: RIGHT_REPLY_LABEL,
                    instance: 1,
                    has_fin: false,
                    channel: Channel::new(7),
                }]),
            )
            .expect("attach worker endpoint");

        let (controller, continue_meta) = controller
            .prepare_flow::<LoopContinueMsg>()
            .expect("open loop continue");
        let waker = noop_waker_ref();
        let mut cx = Context::from_waker(waker);
        let mut continue_send = pin!(controller.send_with_meta::<LoopContinueMsg>(&continue_meta, None));
        let controller = match continue_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok((endpoint, _))) => endpoint,
            Poll::Ready(Err(err)) => panic!("loop continue send failed: {err:?}"),
            Poll::Pending => panic!("loop continue send unexpectedly pending"),
        };
        let (controller, route_right_meta) = controller
            .prepare_flow::<StaticRouteRightMsg>()
            .expect("open nested route-right");
        let mut route_right_send =
            pin!(controller.send_with_meta::<StaticRouteRightMsg>(&route_right_meta, None));
        let _controller = match route_right_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok((endpoint, _))) => endpoint,
            Poll::Ready(Err(err)) => panic!("route-right send failed: {err:?}"),
            Poll::Pending => panic!("route-right send unexpectedly pending"),
        };

        let outer_scope = worker.cursor.node_scope_id();
        let outer_ack = worker.peek_scope_ack(outer_scope);
        let outer_ready_mask = worker.scope_ready_arm_mask(outer_scope);
        let outer_poll_ready_mask = worker.scope_poll_ready_arm_mask(outer_scope);
        let mut offer = pin!(worker.offer());
        let branch = match offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!(
                "passive nested offer failed: {err:?}; outer_scope={outer_scope:?} ack={:?} ready_mask=0b{:02b} poll_ready_mask=0b{:02b}",
                outer_ack,
                outer_ready_mask,
                outer_poll_ready_mask,
            ),
            Poll::Pending => match offer.as_mut().poll(&mut cx) {
                Poll::Ready(Ok(branch)) => branch,
                Poll::Ready(Err(err)) => panic!(
                    "passive nested offer failed after retry: {err:?}; outer_scope={outer_scope:?} ack={:?} ready_mask=0b{:02b} poll_ready_mask=0b{:02b}",
                    outer_ack,
                    outer_ready_mask,
                    outer_poll_ready_mask,
                ),
                Poll::Pending => panic!("passive nested offer remained pending"),
            },
        };
        assert_eq!(
            branch.label(),
            RIGHT_REPLY_LABEL,
            "passive offer must descend into the nested right arm after continue + route-right"
        );
    }

    #[test]
    fn loop_continue_request_then_triple_nested_reply_route_keeps_client_offer_and_server_offer_valid()
    {
        type LoopContinueMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_CONTINUE },
            GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
        >;
        type LoopBreakMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_BREAK },
            GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
        >;
        type SessionRequestWireMsg = Msg<0x10, u8>;
        type AdminReplyMsg = Msg<0x50, u8>;
        type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
        type SnapshotRejectedReplyMsg = Msg<0x52, u8>;
        type CommitCandidatesReplyMsg = Msg<0x53, u8>;
        type CommitFinalReplyMsg = Msg<0x55, u8>;
        type CheckpointMsg = Msg<
            { CheckpointKind::LABEL },
            GenericCapToken<CheckpointKind>,
            CanonicalControl<CheckpointKind>,
        >;
        type SessionCancelControlMsg = Msg<
            { CancelKind::LABEL },
            GenericCapToken<CancelKind>,
            CanonicalControl<CancelKind>,
        >;
        type StaticRouteLeftMsg = Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg = Msg<
            99,
            GenericCapToken<RouteHintRightKind>,
            CanonicalControl<RouteHintRightKind>,
        >;

        let snapshot_reply_decision = g::route(
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                    g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                ),
            ),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<0>, SnapshotRejectedReplyMsg, 3>(),
                    g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                ),
            ),
        );
        let commit_reply_decision = g::route(
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<0>, CommitCandidatesReplyMsg, 3>(),
                    g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                ),
            ),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<0>, CommitFinalReplyMsg, 3>(),
                    g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                ),
            ),
        );
        let reply_decision = g::route(
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
            ),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                g::route(
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                        snapshot_reply_decision,
                    ),
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                        commit_reply_decision,
                    ),
                ),
            ),
        );
        let request_exchange = g::seq(
            g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
            reply_decision,
        );
        let loop_program = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
                request_exchange,
            ),
            g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
        );
        let client_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
            project(&loop_program);
        let server_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
            project(&loop_program);

        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 4096];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1008);
        let reply_payload = 0x51u8;
        let commit_reply_payload = 0x53u8;
        let client = cluster_ref
            .attach_endpoint::<0, _, _, _>(
                rv_id,
                sid,
                &client_program,
                TestBinding::with_incoming_and_payloads(
                    &[IncomingClassification {
                        label: 0x51,
                        instance: 11,
                        has_fin: false,
                        channel: Channel::new(9),
                    }],
                    &[&[reply_payload], &[commit_reply_payload]],
                ),
            )
            .expect("attach client endpoint");
        let server = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &server_program, NoBinding)
            .expect("attach server endpoint");

        let waker = noop_waker_ref();
        let mut cx = Context::from_waker(waker);

        let mut continue_send = pin!(
            client
                .flow::<LoopContinueMsg>()
                .expect("open client continue")
                .send(())
        );
        let (client, _) = match continue_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client continue send failed: {err:?}"),
            Poll::Pending => panic!("client continue send unexpectedly pending"),
        };
        let request_payload = 7u8;
        let mut request_send = pin!(
            client
                .flow::<SessionRequestWireMsg>()
                .expect("open client request")
                .send(&request_payload)
        );
        let (client, _) = match request_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client request send failed: {err:?}"),
            Poll::Pending => panic!("client request send unexpectedly pending"),
        };

        let mut server_offer = pin!(server.offer());
        let branch = match server_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("server request offer failed: {err:?}"),
            Poll::Pending => panic!("server request offer unexpectedly pending"),
        };
        assert_eq!(branch.label(), 0x10, "server must first observe the request");
        let mut server_decode = pin!(branch.decode::<SessionRequestWireMsg>());
        let (server, _request) = match server_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("server request decode failed: {err:?}"),
            Poll::Pending => panic!("server request decode unexpectedly pending"),
        };

        let mut reply_route_right = pin!(
            server
                .flow::<StaticRouteRightMsg>()
                .expect("open outer reply route-right")
                .send(())
        );
        let (server, _) = match reply_route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("outer reply route-right send failed: {err:?}"),
            Poll::Pending => panic!("outer reply route-right unexpectedly pending"),
        };
        let first_category_cursor_idx = server.cursor.index();
        let first_category_node_scope = server.cursor.node_scope_id();
        let first_category_local_meta = server.cursor.try_local_meta();
        let first_category_window = [0usize, 1, 2, 3, 4, 5, 6, 7].map(|offset| {
            let idx = first_category_cursor_idx + offset;
            let cursor = server.cursor.with_index(idx);
            (
                idx,
                cursor.node_scope_id(),
                cursor.label(),
                cursor.try_local_meta(),
                cursor.try_recv_meta(),
                cursor.jump_reason(),
            )
        });
        let mut category_route_left = pin!(
            server
                .flow::<StaticRouteLeftMsg>()
                .expect("open category route-left")
                .send(())
        );
        let (server, _) = match category_route_left.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("category route-left send failed: {err:?}"),
            Poll::Pending => panic!("category route-left unexpectedly pending"),
        };
        let mut snapshot_route_left = pin!(
            server
                .flow::<StaticRouteLeftMsg>()
                .expect("open snapshot route-left")
                .send(())
        );
        let (server, _) = match snapshot_route_left.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("snapshot route-left send failed: {err:?}"),
            Poll::Pending => panic!("snapshot route-left unexpectedly pending"),
        };
        let mut reply_send = pin!(
            server
                .flow::<SnapshotCandidatesReplyMsg>()
                .expect("open snapshot candidates reply")
                .send(&reply_payload)
        );
        let (server, _) = match reply_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("snapshot candidates reply send failed: {err:?}"),
            Poll::Pending => panic!("snapshot candidates reply unexpectedly pending"),
        };

        let mut client_offer = pin!(client.offer());
        let reply_branch = match client_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("client snapshot reply offer failed: {err:?}"),
            Poll::Pending => panic!("client snapshot reply offer unexpectedly pending"),
        };
        assert_eq!(
            reply_branch.label(),
            0x51,
            "client must materialize the selected snapshot candidates reply label"
        );
        let reply_branch_scope = reply_branch.branch_meta.scope_id;
        let reply_branch_scope_parent = reply_branch
            .endpoint
            .cursor
            .scope_parent(reply_branch_scope)
            .filter(|scope| scope.kind() == ScopeKind::Route);
        let reply_branch_scope_grandparent = reply_branch_scope_parent
            .and_then(|scope| reply_branch.endpoint.cursor.scope_parent(scope))
            .filter(|scope| scope.kind() == ScopeKind::Route);
        let reply_branch_selected_arm = reply_branch.branch_meta.selected_arm;
        let reply_branch_kind = reply_branch.branch_meta.kind;
        let reply_branch_cursor_idx = reply_branch.endpoint.cursor.index();
        let reply_branch_node_scope = reply_branch.endpoint.cursor.node_scope_id();
        let reply_branch_recv_meta = reply_branch.endpoint.cursor.try_recv_meta();
        let reply_branch_next_local_meta = reply_branch_recv_meta.and_then(|meta| {
            reply_branch
                .endpoint
                .cursor
                .with_index(meta.next)
                .try_local_meta()
        });
        let reply_branch_next_cursor = reply_branch_recv_meta.map(|meta| {
            let cursor = reply_branch.endpoint.cursor.with_index(meta.next);
            (
                cursor.index(),
                cursor.node_scope_id(),
                cursor.label(),
                cursor.is_jump(),
                cursor.jump_reason(),
                cursor.is_local_action(),
                cursor.is_recv(),
            )
        });
        let mut client_decode = pin!(reply_branch.decode::<SnapshotCandidatesReplyMsg>());
        let (client, _reply) = match client_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client snapshot reply decode failed: {err:?}"),
            Poll::Pending => panic!("client snapshot reply decode unexpectedly pending"),
        };

        let client_cursor_idx = client.cursor.index();
        let client_node_scope = client.cursor.node_scope_id();
        let client_is_send = client.cursor.is_send();
        let client_is_recv = client.cursor.is_recv();
        let client_is_local_action = client.cursor.is_local_action();
        let client_local_meta = client.cursor.try_local_meta();
        let client_recv_meta = client.cursor.try_recv_meta();
        let checkpoint_flow = client.flow::<CheckpointMsg>();
        let checkpoint_flow = match checkpoint_flow {
            Ok(flow) => flow,
            Err(err) => panic!(
                "open client checkpoint control failed: {err:?}; branch_scope={reply_branch_scope:?} branch_arm={reply_branch_selected_arm} branch_kind={reply_branch_kind:?} branch_cursor_idx={reply_branch_cursor_idx} branch_node_scope={reply_branch_node_scope:?} branch_recv_meta={reply_branch_recv_meta:?} branch_next_local_meta={reply_branch_next_local_meta:?} branch_next_cursor={reply_branch_next_cursor:?}; cursor_idx={} node_scope={:?} is_send={} is_recv={} is_local_action={} local_meta={:?} recv_meta={:?}",
                client_cursor_idx,
                client_node_scope,
                client_is_send,
                client_is_recv,
                client_is_local_action,
                client_local_meta,
                client_recv_meta,
            ),
        };
        let mut checkpoint_send = pin!(checkpoint_flow.send(()));
        let (client, _) = match checkpoint_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client checkpoint control send failed: {err:?}"),
            Poll::Pending => panic!("client checkpoint control unexpectedly pending"),
        };
        assert_eq!(
            client.selected_arm_for_scope(reply_branch_scope),
            None,
            "completed non-linger branch scope must not survive into next loop iteration: lane3_len={}; lane3_stack={:?}; branch_scope={reply_branch_scope:?}; parent={reply_branch_scope_parent:?}; grandparent={reply_branch_scope_grandparent:?}",
            client.lane_route_arm_lens[3],
            &client.lane_route_arms[3][..client.lane_route_arm_lens[3] as usize],
        );

        let mut continue_send = pin!(
            client
                .flow::<LoopContinueMsg>()
                .expect("open client continue for second iteration")
                .send(())
        );
        let (client, _) = match continue_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client second continue send failed: {err:?}"),
            Poll::Pending => panic!("client second continue send unexpectedly pending"),
        };
        let request_payload = 8u8;
        let mut request_send = pin!(
            client
                .flow::<SessionRequestWireMsg>()
                .expect("open client commit request")
                .send(&request_payload)
        );
        let (client, _) = match request_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client commit request send failed: {err:?}"),
            Poll::Pending => panic!("client commit request send unexpectedly pending"),
        };

        let mut server_offer = pin!(server.offer());
        let branch = match server_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("server commit request offer failed: {err:?}"),
            Poll::Pending => panic!("server commit request offer unexpectedly pending"),
        };
        assert_eq!(branch.label(), 0x10, "server must observe the second request");
        let mut server_decode = pin!(branch.decode::<SessionRequestWireMsg>());
        let (server, _request) = match server_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("server commit request decode failed: {err:?}"),
            Poll::Pending => panic!("server commit request decode unexpectedly pending"),
        };
        let second_request_decode_cursor_idx = server.cursor.index();
        let second_request_decode_scope = server.cursor.node_scope_id();
        let second_request_decode_local_meta = server.cursor.try_local_meta();
        let second_request_decode_window = [0usize, 1, 2, 3, 4, 5].map(|offset| {
            let idx = second_request_decode_cursor_idx + offset;
            let cursor = server.cursor.with_index(idx);
            (
                idx,
                cursor.node_scope_id(),
                cursor.label(),
                cursor.try_local_meta(),
                cursor.try_recv_meta(),
                cursor.jump_reason(),
            )
        });

        let (server, outer_commit_route_right_meta) = server
            .prepare_flow::<StaticRouteRightMsg>()
            .expect("open outer commit reply route-right");
        let mut reply_route_right =
            pin!(server.send_with_meta::<StaticRouteRightMsg>(&outer_commit_route_right_meta, None));
        let (server, _) = match reply_route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("outer commit reply route-right send failed: {err:?}"),
            Poll::Pending => panic!("outer commit reply route-right unexpectedly pending"),
        };
        let category_route_right_cursor_idx = server.cursor.index();
        let category_route_right_node_scope = server.cursor.node_scope_id();
        let category_route_right_local_meta = server.cursor.try_local_meta();
        let category_route_right_arm0 = server
            .cursor
            .controller_arm_entry_by_arm(category_route_right_node_scope, 0);
        let category_route_right_arm1 = server
            .cursor
            .controller_arm_entry_by_arm(category_route_right_node_scope, 1);
        let (server, category_route_right_meta) = server
            .prepare_flow::<StaticRouteRightMsg>()
            .expect("open commit category route-right");
        let mut category_route_right =
            pin!(server.send_with_meta::<StaticRouteRightMsg>(&category_route_right_meta, None));
        let (server, _) = match category_route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("commit category route-right send failed: {err:?}"),
            Poll::Pending => panic!("commit category route-right unexpectedly pending"),
        };
        let server_cursor_idx = server.cursor.index();
        let server_node_scope = server.cursor.node_scope_id();
        let server_is_send = server.cursor.is_send();
        let server_is_recv = server.cursor.is_recv();
        let server_is_local_action = server.cursor.is_local_action();
        let server_local_meta = server.cursor.try_local_meta();
        let server_recv_meta = server.cursor.try_recv_meta();
        let server_window = [14usize, 15, 16, 17, 18, 19].map(|idx| {
            let cursor = server.cursor.with_index(idx);
            (
                idx,
                cursor.node_scope_id(),
                cursor.label(),
                cursor.try_local_meta(),
                cursor.try_recv_meta(),
                cursor.jump_reason(),
            )
        });
        let commit_route_left_flow = server.flow::<StaticRouteLeftMsg>();
        let commit_route_left_flow = match commit_route_left_flow {
            Ok(flow) => flow,
            Err(err) => panic!(
                "open commit reply route-left failed: {err:?}; first_category_cursor_idx={first_category_cursor_idx} first_category_node_scope={first_category_node_scope:?} first_category_local_meta={first_category_local_meta:?} first_category_window={first_category_window:?}; second_request_decode_cursor_idx={second_request_decode_cursor_idx} second_request_decode_scope={second_request_decode_scope:?} second_request_decode_local_meta={second_request_decode_local_meta:?} second_request_decode_window={second_request_decode_window:?}; outer_commit_route_right_meta={outer_commit_route_right_meta:?}; category_route_right_cursor_idx={category_route_right_cursor_idx} category_route_right_node_scope={category_route_right_node_scope:?} category_route_right_local_meta={category_route_right_local_meta:?} category_route_right_arm0={category_route_right_arm0:?} category_route_right_arm1={category_route_right_arm1:?} category_route_right_meta={category_route_right_meta:?} server_window={server_window:?}; cursor_idx={} node_scope={:?} is_send={} is_recv={} is_local_action={} local_meta={:?} recv_meta={:?}",
                server_cursor_idx,
                server_node_scope,
                server_is_send,
                server_is_recv,
                server_is_local_action,
                server_local_meta,
                server_recv_meta,
            ),
        };
        let mut commit_route_left = pin!(commit_route_left_flow.send(()));
        let (server, _) = match commit_route_left.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("commit reply route-left send failed: {err:?}"),
            Poll::Pending => panic!("commit reply route-left unexpectedly pending"),
        };
        let mut commit_reply_send = pin!(
            server
                .flow::<CommitCandidatesReplyMsg>()
                .expect("open commit candidates reply")
                .send(&commit_reply_payload)
        );
        let (server, _) = match commit_reply_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("commit candidates reply send failed: {err:?}"),
            Poll::Pending => panic!("commit candidates reply unexpectedly pending"),
        };

        let mut client_offer = pin!(client.offer());
        let commit_branch = match client_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("client commit reply offer failed: {err:?}"),
            Poll::Pending => panic!("client commit reply offer unexpectedly pending"),
        };
        assert_eq!(
            commit_branch.label(),
            0x53,
            "client must materialize the selected commit candidates reply label"
        );
        let mut client_decode = pin!(commit_branch.decode::<CommitCandidatesReplyMsg>());
        let (client, _reply) = match client_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client commit reply decode failed: {err:?}"),
            Poll::Pending => panic!("client commit reply decode unexpectedly pending"),
        };

        let mut checkpoint_send = pin!(
            client
                .flow::<CheckpointMsg>()
                .expect("open client checkpoint control after commit reply")
                .send(())
        );
        let (_client, _) = match checkpoint_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client post-commit checkpoint control failed: {err:?}"),
            Poll::Pending => panic!("client post-commit checkpoint unexpectedly pending"),
        };

        let mut server_next_offer = pin!(server.offer());
        match server_next_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Err(err)) => {
                panic!("server next offer after commit path must not fail: {err:?}")
            }
            Poll::Ready(Ok(branch)) => panic!(
                "server next offer after commit path must not spuriously materialize a branch: label={}",
                branch.label()
            ),
            Poll::Pending => {}
        }
    }

    #[test]
    fn admin_reply_then_snapshot_reply_right_path_survives_next_iteration() {
        type LoopContinueMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_CONTINUE },
            GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
        >;
        type LoopBreakMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_BREAK },
            GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
        >;
        type SessionRequestWireMsg = Msg<0x10, u8>;
        type AdminReplyMsg = Msg<0x50, u8>;
        type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
        type CheckpointMsg = Msg<
            { CheckpointKind::LABEL },
            GenericCapToken<CheckpointKind>,
            CanonicalControl<CheckpointKind>,
        >;
        type StaticRouteLeftMsg = Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg = Msg<
            99,
            GenericCapToken<RouteHintRightKind>,
            CanonicalControl<RouteHintRightKind>,
        >;

        let reply_decision = g::route(
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
            ),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                        g::seq(
                            g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                            g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                        ),
                    ),
                ),
            ),
        );
        let request_exchange = g::seq(
            g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
            reply_decision,
        );
        let loop_program = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
                request_exchange,
            ),
            g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
        );
        let client_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
            project(&loop_program);
        let server_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
            project(&loop_program);

        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 4096];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1010);
        let admin_reply_payload = 0x50u8;
        let snapshot_reply_payload = 0x51u8;
        let client = cluster_ref
            .attach_endpoint::<0, _, _, _>(
                rv_id,
                sid,
                &client_program,
                TestBinding::with_incoming_and_payloads(
                    &[
                        IncomingClassification {
                            label: 0x50,
                            instance: 21,
                            has_fin: false,
                            channel: Channel::new(13),
                        },
                        IncomingClassification {
                            label: 0x51,
                            instance: 22,
                            has_fin: false,
                            channel: Channel::new(14),
                        },
                    ],
                    &[&[admin_reply_payload], &[snapshot_reply_payload]],
                ),
            )
            .expect("attach client endpoint");
        let server = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &server_program, NoBinding)
            .expect("attach server endpoint");

        let waker = noop_waker_ref();
        let mut cx = Context::from_waker(waker);

        let mut continue_send = pin!(
            client
                .flow::<LoopContinueMsg>()
                .expect("open client continue")
                .send(())
        );
        let (client, _) = match continue_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client continue send failed: {err:?}"),
            Poll::Pending => panic!("client continue send unexpectedly pending"),
        };
        let mut request_send = pin!(
            client
                .flow::<SessionRequestWireMsg>()
                .expect("open client admin request")
                .send(&1u8)
        );
        let (client, _) = match request_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client admin request send failed: {err:?}"),
            Poll::Pending => panic!("client admin request send unexpectedly pending"),
        };

        let mut server_offer = pin!(server.offer());
        let branch = match server_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("server admin request offer failed: {err:?}"),
            Poll::Pending => panic!("server admin request offer unexpectedly pending"),
        };
        assert_eq!(branch.label(), 0x10, "server must first observe the admin request");
        let mut server_decode = pin!(branch.decode::<SessionRequestWireMsg>());
        let (server, _) = match server_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("server admin request decode failed: {err:?}"),
            Poll::Pending => panic!("server admin request decode unexpectedly pending"),
        };
        let mut admin_route_left = pin!(
            server
                .flow::<StaticRouteLeftMsg>()
                .expect("open admin route-left")
                .send(())
        );
        let (server, _) = match admin_route_left.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("admin route-left send failed: {err:?}"),
            Poll::Pending => panic!("admin route-left unexpectedly pending"),
        };
        let mut admin_reply_send = pin!(
            server
                .flow::<AdminReplyMsg>()
                .expect("open admin reply")
                .send(&admin_reply_payload)
        );
        let (server, _) = match admin_reply_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("admin reply send failed: {err:?}"),
            Poll::Pending => panic!("admin reply unexpectedly pending"),
        };

        let mut client_offer = pin!(client.offer());
        let admin_branch = match client_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("client admin reply offer failed: {err:?}"),
            Poll::Pending => panic!("client admin reply offer unexpectedly pending"),
        };
        assert_eq!(admin_branch.label(), 0x50, "client must materialize the admin reply");
        let admin_reply_scope = admin_branch.branch_meta.scope_id;
        let mut admin_decode = pin!(admin_branch.decode::<AdminReplyMsg>());
        let (client, _) = match admin_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client admin reply decode failed: {err:?}"),
            Poll::Pending => panic!("client admin reply decode unexpectedly pending"),
        };
        assert_eq!(
            client.selected_arm_for_scope(admin_reply_scope),
            None,
            "admin reply branch scope must not survive into the next loop iteration"
        );

        let mut continue_send = pin!(
            client
                .flow::<LoopContinueMsg>()
                .expect("open client continue for snapshot")
                .send(())
        );
        let (client, _) = match continue_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client snapshot continue send failed: {err:?}"),
            Poll::Pending => panic!("client snapshot continue unexpectedly pending"),
        };
        let mut request_send = pin!(
            client
                .flow::<SessionRequestWireMsg>()
                .expect("open client snapshot request")
                .send(&2u8)
        );
        let (client, _) = match request_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client snapshot request send failed: {err:?}"),
            Poll::Pending => panic!("client snapshot request unexpectedly pending"),
        };

        let mut server_offer = pin!(server.offer());
        let branch = match server_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("server snapshot request offer failed: {err:?}"),
            Poll::Pending => panic!("server snapshot request offer unexpectedly pending"),
        };
        assert_eq!(branch.label(), 0x10, "server must observe the snapshot request");
        let mut server_decode = pin!(branch.decode::<SessionRequestWireMsg>());
        let (server, _) = match server_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("server snapshot request decode failed: {err:?}"),
            Poll::Pending => panic!("server snapshot request decode unexpectedly pending"),
        };
        let mut outer_route_right = pin!(
            server
                .flow::<StaticRouteRightMsg>()
                .expect("open snapshot outer route-right")
                .send(())
        );
        let (server, _) = match outer_route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("snapshot outer route-right send failed: {err:?}"),
            Poll::Pending => panic!("snapshot outer route-right unexpectedly pending"),
        };
        let mut category_route_left = pin!(
            server
                .flow::<StaticRouteLeftMsg>()
                .expect("open snapshot category route-left")
                .send(())
        );
        let (server, _) = match category_route_left.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("snapshot category route-left send failed: {err:?}"),
            Poll::Pending => panic!("snapshot category route-left unexpectedly pending"),
        };
        let mut snapshot_route_left = pin!(
            server
                .flow::<StaticRouteLeftMsg>()
                .expect("open snapshot reply route-left")
                .send(())
        );
        let (server, _) = match snapshot_route_left.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("snapshot reply route-left send failed: {err:?}"),
            Poll::Pending => panic!("snapshot reply route-left unexpectedly pending"),
        };
        let mut snapshot_reply_send = pin!(
            server
                .flow::<SnapshotCandidatesReplyMsg>()
                .expect("open snapshot candidates reply")
                .send(&snapshot_reply_payload)
        );
        let (server, _) = match snapshot_reply_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("snapshot candidates reply send failed: {err:?}"),
            Poll::Pending => panic!("snapshot candidates reply unexpectedly pending"),
        };

        let mut client_offer = pin!(client.offer());
        let snapshot_branch = match client_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("client snapshot reply offer failed after admin path: {err:?}"),
            Poll::Pending => panic!("client snapshot reply offer unexpectedly pending after admin path"),
        };
        assert_eq!(
            snapshot_branch.label(),
            0x51,
            "snapshot reply must still materialize after an earlier admin-left iteration"
        );
        let mut snapshot_decode = pin!(snapshot_branch.decode::<SnapshotCandidatesReplyMsg>());
        let (client, _) = match snapshot_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client snapshot reply decode failed after admin path: {err:?}"),
            Poll::Pending => panic!("client snapshot reply decode unexpectedly pending after admin path"),
        };
        let mut checkpoint_send = pin!(
            client
                .flow::<CheckpointMsg>()
                .expect("open snapshot checkpoint after admin path")
                .send(())
        );
        let (_client, _) = match checkpoint_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client snapshot checkpoint send failed after admin path: {err:?}"),
            Poll::Pending => panic!("client snapshot checkpoint unexpectedly pending after admin path"),
        };

        drop(server);
    }

    #[test]
    fn snapshot_then_commit_final_reply_survives_next_iteration() {
        type LoopContinueMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_CONTINUE },
            GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopContinueKind>,
        >;
        type LoopBreakMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_BREAK },
            GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
            CanonicalControl<crate::control::cap::resource_kinds::LoopBreakKind>,
        >;
        type SessionRequestWireMsg = Msg<0x10, u8>;
        type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
        type CommitCandidatesReplyMsg = Msg<0x53, u8>;
        type CommitRejectedReplyMsg = Msg<0x54, u8>;
        type CommitFinalReplyMsg = Msg<0x55, u8>;
        type CheckpointMsg = Msg<
            { CheckpointKind::LABEL },
            GenericCapToken<CheckpointKind>,
            CanonicalControl<CheckpointKind>,
        >;
        type SessionCancelControlMsg = Msg<
            { CancelKind::LABEL },
            GenericCapToken<CancelKind>,
            CanonicalControl<CancelKind>,
        >;
        type StaticRouteLeftMsg = Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg = Msg<
            99,
            GenericCapToken<RouteHintRightKind>,
            CanonicalControl<RouteHintRightKind>,
        >;

        let snapshot_reply_decision = g::route(
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                    g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                ),
            ),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<0>, Msg<0x52, u8>, 3>(),
                    g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                ),
            ),
        );
        let commit_reply_decision = g::route(
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<0>, CommitCandidatesReplyMsg, 3>(),
                    g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                ),
            ),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                g::route(
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                        g::seq(
                            g::send::<Role<1>, Role<0>, CommitRejectedReplyMsg, 3>(),
                            g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                        ),
                    ),
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                        g::seq(
                            g::send::<Role<1>, Role<0>, CommitFinalReplyMsg, 3>(),
                            g::send::<Role<0>, Role<0>, SessionCancelControlMsg, 3>(),
                        ),
                    ),
                ),
            ),
        );
        let reply_decision = g::route(
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::send::<Role<1>, Role<0>, Msg<0x50, u8>, 3>(),
            ),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                g::route(
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                        snapshot_reply_decision,
                    ),
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                        commit_reply_decision,
                    ),
                ),
            ),
        );
        let request_exchange = g::seq(
            g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
            reply_decision,
        );
        let loop_program = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
                request_exchange,
            ),
            g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
        );
        let client_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
            project(&loop_program);
        let server_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
            project(&loop_program);

        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 4096];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1012);
        let snapshot_reply_payload = 0x51u8;
        let commit_final_payload = 0x55u8;
        let client = cluster_ref
            .attach_endpoint::<0, _, _, _>(
                rv_id,
                sid,
                &client_program,
                TestBinding::with_incoming_and_payloads(
                    &[
                        IncomingClassification {
                            label: 0x51,
                            instance: 41,
                            has_fin: false,
                            channel: Channel::new(17),
                        },
                        IncomingClassification {
                            label: 0x55,
                            instance: 42,
                            has_fin: false,
                            channel: Channel::new(18),
                        },
                    ],
                    &[&[snapshot_reply_payload], &[commit_final_payload]],
                ),
            )
            .expect("attach client endpoint");
        let server = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &server_program, NoBinding)
            .expect("attach server endpoint");

        let waker = noop_waker_ref();
        let mut cx = Context::from_waker(waker);

        let mut continue_send = pin!(
            client
                .flow::<LoopContinueMsg>()
                .expect("open first continue")
                .send(())
        );
        let (client, _) = match continue_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("first continue failed: {err:?}"),
            Poll::Pending => panic!("first continue unexpectedly pending"),
        };
        let mut request_send = pin!(
            client
                .flow::<SessionRequestWireMsg>()
                .expect("open first request")
                .send(&1u8)
        );
        let (client, _) = match request_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("first request failed: {err:?}"),
            Poll::Pending => panic!("first request unexpectedly pending"),
        };

        let mut server_offer = pin!(server.offer());
        let branch = match server_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("server first request offer failed: {err:?}"),
            Poll::Pending => panic!("server first request offer unexpectedly pending"),
        };
        assert_eq!(branch.label(), 0x10);
        let mut server_decode = pin!(branch.decode::<SessionRequestWireMsg>());
        let (server, _) = match server_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("server first request decode failed: {err:?}"),
            Poll::Pending => panic!("server first request decode unexpectedly pending"),
        };

        let mut route_right = pin!(
            server
                .flow::<StaticRouteRightMsg>()
                .expect("open first outer route-right")
                .send(())
        );
        let (server, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("first outer route-right failed: {err:?}"),
            Poll::Pending => panic!("first outer route-right unexpectedly pending"),
        };
        let mut route_left = pin!(
            server
                .flow::<StaticRouteLeftMsg>()
                .expect("open first category route-left")
                .send(())
        );
        let (server, _) = match route_left.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("first category route-left failed: {err:?}"),
            Poll::Pending => panic!("first category route-left unexpectedly pending"),
        };
        let mut route_left = pin!(
            server
                .flow::<StaticRouteLeftMsg>()
                .expect("open first snapshot route-left")
                .send(())
        );
        let (server, _) = match route_left.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("first snapshot route-left failed: {err:?}"),
            Poll::Pending => panic!("first snapshot route-left unexpectedly pending"),
        };
        let mut reply_send = pin!(
            server
                .flow::<SnapshotCandidatesReplyMsg>()
                .expect("open first snapshot reply")
                .send(&snapshot_reply_payload)
        );
        let (server, _) = match reply_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("first snapshot reply failed: {err:?}"),
            Poll::Pending => panic!("first snapshot reply unexpectedly pending"),
        };

        let mut client_offer = pin!(client.offer());
        let branch = match client_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("client first offer failed: {err:?}"),
            Poll::Pending => panic!("client first offer unexpectedly pending"),
        };
        assert_eq!(branch.label(), 0x51);
        let branch_scope = branch.branch_meta.scope_id;
        let mut client_decode = pin!(branch.decode::<SnapshotCandidatesReplyMsg>());
        let (client, _) = match client_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client first decode failed: {err:?}"),
            Poll::Pending => panic!("client first decode unexpectedly pending"),
        };
        let mut checkpoint_send = pin!(
            client
                .flow::<CheckpointMsg>()
                .expect("open checkpoint after snapshot")
                .send(())
        );
        let (client, _) = match checkpoint_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("snapshot checkpoint failed: {err:?}"),
            Poll::Pending => panic!("snapshot checkpoint unexpectedly pending"),
        };
        assert_eq!(
            client.selected_arm_for_scope(branch_scope),
            None,
            "completed snapshot branch scope must not survive into the next iteration"
        );

        let mut continue_send = pin!(
            client
                .flow::<LoopContinueMsg>()
                .expect("open second continue")
                .send(())
        );
        let (client, _) = match continue_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("second continue failed: {err:?}"),
            Poll::Pending => panic!("second continue unexpectedly pending"),
        };
        let mut request_send = pin!(
            client
                .flow::<SessionRequestWireMsg>()
                .expect("open second request")
                .send(&2u8)
        );
        let (client, _) = match request_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("second request failed: {err:?}"),
            Poll::Pending => panic!("second request unexpectedly pending"),
        };

        let mut server_offer = pin!(server.offer());
        let branch = match server_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("server second request offer failed: {err:?}"),
            Poll::Pending => panic!("server second request offer unexpectedly pending"),
        };
        assert_eq!(branch.label(), 0x10);
        let mut server_decode = pin!(branch.decode::<SessionRequestWireMsg>());
        let (server, _) = match server_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("server second request decode failed: {err:?}"),
            Poll::Pending => panic!("server second request decode unexpectedly pending"),
        };

        let mut route_right = pin!(
            server
                .flow::<StaticRouteRightMsg>()
                .expect("open second outer route-right")
                .send(())
        );
        let (server, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("second outer route-right failed: {err:?}"),
            Poll::Pending => panic!("second outer route-right unexpectedly pending"),
        };
        let mut route_right = pin!(
            server
                .flow::<StaticRouteRightMsg>()
                .expect("open second category route-right")
                .send(())
        );
        let (server, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("second category route-right failed: {err:?}"),
            Poll::Pending => panic!("second category route-right unexpectedly pending"),
        };
        let mut route_right = pin!(
            server
                .flow::<StaticRouteRightMsg>()
                .expect("open second commit tail route-right")
                .send(())
        );
        let (server, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("second commit tail route-right failed: {err:?}"),
            Poll::Pending => panic!("second commit tail route-right unexpectedly pending"),
        };
        let mut route_right = pin!(
            server
                .flow::<StaticRouteRightMsg>()
                .expect("open second commit final route-right")
                .send(())
        );
        let (server, _) = match route_right.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("second commit final route-right failed: {err:?}"),
            Poll::Pending => panic!("second commit final route-right unexpectedly pending"),
        };
        let mut reply_send = pin!(
            server
                .flow::<CommitFinalReplyMsg>()
                .expect("open second commit final reply")
                .send(&commit_final_payload)
        );
        let (_server, _) = match reply_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("second commit final reply failed: {err:?}"),
            Poll::Pending => panic!("second commit final reply unexpectedly pending"),
        };

        let mut client_offer = pin!(client.offer());
        let branch = match client_offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => branch,
            Poll::Ready(Err(err)) => panic!("client second offer failed: {err:?}"),
            Poll::Pending => panic!("client second offer unexpectedly pending"),
        };
        assert_eq!(branch.label(), 0x55);
        let mut client_decode = pin!(branch.decode::<CommitFinalReplyMsg>());
        let (client, _) = match client_decode.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("client second decode failed: {err:?}"),
            Poll::Pending => panic!("client second decode unexpectedly pending"),
        };
        let mut cancel_send = pin!(
            client
                .flow::<SessionCancelControlMsg>()
                .expect("open cancel after commit final")
                .send(())
        );
        let (_client, _) = match cancel_send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(err)) => panic!("commit final cancel failed: {err:?}"),
            Poll::Pending => panic!("commit final cancel unexpectedly pending"),
        };
    }

    fn static_passive_offer_with_known_arm_waits_on_transport_without_busy_restart() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, PendingTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = PendingTransport::new();
        let transport_probe = transport.clone();
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1201);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &ENTRY_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &ENTRY_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        controller.port_for_lane(0).record_route_decision(scope, 1);

        let waker = noop_waker_ref();
        let mut cx = Context::from_waker(waker);
        let mut offer = pin!(worker.offer());
        match offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => {
                panic!("offer must not materialize before transport ingress: {}", branch.label())
            }
            Poll::Ready(Err(err)) => panic!("offer must wait for transport ingress: {err:?}"),
            Poll::Pending => {}
        }
        assert_eq!(
            transport_probe.poll_count(),
            1,
            "known static passive arm must park on transport once instead of frontier-restarting"
        );
    }

    #[test]
    fn nested_dispatch_arm_counts_as_recv_for_known_passive_route() {
        type OuterLeftMsg = Msg<0x10, u8>;
        type LeafLeftMsg = Msg<0x51, u8>;
        type LeafRightMsg = Msg<0x52, u8>;
        type StaticRouteLeftMsg = Msg<
            { LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            CanonicalControl<RouteDecisionKind>,
        >;
        type StaticRouteRightMsg =
            Msg<99, GenericCapToken<RouteHintRightKind>, CanonicalControl<RouteHintRightKind>>;

        let nested = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, LeafLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                g::send::<Role<0>, Role<1>, LeafRightMsg, 0>(),
            ),
        );
        let program = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteLeftMsg, 0>(),
                g::send::<Role<0>, Role<1>, OuterLeftMsg, 0>(),
            ),
            g::seq(
                g::send::<Role<0>, Role<0>, StaticRouteRightMsg, 0>(),
                nested,
            ),
        );
        let controller_program: RoleProgram<'_, 0, _, crate::control::cap::mint::MintConfig> =
            project(&program);
        let worker_program: RoleProgram<'_, 1, _, crate::control::cap::mint::MintConfig> =
            project(&program);

        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, PendingTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = PendingTransport::new();
        let transport_probe = transport.clone();
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(1202);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &controller_program, NoBinding)
            .expect("attach controller endpoint");
        let worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &worker_program, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        controller.port_for_lane(0).record_route_decision(scope, 1);

        assert!(
            worker.arm_has_recv(scope, 1),
            "nested first-recv dispatch must count as recv-bearing arm"
        );

        let waker = noop_waker_ref();
        let mut cx = Context::from_waker(waker);
        let mut offer = pin!(worker.offer());
        match offer.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(branch)) => panic!(
                "known passive route with nested dispatch recv must wait for wire ingress, got {}",
                branch.label()
            ),
            Poll::Ready(Err(err)) => {
                panic!("known passive route with nested dispatch recv must not fail: {err:?}")
            }
            Poll::Pending => {}
        }
        assert_eq!(
            transport_probe.poll_count(),
            1,
            "known passive route with nested dispatch recv must still poll transport once"
        );
    }

    #[test]
    fn scope_local_label_mapping_never_uses_global_scan() {
        let mut tap_storage = [TapEvent::default(); RING_EVENTS];
        let mut slab = [0u8; 2048];
        let config = Config::new(&mut tap_storage, &mut slab);
        let clock = CounterClock::new();
        let cluster: ManuallyDrop<
            SessionCluster<'_, HintOnlyTransport, DefaultLabelUniverse, CounterClock, 4>,
        > = ManuallyDrop::new(SessionCluster::new(&clock));
        let transport = HintOnlyTransport::new(HINT_NONE);
        let cluster_ref = &*cluster;
        let rv_id = cluster_ref
            .add_rendezvous_from_config(config, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(992);
        let controller = cluster_ref
            .attach_endpoint::<0, _, _, _>(rv_id, sid, &HINT_CONTROLLER_PROGRAM, NoBinding)
            .expect("attach controller endpoint");
        let mut worker = cluster_ref
            .attach_endpoint::<1, _, _, _>(rv_id, sid, &HINT_WORKER_PROGRAM, NoBinding)
            .expect("attach worker endpoint");
        let scope = worker.cursor.node_scope_id();
        assert!(!scope.is_none(), "worker must start at route scope");

        let foreign_label = (1u8..=u8::MAX).find(|label| {
            !matches!(*label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK)
                && worker.cursor.first_recv_target(scope, *label).is_none()
                && worker.cursor.find_arm_for_recv_label(*label).is_some()
        });
        let Some(foreign_label) = foreign_label else {
            // FIRST-recv dispatch can fully cover this scope; no entry-only
            // label remains to probe.
            drop(worker);
            drop(controller);
            return;
        };

        let label_meta = endpoint_scope_label_meta(&worker, scope, ScopeLoopMeta::EMPTY);
        worker.ingest_binding_scope_evidence(scope, foreign_label, false, label_meta);

        assert!(
            !worker.scope_has_ready_arm_evidence(scope),
            "foreign label {} must not become scope-local arm-ready evidence: hint={} arm={:?} evidence={:?} ready_mask=0b{:02b} controller={}",
            foreign_label,
            label_meta.matches_hint_label(foreign_label),
            label_meta.arm_for_label(foreign_label),
            label_meta.evidence_arm_for_label(foreign_label),
            worker.scope_ready_arm_mask(scope),
            worker.cursor.is_route_controller(scope)
        );
        assert!(
            worker.peek_scope_ack(scope).is_none(),
            "foreign label must not mint route authority"
        );

        drop(worker);
        drop(controller);
    }

    #[test]
    fn payload_staging_is_selected_scope_lane_stable() {
        let mut scratch = [0u8; 8];
        let src = [9u8, 8, 7, 6];
        let len = stage_transport_payload(&mut scratch, &src).expect("stage payload");
        assert_eq!(len, src.len());
        assert_eq!(&scratch[..len], &src);
    }
}

/// Internal endpoint kernel. Owns the rendezvous port as well as the lane
/// release handle. Dropping the endpoint releases the lane back to the
/// `SessionCluster` via the handle.
pub struct CursorEndpoint<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    U = crate::runtime::consts::DefaultLabelUniverse,
    C = crate::runtime::config::CounterClock,
    E: EpochTable = EpochTbl,
    const MAX_RV: usize = 8,
    Mint = crate::control::cap::mint::MintConfig,
    B: BindingSlot = NoBinding,
> where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    /// Multi-lane port array. Each active lane has its own port.
    /// For single-lane programs, only `ports[0]` is used.
    ports: [Option<Port<'r, T, E>>; MAX_LANES],
    /// Multi-lane guard array. Each active lane has its own guard.
    guards: [Option<LaneGuard<'r, T, U, C>>; MAX_LANES],
    /// Primary lane index (first active lane, typically 0).
    primary_lane: usize,
    sid: SessionId,
    _owner: Owner<'r, E0>,
    _epoch: EndpointEpoch<'r, E>,
    /// Phase-aware cursor for multi-lane parallel execution.
    #[cfg(feature = "std")]
    cursor: std::boxed::Box<PhaseCursor<ROLE>>,
    #[cfg(not(feature = "std"))]
    cursor: PhaseCursor<ROLE>,
    control: SessionControlCtx<'r, T, U, C, E, MAX_RV>,
    /// Lane-local route arm stacks for parallel composition.
    ///
    /// Each lane maintains its own route arm stack to support independent
    /// `g::route`/`g::loop` scopes within `g::par` regions. When a lane enters
    /// a route scope, only that lane's stack is pushed. This enables:
    ///
    /// - **Shared Scope (Outer)**: Route scopes outside `g::par` are pushed to
    ///   all active lanes when entering the parallel region.
    /// - **Local Scope (Inner)**: Route scopes inside `g::par` are pushed only
    ///   to the specific lane executing that scope.
    /// - **Join**: When exiting `g::par`, each lane's local scopes are popped,
    ///   returning to shared scope state.
    #[cfg(feature = "std")]
    lane_route_arms: std::boxed::Box<[[RouteArmState; MAX_ROUTE_ARM_STACK]; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    lane_route_arms: [[RouteArmState; MAX_ROUTE_ARM_STACK]; MAX_LANES],
    #[cfg(feature = "std")]
    lane_route_arm_lens: std::boxed::Box<[u8; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    lane_route_arm_lens: [u8; MAX_LANES],
    #[cfg(feature = "std")]
    lane_linger_counts: std::boxed::Box<[u8; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    lane_linger_counts: [u8; MAX_LANES],
    lane_linger_mask: u8,
    lane_offer_linger_mask: u8,
    active_offer_mask: u8,
    #[cfg(feature = "std")]
    lane_offer_state: std::boxed::Box<[LaneOfferState; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    lane_offer_state: [LaneOfferState; MAX_LANES],
    root_frontier_len: u8,
    #[cfg(feature = "std")]
    root_frontier_state: std::boxed::Box<[RootFrontierState; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    root_frontier_state: [RootFrontierState; MAX_LANES],
    #[cfg(feature = "std")]
    root_frontier_slot_by_ordinal: std::boxed::Box<[u8; ScopeId::ORDINAL_CAPACITY as usize]>,
    #[cfg(not(feature = "std"))]
    root_frontier_slot_by_ordinal: [u8; ScopeId::ORDINAL_CAPACITY as usize],
    #[cfg(feature = "std")]
    offer_entry_state: std::boxed::Box<[OfferEntryState; crate::global::typestate::MAX_STATES]>,
    #[cfg(not(feature = "std"))]
    offer_entry_state: [OfferEntryState; crate::global::typestate::MAX_STATES],
    global_active_entries: ActiveEntrySet,
    global_offer_lane_mask: u8,
    #[cfg(feature = "std")]
    global_offer_lane_entry_slot_masks: std::boxed::Box<[u8; MAX_LANES]>,
    #[cfg(not(feature = "std"))]
    global_offer_lane_entry_slot_masks: [u8; MAX_LANES],
    frontier_observation_epoch: u32,
    global_frontier_observed_epoch: u32,
    global_frontier_observed_key: FrontierObservationKey,
    global_frontier_observed: ObservedEntrySet,
    binding_inbox: BindingInbox,
    #[cfg(feature = "std")]
    scope_evidence: std::boxed::Box<[ScopeEvidence; crate::eff::meta::MAX_EFF_NODES]>,
    #[cfg(not(feature = "std"))]
    scope_evidence: [ScopeEvidence; crate::eff::meta::MAX_EFF_NODES],
    #[cfg(feature = "std")]
    scope_evidence_generations: std::boxed::Box<[u32; crate::eff::meta::MAX_EFF_NODES]>,
    #[cfg(not(feature = "std"))]
    scope_evidence_generations: [u32; crate::eff::meta::MAX_EFF_NODES],
    liveness_policy: crate::runtime::config::LivenessPolicy,
    mint: Mint,
    binding: B,
}

#[cfg(feature = "std")]
fn boxed_repeat_array<T: Clone, const N: usize>(value: T) -> std::boxed::Box<[T; N]> {
    let values: std::boxed::Box<[T]> = std::vec![value; N].into_boxed_slice();
    match values.try_into() {
        Ok(fixed) => fixed,
        Err(_) => panic!("fixed array length"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopDecision {
    Continue,
    Break,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RouteArmState {
    scope: ScopeId,
    arm: u8,
}

impl RouteArmState {
    const EMPTY: Self = Self {
        scope: ScopeId::none(),
        arm: 0,
    };
}

#[derive(Clone, Copy)]
struct LaneOfferState {
    scope: ScopeId,
    entry: StateIndex,
    parallel_root: ScopeId,
    frontier: FrontierKind,
    loop_meta: ScopeLoopMeta,
    label_meta: ScopeLabelMeta,
    static_ready: bool,
    flags: u8,
}

impl LaneOfferState {
    const FLAG_CONTROLLER: u8 = 1;
    const FLAG_DYNAMIC: u8 = 1 << 1;
    const EMPTY: Self = Self {
        scope: ScopeId::none(),
        entry: StateIndex::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        loop_meta: ScopeLoopMeta::EMPTY,
        label_meta: ScopeLabelMeta::EMPTY,
        static_ready: false,
        flags: 0,
    };

    #[inline]
    fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    fn static_ready(self) -> bool {
        self.static_ready
    }
}

#[derive(Clone, Copy)]
struct ScopeLoopMeta {
    flags: u8,
}

impl ScopeLoopMeta {
    const FLAG_SCOPE_ACTIVE: u8 = 1;
    const FLAG_SCOPE_LINGER: u8 = 1 << 1;
    const FLAG_CONTROL_SCOPE: u8 = 1 << 2;
    const FLAG_CONTINUE_HAS_RECV: u8 = 1 << 3;
    const FLAG_BREAK_HAS_RECV: u8 = 1 << 4;

    const EMPTY: Self = Self { flags: 0 };

    #[inline]
    fn scope_active(self) -> bool {
        (self.flags & Self::FLAG_SCOPE_ACTIVE) != 0
    }

    #[inline]
    fn scope_linger(self) -> bool {
        (self.flags & Self::FLAG_SCOPE_LINGER) != 0
    }

    #[inline]
    fn control_scope(self) -> bool {
        (self.flags & Self::FLAG_CONTROL_SCOPE) != 0
    }

    #[inline]
    fn loop_label_scope(self) -> bool {
        self.control_scope() || self.scope_linger()
    }

    #[inline]
    fn continue_has_recv(self) -> bool {
        (self.flags & Self::FLAG_CONTINUE_HAS_RECV) != 0
    }

    #[inline]
    fn break_has_recv(self) -> bool {
        (self.flags & Self::FLAG_BREAK_HAS_RECV) != 0
    }

    #[inline]
    fn arm_has_recv(self, arm: u8) -> bool {
        match arm {
            0 => self.continue_has_recv(),
            1 => self.break_has_recv(),
            _ => false,
        }
    }

    #[inline]
    fn recvless_ready(self) -> bool {
        (self.scope_active() || self.scope_linger())
            && (!self.continue_has_recv() || !self.break_has_recv())
    }
}

#[derive(Clone, Copy)]
struct ScopeLabelMeta {
    #[cfg(test)]
    scope_id: ScopeId,
    loop_meta: ScopeLoopMeta,
    recv_label: u8,
    recv_arm: u8,
    controller_labels: [u8; 2],
    hint_label_mask: u128,
    arm_label_masks: [u128; 2],
    evidence_arm_label_masks: [u128; 2],
    flags: u8,
}

impl ScopeLabelMeta {
    const FLAG_CURRENT_RECV_LABEL: u8 = 1;
    const FLAG_CURRENT_RECV_ARM: u8 = 1 << 1;
    const FLAG_CONTROLLER_ARM0: u8 = 1 << 2;
    const FLAG_CONTROLLER_ARM1: u8 = 1 << 3;
    const FLAG_CURRENT_RECV_BINDING_EXCLUDED: u8 = 1 << 4;

    const EMPTY: Self = Self {
        #[cfg(test)]
        scope_id: ScopeId::none(),
        loop_meta: ScopeLoopMeta::EMPTY,
        recv_label: 0,
        recv_arm: 0,
        controller_labels: [0; 2],
        hint_label_mask: 0,
        arm_label_masks: [0; 2],
        evidence_arm_label_masks: [0; 2],
        flags: 0,
    };

    #[inline]
    const fn label_bit(label: u8) -> u128 {
        if label < u128::BITS as u8 {
            1u128 << label
        } else {
            0
        }
    }

    #[inline]
    #[cfg(test)]
    fn scope_id(self) -> ScopeId {
        self.scope_id
    }

    #[inline]
    fn loop_meta(self) -> ScopeLoopMeta {
        self.loop_meta
    }

    #[inline]
    fn matches_current_recv_label(self, label: u8) -> bool {
        (self.flags & Self::FLAG_CURRENT_RECV_LABEL) != 0 && self.recv_label == label
    }

    #[inline]
    #[cfg(test)]
    fn current_recv_arm_for_label(self, label: u8) -> Option<u8> {
        if self.matches_current_recv_label(label) && (self.flags & Self::FLAG_CURRENT_RECV_ARM) != 0
        {
            Some(self.recv_arm)
        } else {
            None
        }
    }

    #[inline]
    fn matches_hint_label(self, label: u8) -> bool {
        (self.hint_label_mask & Self::label_bit(label)) != 0
    }

    #[inline]
    #[cfg(test)]
    fn controller_arm_for_label(self, label: u8) -> Option<u8> {
        if (self.flags & Self::FLAG_CONTROLLER_ARM0) != 0 && self.controller_labels[0] == label {
            return Some(0);
        }
        if (self.flags & Self::FLAG_CONTROLLER_ARM1) != 0 && self.controller_labels[1] == label {
            return Some(1);
        }
        None
    }

    #[inline]
    fn arm_for_label(self, label: u8) -> Option<u8> {
        let bit = Self::label_bit(label);
        if (self.arm_label_masks[0] & bit) != 0 {
            return Some(0);
        }
        if (self.arm_label_masks[1] & bit) != 0 {
            return Some(1);
        }
        None
    }

    #[inline]
    fn evidence_arm_for_label(self, label: u8) -> Option<u8> {
        let bit = Self::label_bit(label);
        if (self.evidence_arm_label_masks[0] & bit) != 0 {
            return Some(0);
        }
        if (self.evidence_arm_label_masks[1] & bit) != 0 {
            return Some(1);
        }
        None
    }

    #[inline]
    fn binding_evidence_arm_for_label(self, label: u8) -> Option<u8> {
        if self.matches_current_recv_label(label)
            && (self.flags & Self::FLAG_CURRENT_RECV_BINDING_EXCLUDED) != 0
        {
            return None;
        }
        self.evidence_arm_for_label(label)
    }

    #[inline]
    const fn singleton_label(mask: u128) -> Option<u8> {
        if mask == 0 || (mask & (mask - 1)) != 0 {
            return None;
        }
        Some(mask.trailing_zeros() as u8)
    }

    #[inline]
    fn binding_evidence_label_mask_for_arm(self, arm: u8) -> u128 {
        let arm_idx = arm as usize;
        if arm_idx >= self.evidence_arm_label_masks.len() {
            return 0;
        }
        let mut mask = self.evidence_arm_label_masks[arm_idx];
        if (self.flags & Self::FLAG_CURRENT_RECV_BINDING_EXCLUDED) != 0
            && (self.flags & Self::FLAG_CURRENT_RECV_ARM) != 0
            && self.recv_arm == arm
        {
            mask &= !Self::label_bit(self.recv_label);
        }
        mask
    }

    #[inline]
    fn binding_demux_label_mask_for_arm(self, arm: u8) -> u128 {
        let arm_idx = arm as usize;
        if arm_idx >= self.arm_label_masks.len() {
            return 0;
        }
        self.arm_label_masks[arm_idx]
    }

    #[inline]
    fn preferred_binding_label_mask(self, preferred_arm: Option<u8>) -> u128 {
        preferred_arm
            .map(|arm| self.binding_demux_label_mask_for_arm(arm))
            .unwrap_or(self.hint_label_mask)
    }

    #[inline]
    fn preferred_binding_label(self, preferred_arm: Option<u8>) -> Option<u8> {
        if let Some(arm) = preferred_arm {
            return Self::singleton_label(self.binding_evidence_label_mask_for_arm(arm));
        }
        let arm0 = Self::singleton_label(self.binding_evidence_label_mask_for_arm(0));
        let arm1 = Self::singleton_label(self.binding_evidence_label_mask_for_arm(1));
        match (arm0, arm1) {
            (Some(label), None) | (None, Some(label)) => Some(label),
            (Some(left), Some(right)) if left == right => Some(left),
            _ => None,
        }
    }

    #[inline]
    fn record_hint_label(&mut self, label: u8) {
        self.hint_label_mask |= Self::label_bit(label);
    }

    #[inline]
    fn record_arm_label(&mut self, arm: u8, label: u8) {
        self.record_hint_label(label);
        if (arm as usize) < self.arm_label_masks.len() {
            let bit = Self::label_bit(label);
            self.arm_label_masks[arm as usize] |= bit;
            self.evidence_arm_label_masks[arm as usize] |= bit;
        }
    }

    #[inline]
    fn record_dispatch_arm_label(&mut self, arm: u8, label: u8) {
        self.record_hint_label(label);
        if (arm as usize) < self.arm_label_masks.len() {
            self.arm_label_masks[arm as usize] |= Self::label_bit(label);
        }
    }

    #[inline]
    fn clear_evidence_arm_label(&mut self, arm: u8, label: u8) {
        if (arm as usize) < self.evidence_arm_label_masks.len() {
            self.evidence_arm_label_masks[arm as usize] &= !Self::label_bit(label);
        }
    }
}

#[derive(Clone, Copy)]
struct ActiveEntrySet {
    len: u8,
    entries: [StateIndex; MAX_LANES],
    lane_idx: [u8; MAX_LANES],
}

impl ActiveEntrySet {
    const EMPTY: Self = Self {
        len: 0,
        entries: [StateIndex::MAX; MAX_LANES],
        lane_idx: [u8::MAX; MAX_LANES],
    };

    #[inline]
    fn occupancy_mask(self) -> u8 {
        let len = self.len as usize;
        if len >= MAX_LANES {
            u8::MAX
        } else {
            (1u8 << len) - 1
        }
    }

    #[inline]
    fn entry_at(self, slot_idx: usize) -> Option<usize> {
        if slot_idx >= self.len as usize {
            return None;
        }
        Some(state_index_to_usize(self.entries[slot_idx]))
    }

    #[inline]
    fn contains_only(self, entry_idx: usize) -> bool {
        self.len == 1 && self.entry_at(0) == Some(entry_idx)
    }

    #[inline]
    fn slot_for_entry(self, entry_idx: usize) -> Option<usize> {
        let entry = checked_state_index(entry_idx)?;
        let len = self.len as usize;
        let mut slot_idx = 0usize;
        while slot_idx < len {
            if self.entries[slot_idx] == entry {
                return Some(slot_idx);
            }
            slot_idx += 1;
        }
        None
    }

    fn insert_entry(&mut self, entry_idx: usize, lane_idx: u8) -> bool {
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        let len = self.len as usize;
        let mut insert_idx = 0usize;
        while insert_idx < len {
            if self.entries[insert_idx] == entry {
                return false;
            }
            let existing_lane_idx = self.lane_idx[insert_idx];
            let existing_entry = self.entries[insert_idx];
            if existing_lane_idx > lane_idx
                || (existing_lane_idx == lane_idx && existing_entry.raw() > entry.raw())
            {
                break;
            }
            insert_idx += 1;
        }
        if len >= MAX_LANES {
            return false;
        }
        let mut shift_idx = len;
        while shift_idx > insert_idx {
            self.entries[shift_idx] = self.entries[shift_idx - 1];
            self.lane_idx[shift_idx] = self.lane_idx[shift_idx - 1];
            shift_idx -= 1;
        }
        self.entries[insert_idx] = entry;
        self.lane_idx[insert_idx] = lane_idx;
        self.len += 1;
        true
    }

    fn remove_entry(&mut self, entry_idx: usize) -> bool {
        let Ok(entry) = u16::try_from(entry_idx) else {
            return false;
        };
        let len = self.len as usize;
        let mut idx = 0usize;
        while idx < len {
            if self.entries[idx] == entry {
                break;
            }
            idx += 1;
        }
        if idx >= len {
            return false;
        }
        while idx + 1 < len {
            self.entries[idx] = self.entries[idx + 1];
            self.lane_idx[idx] = self.lane_idx[idx + 1];
            idx += 1;
        }
        self.entries[len - 1] = StateIndex::MAX;
        self.lane_idx[len - 1] = u8::MAX;
        self.len = self.len.saturating_sub(1);
        true
    }
}

#[derive(Clone, Copy)]
struct ObservedEntrySet {
    len: u8,
    entries: [StateIndex; MAX_LANES],
    slot_by_entry: [u8; crate::global::typestate::MAX_STATES],
    controller_mask: u8,
    dynamic_controller_mask: u8,
    progress_mask: u8,
    ready_arm_mask: u8,
    ready_mask: u8,
    route_mask: u8,
    parallel_mask: u8,
    loop_mask: u8,
    passive_observer_mask: u8,
}

impl ObservedEntrySet {
    const EMPTY: Self = Self {
        len: 0,
        entries: [StateIndex::MAX; MAX_LANES],
        slot_by_entry: [u8::MAX; crate::global::typestate::MAX_STATES],
        controller_mask: 0,
        dynamic_controller_mask: 0,
        progress_mask: 0,
        ready_arm_mask: 0,
        ready_mask: 0,
        route_mask: 0,
        parallel_mask: 0,
        loop_mask: 0,
        passive_observer_mask: 0,
    };

    #[inline]
    fn occupancy_mask(self) -> u8 {
        let len = self.len as usize;
        if len >= MAX_LANES {
            u8::MAX
        } else {
            (1u8 << len) - 1
        }
    }

    #[inline]
    fn frontier_mask(self, frontier: FrontierKind) -> u8 {
        match frontier {
            FrontierKind::Route => self.route_mask,
            FrontierKind::Parallel => self.parallel_mask,
            FrontierKind::Loop => self.loop_mask,
            FrontierKind::PassiveObserver => self.passive_observer_mask,
        }
    }

    fn insert_entry(&mut self, entry_idx: usize) -> Option<(u8, bool)> {
        if entry_idx >= crate::global::typestate::MAX_STATES {
            return None;
        }
        let entry = checked_state_index(entry_idx)?;
        let observed_idx = self.slot_by_entry[entry_idx] as usize;
        if observed_idx < self.len as usize && self.entries[observed_idx] == entry {
            return Some((1u8 << observed_idx, false));
        }
        let observed_idx = self.len as usize;
        if observed_idx >= MAX_LANES {
            return None;
        }
        self.entries[observed_idx] = entry;
        self.slot_by_entry[entry_idx] = observed_idx as u8;
        self.len += 1;
        Some((1u8 << observed_idx, true))
    }

    #[inline]
    fn entry_bit(self, entry_idx: usize) -> u8 {
        if entry_idx >= crate::global::typestate::MAX_STATES {
            return 0;
        }
        let observed_idx = self.slot_by_entry[entry_idx] as usize;
        if observed_idx >= self.len as usize {
            return 0;
        }
        1u8 << observed_idx
    }

    #[inline]
    fn first_entry_idx(self, mask: u8) -> Option<usize> {
        if mask == 0 {
            return None;
        }
        let observed_idx = mask.trailing_zeros() as usize;
        if observed_idx >= self.len as usize {
            return None;
        }
        Some(state_index_to_usize(self.entries[observed_idx]))
    }

    #[inline]
    fn observe(&mut self, observed_bit: u8, observed: OfferEntryObservedState) {
        if observed.is_controller() {
            self.controller_mask |= observed_bit;
        }
        if observed.is_dynamic() {
            self.dynamic_controller_mask |= observed_bit;
        }
        if observed.has_progress_evidence() {
            self.progress_mask |= observed_bit;
        }
        if observed.has_ready_arm_evidence() {
            self.ready_arm_mask |= observed_bit;
        }
        if (observed.flags & OfferEntryObservedState::FLAG_READY) != 0 {
            self.ready_mask |= observed_bit;
        }
        if observed.matches_frontier(FrontierKind::Route) {
            self.route_mask |= observed_bit;
        }
        if observed.matches_frontier(FrontierKind::Parallel) {
            self.parallel_mask |= observed_bit;
        }
        if observed.matches_frontier(FrontierKind::Loop) {
            self.loop_mask |= observed_bit;
        }
        if observed.matches_frontier(FrontierKind::PassiveObserver) {
            self.passive_observer_mask |= observed_bit;
        }
    }

    #[inline]
    fn replace_observation(&mut self, entry_idx: usize, observed: OfferEntryObservedState) -> bool {
        let observed_bit = self.entry_bit(entry_idx);
        if observed_bit == 0 {
            return false;
        }
        self.controller_mask &= !observed_bit;
        self.dynamic_controller_mask &= !observed_bit;
        self.progress_mask &= !observed_bit;
        self.ready_arm_mask &= !observed_bit;
        self.ready_mask &= !observed_bit;
        self.route_mask &= !observed_bit;
        self.parallel_mask &= !observed_bit;
        self.loop_mask &= !observed_bit;
        self.passive_observer_mask &= !observed_bit;
        self.observe(observed_bit, observed);
        true
    }

    fn move_entry_slot(&mut self, entry_idx: usize, new_slot_idx: usize) -> bool {
        if entry_idx >= crate::global::typestate::MAX_STATES {
            return false;
        }
        let old_slot_idx = self.slot_by_entry[entry_idx] as usize;
        let len = self.len as usize;
        if old_slot_idx >= len || new_slot_idx >= len {
            return false;
        }
        if old_slot_idx == new_slot_idx {
            return true;
        }
        let entry = self.entries[old_slot_idx];
        if old_slot_idx < new_slot_idx {
            let mut slot_idx = old_slot_idx;
            while slot_idx < new_slot_idx {
                self.entries[slot_idx] = self.entries[slot_idx + 1];
                self.slot_by_entry[state_index_to_usize(self.entries[slot_idx])] = slot_idx as u8;
                slot_idx += 1;
            }
        } else {
            let mut slot_idx = old_slot_idx;
            while slot_idx > new_slot_idx {
                self.entries[slot_idx] = self.entries[slot_idx - 1];
                self.slot_by_entry[state_index_to_usize(self.entries[slot_idx])] = slot_idx as u8;
                slot_idx -= 1;
            }
        }
        self.entries[new_slot_idx] = entry;
        self.slot_by_entry[entry_idx] = new_slot_idx as u8;
        self.controller_mask =
            Self::move_slot_mask(self.controller_mask, len, old_slot_idx, new_slot_idx);
        self.dynamic_controller_mask = Self::move_slot_mask(
            self.dynamic_controller_mask,
            len,
            old_slot_idx,
            new_slot_idx,
        );
        self.progress_mask =
            Self::move_slot_mask(self.progress_mask, len, old_slot_idx, new_slot_idx);
        self.ready_arm_mask =
            Self::move_slot_mask(self.ready_arm_mask, len, old_slot_idx, new_slot_idx);
        self.ready_mask = Self::move_slot_mask(self.ready_mask, len, old_slot_idx, new_slot_idx);
        self.route_mask = Self::move_slot_mask(self.route_mask, len, old_slot_idx, new_slot_idx);
        self.parallel_mask =
            Self::move_slot_mask(self.parallel_mask, len, old_slot_idx, new_slot_idx);
        self.loop_mask = Self::move_slot_mask(self.loop_mask, len, old_slot_idx, new_slot_idx);
        self.passive_observer_mask =
            Self::move_slot_mask(self.passive_observer_mask, len, old_slot_idx, new_slot_idx);
        true
    }

    fn insert_observation_at_slot(
        &mut self,
        entry_idx: usize,
        slot_idx: usize,
        observed: OfferEntryObservedState,
    ) -> bool {
        if entry_idx >= crate::global::typestate::MAX_STATES {
            return false;
        }
        let len = self.len as usize;
        if len >= MAX_LANES || slot_idx > len {
            return false;
        }
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        let existing_slot = self.slot_by_entry[entry_idx] as usize;
        if existing_slot < len && self.entries[existing_slot] == entry {
            return false;
        }
        let mut shift_idx = len;
        while shift_idx > slot_idx {
            self.entries[shift_idx] = self.entries[shift_idx - 1];
            self.slot_by_entry[state_index_to_usize(self.entries[shift_idx])] = shift_idx as u8;
            shift_idx -= 1;
        }
        self.entries[slot_idx] = entry;
        self.slot_by_entry[entry_idx] = slot_idx as u8;
        self.len += 1;
        self.controller_mask = Self::insert_slot_mask(self.controller_mask, len, slot_idx);
        self.dynamic_controller_mask =
            Self::insert_slot_mask(self.dynamic_controller_mask, len, slot_idx);
        self.progress_mask = Self::insert_slot_mask(self.progress_mask, len, slot_idx);
        self.ready_arm_mask = Self::insert_slot_mask(self.ready_arm_mask, len, slot_idx);
        self.ready_mask = Self::insert_slot_mask(self.ready_mask, len, slot_idx);
        self.route_mask = Self::insert_slot_mask(self.route_mask, len, slot_idx);
        self.parallel_mask = Self::insert_slot_mask(self.parallel_mask, len, slot_idx);
        self.loop_mask = Self::insert_slot_mask(self.loop_mask, len, slot_idx);
        self.passive_observer_mask =
            Self::insert_slot_mask(self.passive_observer_mask, len, slot_idx);
        self.observe(1u8 << slot_idx, observed);
        true
    }

    fn remove_observation(&mut self, entry_idx: usize) -> bool {
        if entry_idx >= crate::global::typestate::MAX_STATES {
            return false;
        }
        let slot_idx = self.slot_by_entry[entry_idx] as usize;
        let len = self.len as usize;
        if slot_idx >= len {
            return false;
        }
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        if self.entries[slot_idx] != entry {
            return false;
        }
        let mut shift_idx = slot_idx;
        while shift_idx + 1 < len {
            self.entries[shift_idx] = self.entries[shift_idx + 1];
            self.slot_by_entry[state_index_to_usize(self.entries[shift_idx])] = shift_idx as u8;
            shift_idx += 1;
        }
        self.entries[len - 1] = StateIndex::MAX;
        self.slot_by_entry[entry_idx] = u8::MAX;
        self.len = self.len.saturating_sub(1);
        self.controller_mask = Self::remove_slot_mask(self.controller_mask, len, slot_idx);
        self.dynamic_controller_mask =
            Self::remove_slot_mask(self.dynamic_controller_mask, len, slot_idx);
        self.progress_mask = Self::remove_slot_mask(self.progress_mask, len, slot_idx);
        self.ready_arm_mask = Self::remove_slot_mask(self.ready_arm_mask, len, slot_idx);
        self.ready_mask = Self::remove_slot_mask(self.ready_mask, len, slot_idx);
        self.route_mask = Self::remove_slot_mask(self.route_mask, len, slot_idx);
        self.parallel_mask = Self::remove_slot_mask(self.parallel_mask, len, slot_idx);
        self.loop_mask = Self::remove_slot_mask(self.loop_mask, len, slot_idx);
        self.passive_observer_mask =
            Self::remove_slot_mask(self.passive_observer_mask, len, slot_idx);
        true
    }

    fn replace_entry_at_slot(
        &mut self,
        old_entry_idx: usize,
        new_entry_idx: usize,
        observed: OfferEntryObservedState,
    ) -> bool {
        if old_entry_idx >= crate::global::typestate::MAX_STATES
            || new_entry_idx >= crate::global::typestate::MAX_STATES
        {
            return false;
        }
        let slot_idx = self.slot_by_entry[old_entry_idx] as usize;
        let len = self.len as usize;
        if slot_idx >= len {
            return false;
        }
        let Some(old_entry) = checked_state_index(old_entry_idx) else {
            return false;
        };
        let Some(new_entry) = checked_state_index(new_entry_idx) else {
            return false;
        };
        if self.entries[slot_idx] != old_entry {
            return false;
        }
        let existing_new_slot = self.slot_by_entry[new_entry_idx] as usize;
        if existing_new_slot < len {
            return false;
        }
        let observed_bit = 1u8 << slot_idx;
        self.entries[slot_idx] = new_entry;
        self.slot_by_entry[old_entry_idx] = u8::MAX;
        self.slot_by_entry[new_entry_idx] = slot_idx as u8;
        self.controller_mask &= !observed_bit;
        self.dynamic_controller_mask &= !observed_bit;
        self.progress_mask &= !observed_bit;
        self.ready_arm_mask &= !observed_bit;
        self.ready_mask &= !observed_bit;
        self.route_mask &= !observed_bit;
        self.parallel_mask &= !observed_bit;
        self.loop_mask &= !observed_bit;
        self.passive_observer_mask &= !observed_bit;
        self.observe(observed_bit, observed);
        true
    }

    fn move_slot_mask(mask: u8, len: usize, old_slot_idx: usize, new_slot_idx: usize) -> u8 {
        let mut remapped = 0u8;
        let mut slot_idx = 0usize;
        while slot_idx < len {
            let source_slot = if old_slot_idx < new_slot_idx {
                if slot_idx < old_slot_idx || slot_idx > new_slot_idx {
                    slot_idx
                } else if slot_idx == new_slot_idx {
                    old_slot_idx
                } else {
                    slot_idx + 1
                }
            } else if slot_idx < new_slot_idx || slot_idx > old_slot_idx {
                slot_idx
            } else if slot_idx == new_slot_idx {
                old_slot_idx
            } else {
                slot_idx - 1
            };
            if ((mask >> source_slot) & 1) != 0 {
                remapped |= 1u8 << slot_idx;
            }
            slot_idx += 1;
        }
        remapped
    }

    fn insert_slot_mask(mask: u8, len: usize, slot_idx: usize) -> u8 {
        let mut remapped = 0u8;
        let mut new_slot_idx = 0usize;
        while new_slot_idx <= len {
            if new_slot_idx == slot_idx {
                new_slot_idx += 1;
                continue;
            }
            let source_slot = if new_slot_idx < slot_idx {
                new_slot_idx
            } else {
                new_slot_idx - 1
            };
            if source_slot < len && ((mask >> source_slot) & 1) != 0 {
                remapped |= 1u8 << new_slot_idx;
            }
            new_slot_idx += 1;
        }
        remapped
    }

    fn remove_slot_mask(mask: u8, len: usize, slot_idx: usize) -> u8 {
        if len == 0 || slot_idx >= len {
            return 0;
        }
        let mut remapped = 0u8;
        let mut new_slot_idx = 0usize;
        while new_slot_idx + 1 < len {
            let source_slot = if new_slot_idx < slot_idx {
                new_slot_idx
            } else {
                new_slot_idx + 1
            };
            if ((mask >> source_slot) & 1) != 0 {
                remapped |= 1u8 << new_slot_idx;
            }
            new_slot_idx += 1;
        }
        remapped
    }
}

#[derive(Clone, Copy)]
struct RootFrontierState {
    root: ScopeId,
    active_mask: u8,
    controller_mask: u8,
    dynamic_controller_mask: u8,
    offer_lane_mask: u8,
    offer_lane_entry_slot_masks: [u8; MAX_LANES],
    observed_epoch: u32,
    observed_key: FrontierObservationKey,
    active_entries: ActiveEntrySet,
    observed_entries: ObservedEntrySet,
}

impl RootFrontierState {
    const EMPTY: Self = Self {
        root: ScopeId::none(),
        active_mask: 0,
        controller_mask: 0,
        dynamic_controller_mask: 0,
        offer_lane_mask: 0,
        offer_lane_entry_slot_masks: [0; MAX_LANES],
        observed_epoch: 0,
        observed_key: FrontierObservationKey::EMPTY,
        active_entries: ActiveEntrySet::EMPTY,
        observed_entries: ObservedEntrySet::EMPTY,
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FrontierObservationKey {
    active_entries: [StateIndex; MAX_LANES],
    entry_summary_fingerprints: [u8; MAX_LANES],
    scope_generations: [u32; MAX_LANES],
    offer_lane_mask: u8,
    binding_nonempty_mask: u8,
    route_change_epochs: [u32; MAX_LANES],
}

impl FrontierObservationKey {
    const EMPTY: Self = Self {
        active_entries: [StateIndex::MAX; MAX_LANES],
        entry_summary_fingerprints: [0; MAX_LANES],
        scope_generations: [0; MAX_LANES],
        offer_lane_mask: 0,
        binding_nonempty_mask: 0,
        route_change_epochs: [0; MAX_LANES],
    };
}

#[derive(Clone, Copy)]
struct OfferEntryStaticSummary {
    frontier_mask: u8,
    flags: u8,
}

impl OfferEntryStaticSummary {
    const FLAG_CONTROLLER: u8 = 1;
    const FLAG_DYNAMIC: u8 = 1 << 1;
    const FLAG_STATIC_READY: u8 = 1 << 2;

    const EMPTY: Self = Self {
        frontier_mask: 0,
        flags: 0,
    };

    #[inline]
    fn observe_lane(&mut self, info: LaneOfferState) {
        self.frontier_mask |= info.frontier.bit();
        if info.is_controller() {
            self.flags |= Self::FLAG_CONTROLLER;
        }
        if info.is_dynamic() {
            self.flags |= Self::FLAG_DYNAMIC;
        }
        if info.static_ready() {
            self.flags |= Self::FLAG_STATIC_READY;
        }
    }

    #[inline]
    fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    fn static_ready(self) -> bool {
        (self.flags & Self::FLAG_STATIC_READY) != 0
    }

    #[inline]
    fn observation_fingerprint(self) -> u8 {
        self.frontier_mask | (self.flags << 4)
    }
}

#[derive(Clone, Copy)]
struct OfferEntryState {
    active_mask: u8,
    lane_idx: u8,
    parallel_root: ScopeId,
    frontier: FrontierKind,
    scope_id: ScopeId,
    offer_lane_mask: u8,
    offer_lanes: [u8; MAX_LANES],
    offer_lanes_len: u8,
    selection_meta: CurrentScopeSelectionMeta,
    label_meta: ScopeLabelMeta,
    materialization_meta: ScopeArmMaterializationMeta,
    summary: OfferEntryStaticSummary,
    observed: OfferEntryObservedState,
}

impl OfferEntryState {
    const EMPTY: Self = Self {
        active_mask: 0,
        lane_idx: u8::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        scope_id: ScopeId::none(),
        offer_lane_mask: 0,
        offer_lanes: [0; MAX_LANES],
        offer_lanes_len: 0,
        selection_meta: CurrentScopeSelectionMeta::EMPTY,
        label_meta: ScopeLabelMeta::EMPTY,
        materialization_meta: ScopeArmMaterializationMeta::EMPTY,
        summary: OfferEntryStaticSummary::EMPTY,
        observed: OfferEntryObservedState::EMPTY,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OfferEntryObservedState {
    scope_id: ScopeId,
    frontier_mask: u8,
    flags: u8,
}

impl OfferEntryObservedState {
    const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        frontier_mask: 0,
        flags: 0,
    };
    const FLAG_CONTROLLER: u8 = 1;
    const FLAG_DYNAMIC: u8 = 1 << 1;
    const FLAG_PROGRESS: u8 = 1 << 2;
    const FLAG_READY_ARM: u8 = 1 << 3;
    const FLAG_BINDING_READY: u8 = 1 << 4;
    const FLAG_READY: u8 = 1 << 5;

    #[inline]
    fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    fn has_progress_evidence(self) -> bool {
        (self.flags & Self::FLAG_PROGRESS) != 0
    }

    #[inline]
    fn has_ready_arm_evidence(self) -> bool {
        (self.flags & Self::FLAG_READY_ARM) != 0
    }

    #[inline]
    fn ready(self) -> bool {
        (self.flags & Self::FLAG_READY) != 0
    }

    #[cfg(test)]
    #[inline]
    fn binding_ready(self) -> bool {
        (self.flags & Self::FLAG_BINDING_READY) != 0
    }

    #[inline]
    fn matches_frontier(self, frontier: FrontierKind) -> bool {
        (self.frontier_mask & frontier.bit()) != 0
    }
}

#[derive(Clone, Copy)]
struct BindingInbox {
    slots: [[Option<crate::binding::IncomingClassification>; Self::PER_LANE_CAPACITY]; MAX_LANES],
    len: [u8; MAX_LANES],
    nonempty_mask: u8,
    label_masks: [u128; MAX_LANES],
    buffered_label_lane_masks: [u8; 128],
}

impl BindingInbox {
    const PER_LANE_CAPACITY: usize = 8;
    const EMPTY: Self = Self {
        slots: [[None; Self::PER_LANE_CAPACITY]; MAX_LANES],
        len: [0; MAX_LANES],
        nonempty_mask: 0,
        label_masks: [0; MAX_LANES],
        buffered_label_lane_masks: [0; 128],
    };

    #[inline]
    fn update_nonempty_mask(&mut self, lane_idx: usize) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let bit = 1u8 << lane_idx;
        if self.len[lane_idx] == 0 {
            self.nonempty_mask &= !bit;
        } else {
            self.nonempty_mask |= bit;
        }
    }

    #[inline]
    fn has_buffered_for_lane_mask(&self, lane_mask: u8) -> bool {
        (self.nonempty_mask & lane_mask) != 0
    }

    #[inline]
    fn recompute_label_mask(&mut self, lane_idx: usize) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let buffered = self.len[lane_idx] as usize;
        let mut mask = 0u128;
        let mut idx = 0usize;
        while idx < buffered {
            if let Some(classification) = self.slots[lane_idx][idx] {
                mask |= ScopeLabelMeta::label_bit(classification.label);
            }
            idx += 1;
        }
        self.sync_label_mask(lane_idx, mask);
    }

    #[inline]
    fn sync_label_mask(&mut self, lane_idx: usize, new_mask: u128) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let old_mask = self.label_masks[lane_idx];
        if old_mask == new_mask {
            return;
        }
        let lane_bit = 1u8 << lane_idx;
        let mut removed = old_mask & !new_mask;
        while removed != 0 {
            let label = removed.trailing_zeros() as usize;
            self.buffered_label_lane_masks[label] &= !lane_bit;
            removed &= removed - 1;
        }
        let mut added = new_mask & !old_mask;
        while added != 0 {
            let label = added.trailing_zeros() as usize;
            self.buffered_label_lane_masks[label] |= lane_bit;
            added &= added - 1;
        }
        self.label_masks[lane_idx] = new_mask;
    }

    #[inline]
    fn buffered_lane_mask_for_labels(&self, label_mask: u128) -> u8 {
        let mut labels = label_mask;
        let mut lane_mask = 0u8;
        while labels != 0 {
            let label = labels.trailing_zeros() as usize;
            lane_mask |= self.buffered_label_lane_masks[label];
            labels &= labels - 1;
        }
        lane_mask
    }

    #[inline]
    fn remove_buffered_at(
        &mut self,
        lane_idx: usize,
        idx: usize,
    ) -> Option<crate::binding::IncomingClassification> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let buffered = self.len[lane_idx] as usize;
        if idx >= buffered {
            return None;
        }
        let classification = self.slots[lane_idx][idx]
            .take()
            .expect("binding inbox buffered slot must be populated");
        let mut shift = idx + 1;
        while shift < buffered {
            self.slots[lane_idx][shift - 1] = self.slots[lane_idx][shift];
            shift += 1;
        }
        self.slots[lane_idx][buffered - 1] = None;
        self.len[lane_idx] = (buffered - 1) as u8;
        self.recompute_label_mask(lane_idx);
        self.update_nonempty_mask(lane_idx);
        Some(classification)
    }

    #[inline]
    fn take_or_poll<B: BindingSlot>(
        &mut self,
        binding: &mut B,
        lane_idx: usize,
    ) -> Option<crate::binding::IncomingClassification> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let buffered = self.len[lane_idx] as usize;
        if buffered != 0 {
            return self.remove_buffered_at(lane_idx, 0);
        }
        let lane = lane_idx as u8;
        binding.poll_incoming_for_lane(lane)
    }

    #[inline]
    fn push_back(
        &mut self,
        lane_idx: usize,
        classification: crate::binding::IncomingClassification,
    ) -> bool {
        if lane_idx >= MAX_LANES {
            return false;
        }
        let buffered = self.len[lane_idx] as usize;
        if buffered >= Self::PER_LANE_CAPACITY {
            return false;
        }
        self.slots[lane_idx][buffered] = Some(classification);
        self.len[lane_idx] = (buffered + 1) as u8;
        self.nonempty_mask |= 1u8 << lane_idx;
        self.sync_label_mask(
            lane_idx,
            self.label_masks[lane_idx] | ScopeLabelMeta::label_bit(classification.label),
        );
        true
    }

    #[inline]
    fn take_matching_or_poll<B: BindingSlot>(
        &mut self,
        binding: &mut B,
        lane_idx: usize,
        expected_label: u8,
    ) -> Option<crate::binding::IncomingClassification> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let expected_bit = ScopeLabelMeta::label_bit(expected_label);
        if (self.label_masks[lane_idx] & expected_bit) != 0 {
            let buffered = self.len[lane_idx] as usize;
            let mut idx = 0usize;
            while idx < buffered {
                if let Some(classification) = self.slots[lane_idx][idx]
                    && classification.label == expected_label
                {
                    return self.remove_buffered_at(lane_idx, idx);
                }
                idx += 1;
            }
            self.recompute_label_mask(lane_idx);
        }

        let mut scans = 0usize;
        while scans < Self::PER_LANE_CAPACITY {
            scans += 1;
            if (self.len[lane_idx] as usize) >= Self::PER_LANE_CAPACITY {
                break;
            }
            let Some(classification) = binding.poll_incoming_for_lane(lane_idx as u8) else {
                break;
            };
            if classification.label == expected_label {
                return Some(classification);
            }
            if !self.push_back(lane_idx, classification) {
                break;
            }
        }
        None
    }

    #[inline]
    fn take_matching_mask_or_poll<B: BindingSlot, F: FnMut(u8) -> bool>(
        &mut self,
        binding: &mut B,
        lane_idx: usize,
        label_mask: u128,
        drop_label_mask: u128,
        mut drop_mismatch: F,
    ) -> Option<crate::binding::IncomingClassification> {
        if lane_idx >= MAX_LANES || label_mask == 0 {
            return None;
        }
        let buffered_scan_mask = label_mask | drop_label_mask;
        if (self.label_masks[lane_idx] & buffered_scan_mask) != 0 {
            let mut idx = 0usize;
            while idx < (self.len[lane_idx] as usize) {
                let Some(classification) = self.slots[lane_idx][idx] else {
                    idx += 1;
                    continue;
                };
                let label_bit = ScopeLabelMeta::label_bit(classification.label);
                if (label_mask & label_bit) != 0 {
                    return self.remove_buffered_at(lane_idx, idx);
                }
                if (drop_label_mask & label_bit) != 0 && drop_mismatch(classification.label) {
                    let _ = self.remove_buffered_at(lane_idx, idx);
                    continue;
                }
                idx += 1;
            }
        }

        let mut scans = 0usize;
        while scans < Self::PER_LANE_CAPACITY {
            scans += 1;
            if (self.len[lane_idx] as usize) >= Self::PER_LANE_CAPACITY {
                break;
            }
            let Some(classification) = binding.poll_incoming_for_lane(lane_idx as u8) else {
                break;
            };
            let label_bit = ScopeLabelMeta::label_bit(classification.label);
            if (label_mask & label_bit) != 0 {
                return Some(classification);
            }
            if (drop_label_mask & label_bit) != 0 && drop_mismatch(classification.label) {
                continue;
            }
            if !self.push_back(lane_idx, classification) {
                break;
            }
        }
        None
    }

    #[inline]
    fn put_back(
        &mut self,
        lane_idx: usize,
        classification: crate::binding::IncomingClassification,
    ) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let buffered = self.len[lane_idx] as usize;
        if buffered >= Self::PER_LANE_CAPACITY {
            return;
        }
        let mut idx = buffered;
        while idx > 0 {
            self.slots[lane_idx][idx] = self.slots[lane_idx][idx - 1];
            idx -= 1;
        }
        self.slots[lane_idx][0] = Some(classification);
        self.len[lane_idx] = (buffered + 1) as u8;
        self.nonempty_mask |= 1u8 << lane_idx;
        self.sync_label_mask(
            lane_idx,
            self.label_masks[lane_idx] | ScopeLabelMeta::label_bit(classification.label),
        );
    }
}

#[derive(Clone, Copy)]
struct ScopeEvidence {
    ack: Option<RouteDecisionToken>,
    hint_label: u8,
    ready_arm_mask: u8,
    poll_ready_arm_mask: u8,
    flags: u8,
}

impl ScopeEvidence {
    const NONE: u8 = u8::MAX;
    const ARM0_READY: u8 = 1 << 0;
    const ARM1_READY: u8 = 1 << 1;
    const FLAG_ACK_CONFLICT: u8 = 1;
    const FLAG_HINT_CONFLICT: u8 = 1 << 1;
    const EMPTY: Self = Self {
        ack: None,
        hint_label: Self::NONE,
        ready_arm_mask: 0,
        poll_ready_arm_mask: 0,
        flags: 0,
    };

    #[inline]
    const fn arm_bit(arm: u8) -> u8 {
        match arm {
            0 => Self::ARM0_READY,
            1 => Self::ARM1_READY,
            _ => 0,
        }
    }
}

const MAX_ROUTE_ARM_STACK: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Arm(u8);

impl Arm {
    #[inline]
    const fn new(value: u8) -> Option<Self> {
        if value <= 1 { Some(Self(value)) } else { None }
    }

    #[inline]
    const fn as_u8(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy)]
struct ScopeHint(u8);

impl ScopeHint {
    #[inline]
    const fn new(label: u8) -> Option<Self> {
        if label == 0 { None } else { Some(Self(label)) }
    }

    #[inline]
    const fn label(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RouteDecisionSource {
    Ack,
    Resolver,
    Poll,
}

impl RouteDecisionSource {
    #[inline]
    const fn as_tap_seq(self) -> u8 {
        match self {
            Self::Ack => 1,
            Self::Resolver => 2,
            Self::Poll => 3,
        }
    }

    #[cfg(test)]
    #[inline]
    const fn from_tap_seq(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Ack),
            2 => Some(Self::Resolver),
            3 => Some(Self::Poll),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RouteDecisionToken {
    arm: Arm,
    source: RouteDecisionSource,
}

impl RouteDecisionToken {
    #[inline]
    const fn from_ack(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteDecisionSource::Ack,
        }
    }

    #[inline]
    const fn from_resolver(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteDecisionSource::Resolver,
        }
    }

    #[inline]
    const fn from_poll(arm: Arm) -> Self {
        Self {
            arm,
            source: RouteDecisionSource::Poll,
        }
    }

    #[inline]
    const fn arm(self) -> Arm {
        self.arm
    }

    #[inline]
    const fn source(self) -> RouteDecisionSource {
        self.source
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RouteResolveStep {
    Resolved(Arm),
    Deferred { retry_hint: u8, source: DeferSource },
    Abort(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrontierCandidate {
    scope_id: ScopeId,
    entry_idx: usize,
    parallel_root: ScopeId,
    frontier: FrontierKind,
    is_controller: bool,
    is_dynamic: bool,
    has_evidence: bool,
    ready: bool,
}

impl FrontierCandidate {
    const EMPTY: Self = Self {
        scope_id: ScopeId::none(),
        entry_idx: usize::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        is_controller: false,
        is_dynamic: false,
        has_evidence: false,
        ready: false,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrontierSnapshot {
    current_scope: ScopeId,
    current_entry_idx: usize,
    current_parallel_root: ScopeId,
    current_frontier: FrontierKind,
    candidates: [FrontierCandidate; MAX_LANES],
    candidate_len: usize,
}

impl FrontierSnapshot {
    #[inline]
    fn matches_parallel_root(self, candidate: FrontierCandidate) -> bool {
        self.current_parallel_root.is_none()
            || candidate.parallel_root == self.current_parallel_root
    }

    fn select_yield_candidate(self, visited: FrontierVisitSet) -> Option<FrontierCandidate> {
        let mut idx = 0usize;
        while idx < self.candidate_len {
            let candidate = self.candidates[idx];
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.frontier == self.current_frontier
                && candidate.ready
                && candidate.has_evidence
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        idx = 0;
        while idx < self.candidate_len {
            let candidate = self.candidates[idx];
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.ready
                && candidate.has_evidence
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        None
    }

    fn select_exhausted_controller_candidate(
        self,
        visited: FrontierVisitSet,
    ) -> Option<FrontierCandidate> {
        let mut idx = 0usize;
        while idx < self.candidate_len {
            let candidate = self.candidates[idx];
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.frontier == self.current_frontier
                && candidate.is_controller
                && candidate.ready
                && candidate.has_evidence
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        idx = 0;
        while idx < self.candidate_len {
            let candidate = self.candidates[idx];
            if (candidate.scope_id != self.current_scope
                || candidate.entry_idx != self.current_entry_idx)
                && self.matches_parallel_root(candidate)
                && candidate.is_controller
                && candidate.ready
                && candidate.has_evidence
                && !visited.contains(candidate.scope_id)
            {
                return Some(candidate);
            }
            idx += 1;
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrontierVisitSet {
    slots: [ScopeId; MAX_LANES],
    len: usize,
}

impl FrontierVisitSet {
    const EMPTY: Self = Self {
        slots: [ScopeId::none(); MAX_LANES],
        len: 0,
    };

    #[inline]
    fn contains(self, scope: ScopeId) -> bool {
        let mut idx = 0usize;
        while idx < self.len {
            if self.slots[idx] == scope {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    fn record(&mut self, scope: ScopeId) {
        if scope.is_none() || self.contains(scope) || self.len >= MAX_LANES {
            return;
        }
        self.slots[self.len] = scope;
        self.len += 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrontierDeferOutcome {
    Continue,
    Yielded,
    Exhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EvidenceFingerprint(u8);

impl EvidenceFingerprint {
    #[inline]
    const fn new(has_ack: bool, has_ready_arm_evidence: bool, binding_ready: bool) -> Self {
        let mut bits = 0u8;
        if has_ack {
            bits |= 1 << 0;
        }
        if has_ready_arm_evidence {
            bits |= 1 << 1;
        }
        if binding_ready {
            bits |= 1 << 2;
        }
        Self(bits)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OfferLivenessState {
    policy: crate::runtime::config::LivenessPolicy,
    remaining_defer: u8,
    remaining_no_evidence_defer: u8,
    forced_poll_attempts: u8,
    last_fingerprint: Option<EvidenceFingerprint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeferBudgetOutcome {
    Continue,
    Exhausted,
}

impl OfferLivenessState {
    #[inline]
    fn new(policy: crate::runtime::config::LivenessPolicy) -> Self {
        Self {
            policy,
            remaining_defer: policy.max_defer_per_offer,
            remaining_no_evidence_defer: policy.max_no_evidence_defer,
            forced_poll_attempts: 0,
            last_fingerprint: None,
        }
    }

    #[inline]
    fn on_defer(&mut self, fingerprint: EvidenceFingerprint) -> DeferBudgetOutcome {
        if self.remaining_defer == 0 {
            return DeferBudgetOutcome::Exhausted;
        }
        self.remaining_defer = self.remaining_defer.saturating_sub(1);
        let has_new_evidence = self.last_fingerprint != Some(fingerprint);
        self.last_fingerprint = Some(fingerprint);
        if !has_new_evidence {
            if self.remaining_no_evidence_defer == 0 {
                return DeferBudgetOutcome::Exhausted;
            }
            self.remaining_no_evidence_defer = self.remaining_no_evidence_defer.saturating_sub(1);
        }
        DeferBudgetOutcome::Continue
    }

    #[inline]
    const fn can_force_poll(self) -> bool {
        self.policy.force_poll_on_exhaustion
            && self.forced_poll_attempts < self.policy.max_forced_poll_attempts
    }

    #[inline]
    fn mark_forced_poll(&mut self) {
        self.forced_poll_attempts = self.forced_poll_attempts.saturating_add(1);
    }

    #[inline]
    const fn exhaust_reason(self) -> u16 {
        self.policy.exhaust_reason
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OfferSelectPriority {
    CurrentOfferEntry,
    DynamicControllerUnique,
    ControllerUnique,
    CandidateUnique,
}

#[inline]
fn choose_offer_priority(
    current_is_candidate: bool,
    dynamic_controller_count: usize,
    controller_count: usize,
    candidate_count: usize,
) -> Option<OfferSelectPriority> {
    if current_is_candidate {
        Some(OfferSelectPriority::CurrentOfferEntry)
    } else if dynamic_controller_count == 1 {
        Some(OfferSelectPriority::DynamicControllerUnique)
    } else if controller_count == 1 {
        Some(OfferSelectPriority::ControllerUnique)
    } else if candidate_count == 1 {
        Some(OfferSelectPriority::CandidateUnique)
    } else {
        None
    }
}

#[inline]
    async fn yield_once() {
        let mut yielded = false;
        poll_fn(|cx| {
            if yielded {
                Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
        })
        .await
    }

#[inline]
fn current_entry_is_candidate(
    current_matches_candidate: bool,
    current_is_controller: bool,
    current_has_evidence: bool,
    candidate_count: usize,
    progress_sibling_exists: bool,
) -> bool {
    if !current_matches_candidate {
        return false;
    }
    if current_is_controller
        && !current_has_evidence
        && progress_sibling_exists
        && candidate_count > 0
    {
        return false;
    }
    true
}

#[inline]
fn current_entry_matches_after_filter(
    current_matches_candidate: bool,
    current_has_offer_lanes: bool,
    current_idx: usize,
    hint_filter: Option<usize>,
) -> bool {
    if !current_matches_candidate || !current_has_offer_lanes {
        return false;
    }
    if let Some(filtered_idx) = hint_filter {
        return current_idx == filtered_idx;
    }
    true
}

#[inline]
fn should_suppress_current_passive_without_evidence(
    current_frontier: FrontierKind,
    current_is_controller: bool,
    current_has_evidence: bool,
    controller_progress_sibling_exists: bool,
) -> bool {
    current_frontier == FrontierKind::PassiveObserver
        && !current_is_controller
        && !current_has_evidence
        && controller_progress_sibling_exists
}

#[cfg(test)]
#[inline]
fn candidate_participates_in_frontier_arbitration(
    entry_idx: usize,
    current_idx: usize,
    has_progress_evidence: bool,
    current_entry_unrunnable: bool,
) -> bool {
    entry_idx == current_idx
        || has_progress_evidence
        || (current_entry_unrunnable && entry_idx != current_idx)
}

#[cfg(test)]
#[inline]
fn controller_candidate_ready(
    is_controller: bool,
    entry_idx: usize,
    current_idx: usize,
    has_progress_evidence: bool,
) -> bool {
    !is_controller || entry_idx == current_idx || has_progress_evidence
}

#[inline]
fn candidate_has_progress_evidence(
    has_ready_arm_evidence: bool,
    ack_is_progress: bool,
    binding_ready: bool,
) -> bool {
    has_ready_arm_evidence || ack_is_progress || binding_ready
}

#[inline]
fn offer_entry_observed_state(
    scope_id: ScopeId,
    summary: OfferEntryStaticSummary,
    has_ready_arm_evidence: bool,
    ack_is_progress: bool,
    binding_ready: bool,
) -> OfferEntryObservedState {
    let has_progress_evidence =
        candidate_has_progress_evidence(has_ready_arm_evidence, ack_is_progress, binding_ready);
    let ready =
        has_ready_arm_evidence || ack_is_progress || binding_ready || summary.static_ready();
    let mut flags = 0u8;
    if summary.is_controller() {
        flags |= OfferEntryObservedState::FLAG_CONTROLLER;
    }
    if summary.is_dynamic() {
        flags |= OfferEntryObservedState::FLAG_DYNAMIC;
    }
    if has_progress_evidence {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if has_ready_arm_evidence {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }
    if binding_ready {
        flags |= OfferEntryObservedState::FLAG_BINDING_READY;
    }
    if ready {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    OfferEntryObservedState {
        scope_id,
        frontier_mask: summary.frontier_mask,
        flags,
    }
}

#[inline]
fn offer_entry_frontier_candidate(
    entry_idx: usize,
    parallel_root: ScopeId,
    frontier: FrontierKind,
    observed: OfferEntryObservedState,
) -> FrontierCandidate {
    FrontierCandidate {
        scope_id: observed.scope_id,
        entry_idx,
        parallel_root,
        frontier,
        is_controller: observed.is_controller(),
        is_dynamic: observed.is_dynamic(),
        has_evidence: observed.has_progress_evidence(),
        ready: observed.ready(),
    }
}

#[inline]
fn cached_offer_entry_observed_state(
    scope_id: ScopeId,
    summary: OfferEntryStaticSummary,
    observed_entries: ObservedEntrySet,
    observed_bit: u8,
) -> OfferEntryObservedState {
    let mut flags = 0u8;
    if summary.is_controller() {
        flags |= OfferEntryObservedState::FLAG_CONTROLLER;
    }
    if summary.is_dynamic() {
        flags |= OfferEntryObservedState::FLAG_DYNAMIC;
    }
    if (observed_entries.progress_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_PROGRESS;
    }
    if (observed_entries.ready_arm_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_READY_ARM;
    }
    if (observed_entries.ready_mask & observed_bit) != 0 {
        flags |= OfferEntryObservedState::FLAG_READY;
    }
    OfferEntryObservedState {
        scope_id,
        frontier_mask: summary.frontier_mask,
        flags,
    }
}

#[cfg(test)]
#[inline]
fn record_offer_entry_reentry_candidate(
    current_scope: ScopeId,
    current_entry_idx: usize,
    current_parallel_root: ScopeId,
    candidate: FrontierCandidate,
    ready_entry_idx: &mut Option<usize>,
    any_entry_idx: &mut Option<usize>,
) {
    if (candidate.scope_id == current_scope && candidate.entry_idx == current_entry_idx)
        || (!current_parallel_root.is_none() && candidate.parallel_root != current_parallel_root)
    {
        return;
    }
    if any_entry_idx.is_none() {
        *any_entry_idx = Some(candidate.entry_idx);
    }
    if candidate.ready && ready_entry_idx.is_none() {
        *ready_entry_idx = Some(candidate.entry_idx);
    }
}

#[derive(Clone, Copy)]
struct OfferScopeSelection {
    scope_id: ScopeId,
    frontier_parallel_root: Option<ScopeId>,
    offer_lanes: [u8; MAX_LANES],
    offer_lane_mask: u8,
    offer_lanes_len: usize,
    offer_lane: u8,
    offer_lane_idx: usize,
    label_meta: ScopeLabelMeta,
    materialization_meta: ScopeArmMaterializationMeta,
    passive_recv_meta: [CachedRecvMeta; 2],
    at_route_offer_entry: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CachedRecvMeta {
    cursor_index: StateIndex,
    eff_index: EffIndex,
    peer: u8,
    label: u8,
    resource: Option<u8>,
    is_control: bool,
    next: StateIndex,
    scope: ScopeId,
    route_arm: u8,
    is_choice_determinant: bool,
    shot: Option<CapShot>,
    policy: PolicyMode,
    lane: u8,
    flags: u8,
}

impl CachedRecvMeta {
    const FLAG_RECV_STEP: u8 = 1;

    const EMPTY: Self = Self {
        cursor_index: StateIndex::MAX,
        eff_index: EffIndex::ZERO,
        peer: 0,
        label: 0,
        resource: None,
        is_control: false,
        next: StateIndex::MAX,
        scope: ScopeId::none(),
        route_arm: u8::MAX,
        is_choice_determinant: false,
        shot: None,
        policy: PolicyMode::static_mode(),
        lane: 0,
        flags: 0,
    };

    #[inline]
    fn recv_meta(self) -> Option<(usize, RecvMeta)> {
        if self.cursor_index.is_max() || self.next.is_max() {
            return None;
        }
        Some((
            state_index_to_usize(self.cursor_index),
            RecvMeta {
                eff_index: self.eff_index,
                peer: self.peer,
                label: self.label,
                resource: self.resource,
                is_control: self.is_control,
                next: state_index_to_usize(self.next),
                scope: self.scope,
                route_arm: (self.route_arm != u8::MAX).then_some(self.route_arm),
                is_choice_determinant: self.is_choice_determinant,
                shot: self.shot,
                policy: self.policy,
                lane: self.lane,
            },
        ))
    }

    #[inline]
    fn is_recv_step(self) -> bool {
        (self.flags & Self::FLAG_RECV_STEP) != 0
    }
}

#[derive(Clone, Copy)]
struct ScopeArmMaterializationMeta {
    arm_count: u8,
    controller_arm_entry: [StateIndex; 2],
    controller_arm_label: [u8; 2],
    controller_recv_mask: u8,
    controller_cross_role_recv_mask: u8,
    recv_entry: [StateIndex; 2],
    passive_arm_entry: [StateIndex; 2],
    passive_arm_scope: [ScopeId; 2],
    binding_demux_lane_mask: [u8; 2],
    first_recv_dispatch: [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    first_recv_len: u8,
}

impl ScopeArmMaterializationMeta {
    const EMPTY: Self = Self {
        arm_count: 0,
        controller_arm_entry: [StateIndex::MAX; 2],
        controller_arm_label: [0; 2],
        controller_recv_mask: 0,
        controller_cross_role_recv_mask: 0,
        recv_entry: [StateIndex::MAX; 2],
        passive_arm_entry: [StateIndex::MAX; 2],
        passive_arm_scope: [ScopeId::none(); 2],
        binding_demux_lane_mask: [0; 2],
        first_recv_dispatch: [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH],
        first_recv_len: 0,
    };

    #[inline]
    fn controller_arm_entry(self, arm: u8) -> Option<(StateIndex, u8)> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.controller_arm_entry[arm];
        (!entry.is_max()).then_some((entry, self.controller_arm_label[arm]))
    }

    #[inline]
    fn recv_entry(self, arm: u8) -> Option<StateIndex> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.recv_entry[arm];
        (!entry.is_max()).then_some(entry)
    }

    #[inline]
    fn passive_arm_entry(self, arm: u8) -> Option<StateIndex> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let entry = self.passive_arm_entry[arm];
        (!entry.is_max()).then_some(entry)
    }

    #[inline]
    fn passive_arm_scope(self, arm: u8) -> Option<ScopeId> {
        let arm = arm as usize;
        if arm >= 2 {
            return None;
        }
        let scope = self.passive_arm_scope[arm];
        (!scope.is_none()).then_some(scope)
    }

    #[inline]
    fn record_binding_demux_lane(&mut self, arm: u8, lane: u8) {
        let bit = 1u8 << (lane as usize);
        if arm == ARM_SHARED {
            self.binding_demux_lane_mask[0] |= bit;
            self.binding_demux_lane_mask[1] |= bit;
            return;
        }
        let arm = arm as usize;
        if arm < self.binding_demux_lane_mask.len() {
            self.binding_demux_lane_mask[arm] |= bit;
        }
    }

    #[inline]
    fn binding_demux_lane_mask(self, preferred_arm: Option<u8>) -> u8 {
        preferred_arm
            .and_then(|arm| self.binding_demux_lane_mask.get(arm as usize).copied())
            .unwrap_or(self.binding_demux_lane_mask[0] | self.binding_demux_lane_mask[1])
    }

    #[inline]
    fn binding_demux_lane_mask_for_label_mask(
        self,
        label_meta: ScopeLabelMeta,
        label_mask: u128,
    ) -> u8 {
        if label_mask == 0 {
            return 0;
        }
        let mut lane_mask = 0u8;
        let mut arm = 0u8;
        while arm <= 1 {
            if (label_meta.binding_demux_label_mask_for_arm(arm) & label_mask) != 0 {
                lane_mask |= self.binding_demux_lane_mask(Some(arm));
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        if lane_mask != 0 {
            lane_mask
        } else {
            self.binding_demux_lane_mask(None)
        }
    }

    #[inline]
    fn first_recv_target(self, label: u8) -> Option<(u8, StateIndex)> {
        let mut idx = 0usize;
        while idx < self.first_recv_len as usize {
            let (entry_label, arm, target) = self.first_recv_dispatch[idx];
            if entry_label == label && !target.is_max() {
                return Some((arm, target));
            }
            idx += 1;
        }
        None
    }

    #[inline]
    fn arm_has_first_recv_dispatch(self, arm: u8) -> bool {
        let mut idx = 0usize;
        while idx < self.first_recv_len as usize {
            let (_label, dispatch_arm, target) = self.first_recv_dispatch[idx];
            if !target.is_max() && (dispatch_arm == arm || dispatch_arm == ARM_SHARED) {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    fn controller_arm_is_recv(self, arm: u8) -> bool {
        arm < 2 && (self.controller_recv_mask & (1u8 << arm)) != 0
    }

    #[inline]
    fn controller_arm_requires_ready_evidence(self, arm: u8) -> bool {
        arm < 2 && (self.controller_cross_role_recv_mask & (1u8 << arm)) != 0
    }
}

#[derive(Clone, Copy)]
struct ResolvedRouteDecision {
    route_token: RouteDecisionToken,
    selected_arm: u8,
    resolved_label_hint: Option<u8>,
}

enum ResolveTokenOutcome {
    RestartFrontier,
    Resolved(ResolvedRouteDecision),
}

#[derive(Clone, Copy)]
struct CurrentScopeSelectionMeta {
    flags: u8,
}

impl CurrentScopeSelectionMeta {
    const FLAG_ROUTE_ENTRY: u8 = 1;
    const FLAG_HAS_OFFER_LANES: u8 = 1 << 1;
    const FLAG_CONTROLLER: u8 = 1 << 2;

    const EMPTY: Self = Self { flags: 0 };

    #[inline]
    fn is_route_entry(self) -> bool {
        (self.flags & Self::FLAG_ROUTE_ENTRY) != 0
    }

    #[inline]
    fn has_offer_lanes(self) -> bool {
        !self.is_route_entry() || (self.flags & Self::FLAG_HAS_OFFER_LANES) != 0
    }

    #[inline]
    fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }
}

#[derive(Clone, Copy)]
struct CurrentFrontierSelectionState {
    frontier: FrontierKind,
    parallel_root: ScopeId,
    ready: bool,
    has_progress_evidence: bool,
    flags: u8,
}

impl CurrentFrontierSelectionState {
    const FLAG_CONTROLLER: u8 = 1;
    const FLAG_DYNAMIC: u8 = 1 << 1;

    #[inline]
    fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    fn parallel(self) -> Option<ScopeId> {
        if self.parallel_root.is_none() {
            None
        } else {
            Some(self.parallel_root)
        }
    }

    #[cfg(test)]
    #[inline]
    fn observe_candidate(
        &mut self,
        current_scope: ScopeId,
        current_idx: usize,
        candidate: FrontierCandidate,
    ) {
        if candidate.scope_id == current_scope && candidate.entry_idx == current_idx {
            self.ready = candidate.ready;
            self.has_progress_evidence = candidate.has_evidence;
        }
    }

    #[inline]
    fn loop_controller_without_evidence(self) -> bool {
        self.frontier == FrontierKind::Loop
            && self.is_controller()
            && self.ready
            && !self.has_progress_evidence
    }
}

#[derive(Clone, Copy)]
struct FrontierStaticFacts {
    frontier: FrontierKind,
    loop_meta: ScopeLoopMeta,
    ready: bool,
}

/// Branch metadata carried from `offer()` to `decode()`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BranchMeta {
    /// The scope this branch belongs to.
    pub(crate) scope_id: ScopeId,
    /// The selected arm (0, 1, ...).
    pub(crate) selected_arm: u8,
    /// Wire lane for this branch.
    pub(crate) lane_wire: u8,
    /// EffIndex for lane cursor advancement.
    pub(crate) eff_index: EffIndex,
    /// Classification of the branch for decode() dispatch.
    pub(crate) kind: BranchKind,
}

/// Classification of branch types for `decode()` dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BranchKind {
    /// Normal wire recv: payload comes from transport/binding.
    WireRecv,
    /// Synthetic local control: CanonicalControl self-send that doesn't go on wire.
    /// Decode from zero buffer; scope settlement uses meta fields directly.
    LocalControl,
    /// Arm starts with Send operation (passive observer scenario).
    /// The driver should use `into_endpoint()` and `flow().send()` instead of `decode()`.
    ArmSendHint,
    /// Empty arm leading to terminal (e.g., empty break arm).
    /// Decode succeeds with zero buffer; cursor advances to scope end.
    EmptyArmTerminal,
}

pub struct RouteBranch<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    U,
    C,
    E: EpochTable,
    const MAX_RV: usize,
    Mint,
    B: BindingSlot,
> where
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
{
    label: u8,
    transport_payload_len: usize,
    transport_payload_lane: u8,
    endpoint: CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    /// Channel selected by binding classification for the binding-backed recv path.
    /// `None` means the payload is read from the transport directly.
    binding_channel: Option<crate::binding::Channel>,
    /// Branch metadata from offer() for decode() dispatch.
    /// Eliminates label→arm inference in decode().
    branch_meta: BranchMeta,
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    pub async fn decode<M>(
        self,
    ) -> RecvResult<(
        CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        M::Payload,
    )>
    where
        M: MessageSpec,
        M::Payload: WireDecodeOwned,
    {
        let RouteBranch {
            label,
            transport_payload_len,
            transport_payload_lane,
            mut endpoint,
            binding_channel,
            branch_meta,
        } = self;

        let expected = <M as MessageSpec>::LABEL;
        if label != expected {
            return Err(RecvError::LabelMismatch {
                expected,
                actual: label,
            });
        }

        match branch_meta.kind {
            BranchKind::LocalControl => {
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;

                let route_arm = Some(branch_meta.selected_arm);
                let lane_idx = branch_meta.lane_wire as usize;
                let progress_eff = if let Some(eff) = endpoint.cursor.scope_lane_last_eff_for_arm(
                    branch_meta.scope_id,
                    branch_meta.selected_arm,
                    branch_meta.lane_wire,
                ) {
                    eff
                } else if let Some(eff) = endpoint
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    eff
                } else {
                    branch_meta.eff_index
                };
                endpoint.advance_lane_cursor(lane_idx, progress_eff);
                if branch_meta.selected_arm > 0
                    && endpoint
                        .cursor
                        .scope_region_by_id(branch_meta.scope_id)
                        .map(|region| region.linger)
                        .unwrap_or(false)
                    && let Some(scope_last_eff) = endpoint
                        .cursor
                        .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    endpoint.advance_lane_cursor(lane_idx, scope_last_eff);
                }
                if !endpoint.align_cursor_to_lane_progress(lane_idx) {
                    endpoint.set_cursor(
                        endpoint
                            .cursor
                            .try_advance_past_jumps()
                            .map_err(|_| RecvError::PhaseInvariant)?,
                    );
                }
                endpoint.maybe_skip_remaining_route_arm(
                    branch_meta.scope_id,
                    branch_meta.lane_wire,
                    Some(branch_meta.selected_arm),
                    progress_eff,
                );
                endpoint.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                endpoint.maybe_advance_phase();

                return Ok((endpoint, payload));
            }

            BranchKind::EmptyArmTerminal => {
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;

                let route_arm = Some(branch_meta.selected_arm);

                endpoint.set_cursor(
                    endpoint
                        .cursor
                        .try_follow_jumps()
                        .map_err(|_| RecvError::PhaseInvariant)?,
                );

                let lane_idx = branch_meta.lane_wire as usize;
                if let Some(eff_index) = endpoint
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    endpoint.advance_lane_cursor(lane_idx, eff_index);
                } else {
                    endpoint.advance_lane_cursor(lane_idx, branch_meta.eff_index);
                }
                endpoint.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                endpoint.maybe_advance_phase();

                return Ok((endpoint, payload));
            }

            BranchKind::ArmSendHint => {
                static ZERO_BUF: [u8; 64] = [0u8; 64];
                let payload = M::Payload::decode_owned(&ZERO_BUF).map_err(RecvError::Codec)?;

                let route_arm = Some(branch_meta.selected_arm);
                let lane_idx = branch_meta.lane_wire as usize;
                let progress_eff = if let Some(eff) = endpoint.cursor.scope_lane_last_eff_for_arm(
                    branch_meta.scope_id,
                    branch_meta.selected_arm,
                    branch_meta.lane_wire,
                ) {
                    eff
                } else if let Some(eff) = endpoint
                    .cursor
                    .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    eff
                } else {
                    branch_meta.eff_index
                };
                endpoint.advance_lane_cursor(lane_idx, progress_eff);
                if branch_meta.selected_arm > 0
                    && endpoint
                        .cursor
                        .scope_region_by_id(branch_meta.scope_id)
                        .map(|region| region.linger)
                        .unwrap_or(false)
                    && let Some(scope_last_eff) = endpoint
                        .cursor
                        .scope_lane_last_eff(branch_meta.scope_id, branch_meta.lane_wire)
                {
                    endpoint.advance_lane_cursor(lane_idx, scope_last_eff);
                }
                if !endpoint.align_cursor_to_lane_progress(lane_idx) {
                    endpoint.set_cursor(
                        endpoint
                            .cursor
                            .try_advance_past_jumps()
                            .map_err(|_| RecvError::PhaseInvariant)?,
                    );
                }
                endpoint.maybe_skip_remaining_route_arm(
                    branch_meta.scope_id,
                    branch_meta.lane_wire,
                    Some(branch_meta.selected_arm),
                    progress_eff,
                );
                endpoint.settle_scope_after_action(
                    branch_meta.scope_id,
                    route_arm,
                    None,
                    branch_meta.lane_wire,
                );
                endpoint.maybe_advance_phase();

                return Ok((endpoint, payload));
            }

            BranchKind::WireRecv => {}
        }

        let meta = endpoint
            .cursor
            .try_recv_meta()
            .ok_or_else(decode_phase_invariant)?;
        let control_handling = <M::ControlKind as ControlPayloadKind>::HANDLING;
        let expects_control = !matches!(control_handling, ControlHandling::None);
        if meta.is_control != expects_control {
            return Err(decode_phase_invariant());
        }

        if matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK) {
            if let Some(LoopMetadata {
                scope: scope_id,
                controller,
                target,
                role,
                ..
            }) = endpoint.cursor.loop_metadata_inner()
            {
                if role != LoopRole::Target || target != ROLE {
                    return Err(decode_phase_invariant());
                }

                if meta.peer != controller {
                    return Err(RecvError::PeerMismatch {
                        expected: controller,
                        actual: meta.peer,
                    });
                }

                let idx = CursorEndpoint::<ROLE, T, U, C, E, MAX_RV, Mint>::loop_index(scope_id)
                    .ok_or_else(decode_phase_invariant)?;
                let port = endpoint.port_for_lane(meta.lane as usize);
                let lane = port.lane();
                port.loop_table().acknowledge(lane, ROLE, idx);
                let has_local_decision = port.loop_table().has_decision(lane, idx);
                if has_local_decision {
                    port.ack_loop_decision(idx, ROLE);
                }
            }
        }

        let payload = if let Some(channel) = binding_channel {
            let port = endpoint.port_mut();
            let scratch_ptr = port.scratch_ptr();
            let scratch = unsafe { &mut *scratch_ptr };
            let n = endpoint
                .binding
                .on_recv(channel, scratch)
                .map_err(|_| decode_phase_invariant())?;

            M::Payload::decode_owned(&scratch[..n]).map_err(RecvError::Codec)?
        } else if transport_payload_len != 0 {
            let port = endpoint.port_for_lane(transport_payload_lane as usize);
            let scratch_ptr = port.scratch_ptr();
            let scratch = unsafe { &*scratch_ptr };
            M::Payload::decode_owned(&scratch[..transport_payload_len]).map_err(RecvError::Codec)?
        } else {
            // Empty payload (e.g., for marker types like HqResponseFin with no data)
            M::Payload::decode_owned(&[]).map_err(RecvError::Codec)?
        };

        endpoint.set_cursor(
            endpoint
                .cursor
                .try_advance_past_jumps()
                .map_err(|_| decode_phase_invariant())?,
        );

        let decode_lane_idx = meta.lane as usize;
        endpoint.advance_lane_cursor(decode_lane_idx, meta.eff_index);
        endpoint.maybe_skip_remaining_route_arm(
            meta.scope,
            meta.lane,
            meta.route_arm,
            meta.eff_index,
        );
        endpoint.settle_scope_after_action(
            meta.scope,
            meta.route_arm,
            Some(meta.eff_index),
            meta.lane,
        );
        if branch_meta.scope_id != meta.scope {
            endpoint.settle_scope_after_action(
                branch_meta.scope_id,
                Some(branch_meta.selected_arm),
                Some(meta.eff_index),
                branch_meta.lane_wire,
            );
        }
        let mut linger_scope = meta.scope;
        loop {
            if endpoint.is_linger_route(linger_scope) {
                let mut arm = endpoint.route_arm_for(meta.lane, linger_scope);
                if arm.is_none() {
                    arm = endpoint
                        .cursor
                        .first_recv_target_evidence(linger_scope, label)
                        .map(|(arm, _)| if arm == ARM_SHARED { 0 } else { arm });
                    if let Some(selected) = arm {
                        endpoint.set_route_arm(meta.lane, linger_scope, selected)?;
                    }
                }
                if let Some(arm) = arm {
                    if arm == 0 {
                        if let Some(last_eff) = endpoint.cursor.scope_lane_last_eff_for_arm(
                            linger_scope,
                            arm,
                            meta.lane,
                        ) {
                            if last_eff == meta.eff_index {
                                if let Some(first_eff) = endpoint
                                    .cursor
                                    .scope_lane_first_eff(linger_scope, meta.lane)
                                {
                                    endpoint.set_lane_cursor_to_eff_index(
                                        meta.lane as usize,
                                        first_eff,
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            let Some(parent) = endpoint.cursor.scope_parent(linger_scope) else {
                break;
            };
            linger_scope = parent;
        }
        if let Some(region) = endpoint.cursor.scope_region() {
            if region.kind == ScopeKind::Route && region.linger {
                let at_scope_start = endpoint.cursor.index() == region.start;
                let at_passive_branch = endpoint.cursor.jump_reason()
                    == Some(JumpReason::PassiveObserverBranch)
                    && endpoint
                        .cursor
                        .scope_region()
                        .map(|scope_region| scope_region.scope_id == region.scope_id)
                        .unwrap_or(false);
                if at_scope_start || at_passive_branch {
                    if let Some(arm) = endpoint.route_arm_for(meta.lane, region.scope_id) {
                        if arm == 0 {
                            if let Some(first_eff) = endpoint
                                .cursor
                                .scope_lane_first_eff(region.scope_id, meta.lane)
                            {
                                endpoint
                                    .set_lane_cursor_to_eff_index(meta.lane as usize, first_eff);
                            }
                        }
                    }
                }
            }
        }
        endpoint.maybe_advance_phase();
        Ok((endpoint, payload))
    }

    /// Branch label.
    #[inline]
    pub fn label(&self) -> u8 {
        self.label
    }

    /// Consume the branch and recover the underlying endpoint.
    ///
    /// Note: This does NOT advance the typestate. Use `decode()` to properly
    /// advance the cursor after receiving a route arm message.
    #[inline]
    pub fn into_endpoint(self) -> CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B> {
        self.endpoint
    }
}

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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        ports: [Option<Port<'r, T, E>>; MAX_LANES],
        guards: [Option<LaneGuard<'r, T, U, C>>; MAX_LANES],
        primary_lane: usize,
        sid: SessionId,
        owner: Owner<'r, E0>,
        epoch: EndpointEpoch<'r, E>,
        cursor: PhaseCursor<ROLE>,
        control: SessionControlCtx<'r, T, U, C, E, MAX_RV>,
        mint: Mint,
        binding: B,
    ) -> Self {
        let liveness_policy = control.liveness_policy();

        #[cfg(feature = "std")]
        let mut endpoint = Self {
            ports,
            guards,
            primary_lane,
            sid,
            _owner: owner,
            _epoch: epoch,
            cursor: std::boxed::Box::new(cursor),
            control,
            lane_route_arms: boxed_repeat_array([RouteArmState::EMPTY; MAX_ROUTE_ARM_STACK]),
            lane_route_arm_lens: boxed_repeat_array(0u8),
            lane_linger_counts: boxed_repeat_array(0u8),
            lane_linger_mask: 0,
            lane_offer_linger_mask: 0,
            active_offer_mask: 0,
            lane_offer_state: boxed_repeat_array(LaneOfferState::EMPTY),
            root_frontier_len: 0,
            root_frontier_state: boxed_repeat_array(RootFrontierState::EMPTY),
            root_frontier_slot_by_ordinal: boxed_repeat_array(u8::MAX),
            offer_entry_state: boxed_repeat_array(OfferEntryState::EMPTY),
            global_active_entries: ActiveEntrySet::EMPTY,
            global_offer_lane_mask: 0,
            global_offer_lane_entry_slot_masks: boxed_repeat_array(0u8),
            frontier_observation_epoch: 0,
            global_frontier_observed_epoch: 0,
            global_frontier_observed_key: FrontierObservationKey::EMPTY,
            global_frontier_observed: ObservedEntrySet::EMPTY,
            binding_inbox: BindingInbox::EMPTY,
            scope_evidence: boxed_repeat_array(ScopeEvidence::EMPTY),
            scope_evidence_generations: boxed_repeat_array(0u32),
            liveness_policy,
            mint,
            binding,
        };

        #[cfg(not(feature = "std"))]
        let mut endpoint = Self {
            ports,
            guards,
            primary_lane,
            sid,
            _owner: owner,
            _epoch: epoch,
            cursor,
            control,
            lane_route_arms: [[RouteArmState::EMPTY; MAX_ROUTE_ARM_STACK]; MAX_LANES],
            lane_route_arm_lens: [0; MAX_LANES],
            lane_linger_counts: [0; MAX_LANES],
            lane_linger_mask: 0,
            lane_offer_linger_mask: 0,
            active_offer_mask: 0,
            lane_offer_state: [LaneOfferState::EMPTY; MAX_LANES],
            root_frontier_len: 0,
            root_frontier_state: [RootFrontierState::EMPTY; MAX_LANES],
            root_frontier_slot_by_ordinal: [u8::MAX; ScopeId::ORDINAL_CAPACITY as usize],
            offer_entry_state: [OfferEntryState::EMPTY; crate::global::typestate::MAX_STATES],
            global_active_entries: ActiveEntrySet::EMPTY,
            global_offer_lane_mask: 0,
            global_offer_lane_entry_slot_masks: [0; MAX_LANES],
            frontier_observation_epoch: 0,
            global_frontier_observed_epoch: 0,
            global_frontier_observed_key: FrontierObservationKey::EMPTY,
            global_frontier_observed: ObservedEntrySet::EMPTY,
            binding_inbox: BindingInbox::EMPTY,
            scope_evidence: [ScopeEvidence::EMPTY; crate::eff::meta::MAX_EFF_NODES],
            scope_evidence_generations: [0; crate::eff::meta::MAX_EFF_NODES],
            liveness_policy,
            mint,
            binding,
        };
        endpoint.sync_lane_offer_state();
        endpoint
    }

    #[inline(always)]
    fn set_cursor(&mut self, cursor: PhaseCursor<ROLE>) {
        #[cfg(feature = "std")]
        {
            *self.cursor = cursor;
        }
        #[cfg(not(feature = "std"))]
        {
            self.cursor = cursor;
        }
    }

    #[inline]
    fn scope_trace(&self, scope: ScopeId) -> Option<ScopeTrace> {
        if scope.is_none() {
            return None;
        }
        self.cursor
            .scope_region_by_id(scope)
            .map(|region| ScopeTrace::new(region.range, region.nest))
    }

    /// Set route arm for (lane, scope) — update-in-place if exists, insert if not.
    ///
    /// Returns `Err(PhaseInvariant)` on capacity overflow or invalid lane.
    /// This prevents silent drops that could hide correctness bugs.
    fn set_route_arm(&mut self, lane: u8, scope: ScopeId, arm: u8) -> Result<(), RecvError> {
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return Ok(());
        }
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return Err(RecvError::PhaseInvariant);
        }
        let len = self.lane_route_arm_lens[lane_idx] as usize;
        let is_linger = self.is_linger_route(scope);

        // Check if (lane, scope) already exists — update in place
        for idx in 0..len {
            if self.lane_route_arms[lane_idx][idx].scope == scope {
                self.lane_route_arms[lane_idx][idx].arm = arm;
                self.refresh_lane_offer_state(lane_idx);
                return Ok(());
            }
        }

        // Not found — insert new entry
        if len >= MAX_ROUTE_ARM_STACK {
            return Err(RecvError::PhaseInvariant);
        }
        self.lane_route_arms[lane_idx][len] = RouteArmState { scope, arm };
        self.lane_route_arm_lens[lane_idx] = self.lane_route_arm_lens[lane_idx].saturating_add(1);
        if is_linger {
            self.increment_linger_count(lane_idx);
        }
        self.refresh_lane_offer_state(lane_idx);
        Ok(())
    }

    fn pop_route_arm(&mut self, lane: u8, scope: ScopeId) {
        if scope.is_none() {
            return;
        }
        let lane_idx = lane as usize;
        debug_assert!(
            lane_idx < MAX_LANES,
            "pop_route_arm: lane {} exceeds MAX_LANES {}",
            lane_idx,
            MAX_LANES
        );
        if lane_idx >= MAX_LANES {
            return;
        }
        let len = self.lane_route_arm_lens[lane_idx] as usize;
        if len == 0 {
            return;
        }
        let is_linger = self.is_linger_route(scope);
        if let Some(pos) = (0..len)
            .rev()
            .find(|&idx| self.lane_route_arms[lane_idx][idx].scope == scope)
        {
            let _removed = self.lane_route_arms[lane_idx][pos];
            let last = len - 1;
            for idx in pos..last {
                self.lane_route_arms[lane_idx][idx] = self.lane_route_arms[lane_idx][idx + 1];
            }
            self.lane_route_arms[lane_idx][last] = RouteArmState::EMPTY;
            self.lane_route_arm_lens[lane_idx] =
                self.lane_route_arm_lens[lane_idx].saturating_sub(1);
            if is_linger {
                self.decrement_linger_count(lane_idx);
            }
            self.refresh_lane_offer_state(lane_idx);
        }
    }

    fn scope_is_descendant_of(&self, scope: ScopeId, ancestor: ScopeId) -> bool {
        if scope.is_none() || ancestor.is_none() || scope == ancestor {
            return false;
        }
        let mut current = scope;
        while let Some(parent) = self.cursor.scope_parent(current) {
            if parent == ancestor {
                return true;
            }
            current = parent;
        }
        false
    }

    fn clear_descendant_route_state_for_lane(&mut self, lane: u8, ancestor_scope: ScopeId) {
        if ancestor_scope.is_none() {
            return;
        }
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return;
        }
        let len = self.lane_route_arm_lens[lane_idx] as usize;
        if len == 0 {
            return;
        }
        let mut stale_scopes = [ScopeId::none(); MAX_ROUTE_ARM_STACK];
        let mut stale_len = 0usize;
        let mut idx = 0usize;
        while idx < len {
            let scope = self.lane_route_arms[lane_idx][idx].scope;
            if !scope.is_none()
                && scope.kind() == ScopeKind::Route
                && self.scope_is_descendant_of(scope, ancestor_scope)
            {
                stale_scopes[stale_len] = scope;
                stale_len += 1;
            }
            idx += 1;
        }
        while stale_len > 0 {
            stale_len -= 1;
            let scope = stale_scopes[stale_len];
            self.pop_route_arm(lane, scope);
            self.clear_scope_evidence(scope);
        }
    }

    fn prune_route_state_to_cursor_path_for_lane(&mut self, lane: u8) {
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return;
        }
        let len = self.lane_route_arm_lens[lane_idx] as usize;
        if len == 0 {
            return;
        }
        let cursor_scope = self.cursor.node_scope_id();
        let mut stale_scopes = [ScopeId::none(); MAX_ROUTE_ARM_STACK];
        let mut stale_len = 0usize;
        let mut idx = 0usize;
        while idx < len {
            let scope = self.lane_route_arms[lane_idx][idx].scope;
            let keep = !scope.is_none()
                && (scope == cursor_scope || self.scope_is_descendant_of(cursor_scope, scope));
            if !keep && !scope.is_none() {
                stale_scopes[stale_len] = scope;
                stale_len += 1;
            }
            idx += 1;
        }
        while stale_len > 0 {
            stale_len -= 1;
            let scope = stale_scopes[stale_len];
            self.pop_route_arm(lane, scope);
            self.clear_scope_evidence(scope);
        }
    }

    fn is_linger_route(&self, scope: ScopeId) -> bool {
        self.cursor
            .scope_region_by_id(scope)
            .map(|region| {
                if region.kind == ScopeKind::Loop {
                    return true;
                }
                region.kind == ScopeKind::Route && region.linger
            })
            .unwrap_or(false)
    }

    fn increment_linger_count(&mut self, lane_idx: usize) {
        let count = &mut self.lane_linger_counts[lane_idx];
        debug_assert!(*count < u8::MAX);
        *count = count.saturating_add(1);
        if *count == 1 {
            self.lane_linger_mask |= 1u8 << lane_idx;
        }
    }

    fn decrement_linger_count(&mut self, lane_idx: usize) {
        let count = &mut self.lane_linger_counts[lane_idx];
        debug_assert!(*count > 0);
        if *count == 0 {
            return;
        }
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.lane_linger_mask &= !(1u8 << lane_idx);
        }
    }

    fn route_arm_for(&self, lane: u8, scope: ScopeId) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        let lane_idx = lane as usize;
        if lane_idx >= MAX_LANES {
            return None;
        }
        (0..self.lane_route_arm_lens[lane_idx] as usize)
            .rev()
            .find_map(|idx| {
                let slot = self.lane_route_arms[lane_idx][idx];
                (slot.scope == scope).then_some(slot.arm)
            })
    }

    fn selected_arm_for_scope(&self, scope: ScopeId) -> Option<u8> {
        if scope.is_none() {
            return None;
        }
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if let Some(arm) = self.route_arm_for(lane_idx as u8, scope) {
                return Some(arm);
            }
            lane_idx += 1;
        }
        None
    }

    fn route_scope_offer_entry_index(&self, scope_id: ScopeId) -> Option<usize> {
        let offer_entry = self.cursor.route_scope_offer_entry(scope_id)?;
        Some(if offer_entry.is_max() {
            self.cursor.index()
        } else {
            state_index_to_usize(offer_entry)
        })
    }

    fn route_scope_materialization_index(&self, scope_id: ScopeId) -> Option<usize> {
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id)
            && !offer_entry.is_max()
        {
            return Some(state_index_to_usize(offer_entry));
        }
        self.cursor
            .scope_region_by_id(scope_id)
            .map(|region| region.start)
    }

    fn preview_passive_materialization_index_for_selected_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<usize> {
        let mut scope = scope_id;
        let mut selected_arm = arm;
        let mut depth = 0usize;
        while depth < crate::eff::meta::MAX_EFF_NODES {
            if let Some(entry) = self.cursor.route_scope_arm_recv_index(scope, selected_arm) {
                return Some(entry);
            }
            let PassiveArmNavigation::WithinArm { entry } = self
                .cursor
                .follow_passive_observer_arm_for_scope(scope, selected_arm)?;
            let entry_idx = state_index_to_usize(entry);
            let target_cursor = self.cursor.with_index(entry_idx);
            if target_cursor.is_recv()
                || target_cursor.is_send()
                || target_cursor.is_local_action()
                || target_cursor.is_jump()
            {
                return Some(entry_idx);
            }
            let child_scope = self
                .cursor
                .passive_arm_scope_by_arm(scope, selected_arm)
                .or_else(|| {
                    let node_scope = target_cursor.node_scope_id();
                    (node_scope != scope && node_scope.kind() == ScopeKind::Route)
                        .then_some(node_scope)
                })?;
            selected_arm = self.preview_selected_arm_for_scope(child_scope)?;
            scope = child_scope;
            depth += 1;
        }
        None
    }

    fn preview_selected_arm_for_scope(&self, scope_id: ScopeId) -> Option<u8> {
        if let Some(arm) = self.selected_arm_for_scope(scope_id) {
            return Some(arm);
        }
        let (offer_lanes, offer_lanes_len) = self.offer_lanes_for_scope(scope_id);
        if offer_lanes_len == 0 {
            return None;
        }
        let mut offer_lane_mask = 0u8;
        let mut lane_idx = 0usize;
        while lane_idx < offer_lanes_len {
            let lane = offer_lanes[lane_idx] as usize;
            if lane < MAX_LANES {
                offer_lane_mask |= 1u8 << lane;
            }
            lane_idx += 1;
        }
        self.preview_scope_ack_token_non_consuming(
            scope_id,
            offer_lanes[0] as usize,
            offer_lane_mask,
        )
        .map(|token| token.arm().as_u8())
        .or_else(|| self.poll_arm_from_ready_mask(scope_id).map(Arm::as_u8))
    }

    fn scope_entry_index(&self, scope_id: ScopeId) -> Option<usize> {
        self.route_scope_offer_entry_index(scope_id).or_else(|| {
            self.cursor
                .scope_region_by_id(scope_id)
                .map(|region| region.start)
        })
    }

    fn scope_within_parent_arm(
        &self,
        child_scope: ScopeId,
        parent_scope: ScopeId,
        parent_arm: u8,
    ) -> bool {
        let Some(child_entry_idx) = self.scope_entry_index(child_scope) else {
            return false;
        };
        let arm_start = if let Some((entry, _)) = self
            .cursor
            .controller_arm_entry_by_arm(parent_scope, parent_arm)
        {
            state_index_to_usize(entry)
        } else if let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(parent_scope, parent_arm)
        {
            state_index_to_usize(entry)
        } else if let Some(entry) = self
            .cursor
            .route_scope_arm_recv_index(parent_scope, parent_arm)
        {
            entry
        } else {
            return false;
        };
        if child_entry_idx < arm_start {
            return false;
        }
        let sibling_arm = if parent_arm == 0 { 1 } else { 0 };
        if let Some((entry, _)) = self
            .cursor
            .controller_arm_entry_by_arm(parent_scope, sibling_arm)
        {
            let sibling_start = state_index_to_usize(entry);
            if parent_arm == 0 {
                return child_entry_idx < sibling_start;
            }
            return child_entry_idx >= arm_start;
        }
        if let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(parent_scope, sibling_arm)
        {
            let sibling_start = state_index_to_usize(entry);
            if parent_arm == 0 {
                return child_entry_idx < sibling_start;
            }
        }
        true
    }

    fn structural_arm_for_child_scope(
        &self,
        parent_scope: ScopeId,
        child_scope: ScopeId,
    ) -> Option<u8> {
        let child_in_arm0 = self.scope_within_parent_arm(child_scope, parent_scope, 0);
        let child_in_arm1 = self.scope_within_parent_arm(child_scope, parent_scope, 1);
        match (child_in_arm0, child_in_arm1) {
            (true, false) => Some(0),
            (false, true) => Some(1),
            _ => None,
        }
    }

    #[inline]
    fn current_offer_scope_id(&self) -> ScopeId {
        let node_scope = self.cursor.node_scope_id();
        if node_scope.is_none() {
            return node_scope;
        }
        let mut child_scope = node_scope;
        while let Some(parent_scope) = self.cursor.scope_parent(child_scope) {
            if parent_scope.kind() != ScopeKind::Route {
                child_scope = parent_scope;
                continue;
            }
            let child_selected_arm = self.selected_arm_for_scope(child_scope);
            let Some(parent_arm) = self
                .selected_arm_for_scope(parent_scope)
                .or_else(|| {
                    // Once we have descended into a selected child route, the
                    // ancestor arm is derivable from the structural placement
                    // of that child. Do not invent ancestor authority before
                    // the child itself has become selected.
                    if child_selected_arm.is_some() {
                        self.structural_arm_for_child_scope(parent_scope, child_scope)
                    } else {
                        None
                    }
                })
                .or_else(|| self.preview_selected_arm_for_scope(parent_scope))
            else {
                return parent_scope;
            };
            if !self.scope_within_parent_arm(child_scope, parent_scope, parent_arm) {
                return parent_scope;
            }
            child_scope = parent_scope;
        }
        node_scope
    }

    fn ensure_current_route_arm_state(&mut self) -> RecvResult<Option<bool>> {
        let Some(region) = self.cursor.scope_region() else {
            return Ok(None);
        };
        if region.kind != ScopeKind::Route {
            return Ok(None);
        }
        let Some(current_arm) = self.cursor.typestate_node(self.cursor.index()).route_arm() else {
            return Ok(None);
        };
        if let Some(selected_arm) = self.selected_arm_for_scope(region.scope_id) {
            return Ok((selected_arm == current_arm).then_some(false));
        }
        let lane = self.offer_lane_for_scope(region.scope_id);
        self.set_route_arm(lane, region.scope_id, current_arm)?;
        Ok(Some(true))
    }

    #[inline]
    fn endpoint_policy_args(lane: Lane, label: u8, flags: FrameFlags) -> u32 {
        ((ROLE as u32) << 24)
            | ((lane.as_wire() as u32) << 16)
            | ((label as u32) << 8)
            | flags.bits() as u32
    }

    /// Emit a policy-layer tap event associated with this endpoint.
    ///
    /// The event is tagged with the current lane and session so downstream
    /// tap inspection can attribute `POLICY_*` events to the correct
    /// rendezvous lane. Use this for recording resolver / EPF decisions such
    /// as `policy_effect`, `policy_trap`, or `policy_abort`.
    #[inline]
    fn emit_policy_event(&self, id: u16, arg0: u32, arg1: u32, scope: ScopeId, lane: Lane) {
        let port = self.port_for_lane(lane.raw() as usize);
        let causal = {
            let raw = lane.raw();
            debug_assert!(
                raw <= u32::from(u8::MAX),
                "lane id must fit within causal key encoding"
            );
            TapEvent::make_causal_key(raw as u8 + 1, 0)
        };
        let mut event = events::RawEvent::new(port.now32(), id)
            .with_causal_key(causal)
            .with_arg0(arg0)
            .with_arg1(arg1);
        if let Some(trace) = self.scope_trace(scope) {
            event = event.with_arg2(trace.pack());
        }
        emit(port.tap(), event);
    }

    #[inline]
    fn emit_policy_audit_event(&self, id: u16, arg0: u32, arg1: u32, arg2: u32, lane: Lane) {
        let port = self.port_for_lane(lane.raw() as usize);
        let causal = {
            let raw = lane.raw();
            debug_assert!(
                raw <= u32::from(u8::MAX),
                "lane id must fit within causal key encoding"
            );
            TapEvent::make_causal_key(raw as u8 + 1, 0)
        };
        let event = events::RawEvent::new(port.now32(), id)
            .with_causal_key(causal)
            .with_arg0(arg0)
            .with_arg1(arg1)
            .with_arg2(arg2);
        emit(port.tap(), event);
    }

    #[inline]
    fn emit_policy_defer_event(
        &self,
        source: DeferSource,
        reason: DeferReason,
        scope_id: ScopeId,
        frontier: FrontierKind,
        selected_arm: Option<u8>,
        hint: Option<u8>,
        retry_hint: u8,
        liveness: OfferLivenessState,
        ready_arm_mask: u8,
        binding_ready: bool,
        exhausted: bool,
        lane: u8,
    ) {
        let source_tag = match source {
            DeferSource::Epf => 1u32,
            DeferSource::Resolver => 2u32,
        };
        let scope_slot = self
            .scope_slot_for_route(scope_id)
            .and_then(|slot| u16::try_from(slot).ok())
            .unwrap_or(u16::MAX) as u32;
        let arm = selected_arm.unwrap_or(u8::MAX) as u32;
        let hint = hint.unwrap_or(0) as u32;
        let arg0 =
            (source_tag << 24) | ((retry_hint as u32) << 16) | (liveness.remaining_defer as u32);
        let arg1 = (scope_slot << 16) | (arm << 8) | (ready_arm_mask as u32);
        let arg2 = ((reason as u32) << 16)
            | (hint << 8)
            | ((frontier.as_audit_tag() as u32) << 4)
            | ((u32::from(binding_ready)) << 1)
            | u32::from(exhausted);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_DEFER,
            arg0,
            arg1,
            arg2,
            Lane::new(lane as u32),
        );
    }

    fn emit_endpoint_event(
        &self,
        id: u16,
        meta: TapFrameMeta,
        scope_trace: Option<ScopeTrace>,
        lane: u8,
    ) {
        let port = self.port_for_lane(lane as usize);
        let packed = ((ROLE as u32) << 24)
            | ((meta.lane as u32) << 16)
            | ((meta.label as u32) << 8)
            | meta.flags.bits() as u32;
        let mut event = events::RawEvent::new(port.now32(), id)
            .with_arg0(meta.sid)
            .with_arg1(packed);
        if let Some(scope) = scope_trace {
            event = event.with_arg2(scope.pack());
        }
        emit(port.tap(), event);
    }

    fn eval_endpoint_policy(
        &self,
        slot: Slot,
        event_id: u16,
        arg0: u32,
        arg1: u32,
        lane: Lane,
    ) -> Action {
        let port = self.port_for_lane(lane.raw() as usize);
        let event = events::RawEvent::new(port.now32(), event_id)
            .with_arg0(arg0)
            .with_arg1(arg1);
        let _ = port.flush_transport_events();
        let transport_metrics = port.transport().metrics().snapshot();
        let signals = self.policy_signals_for_slot(slot);
        let policy_input = signals.input;
        let policy_digest = port.host_slots().active_digest(slot);
        let event_hash = epf::hash_tap_event(&event);
        let signals_input_hash = epf::hash_policy_input(policy_input);
        let signals_attrs_hash = signals.attrs.hash32();
        let transport_snapshot_hash = epf::hash_transport_snapshot(transport_metrics);
        let replay_transport = epf::replay_transport_inputs(transport_metrics);
        let replay_transport_presence = epf::replay_transport_presence(transport_metrics);
        let slot_id = epf::slot_tag(slot);
        let mode_id = epf::policy_mode_tag(port.host_slots().policy_mode(slot));
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT,
            policy_digest,
            event_hash,
            signals_input_hash,
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_EXT,
            signals_attrs_hash,
            transport_snapshot_hash,
            ((slot_id as u32) << 24) | ((mode_id as u32) << 16),
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT,
            event.ts,
            event.id as u32,
            event.arg0,
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT_EXT,
            event.arg1,
            event.arg2,
            event.causal_key as u32,
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT0,
            policy_input[0],
            policy_input[1],
            policy_input[2],
            lane,
        );
        self.emit_policy_audit_event(ids::POLICY_REPLAY_INPUT1, policy_input[3], 0, 0, lane);
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT0,
            replay_transport[0],
            replay_transport[1],
            replay_transport[2],
            lane,
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT1,
            replay_transport[3],
            replay_transport_presence as u32,
            0,
            lane,
        );
        let action = epf::run_with(
            port.host_slots(),
            slot,
            &event,
            port.caps_mask(),
            Some(self.sid),
            Some(lane),
            move |ctx| {
                ctx.set_transport_snapshot(transport_metrics);
                ctx.set_policy_input(policy_input);
            },
        );
        let verdict = action.verdict();
        let verdict_meta =
            ((epf::verdict_tag(verdict) as u32) << 24) | ((epf::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_RESULT,
            verdict_meta,
            epf::verdict_reason(verdict) as u32,
            port.host_slots().last_fuel_used(slot) as u32,
            lane,
        );
        action
    }

    fn apply_send_policy(&self, action: Action, scope: ScopeId, lane: Lane) -> SendResult<()> {
        if let Action::Tap { id, arg0, arg1 } = action {
            self.emit_policy_event(id, arg0, arg1, scope, lane);
        }

        match action.verdict() {
            epf::PolicyVerdict::Proceed | epf::PolicyVerdict::RouteArm(_) => Ok(()),
            epf::PolicyVerdict::Reject(reason) => {
                if let Action::Abort(info) = action {
                    return Err(self.policy_abort_send(info, scope, lane));
                }
                self.emit_policy_event(policy_trap(), reason as u32, self.sid.raw(), scope, lane);
                self.emit_policy_event(policy_abort(), reason as u32, self.sid.raw(), scope, lane);
                Err(SendError::PolicyAbort { reason })
            }
        }
    }

    fn policy_abort_send(&self, info: AbortInfo, scope: ScopeId, lane: Lane) -> SendError {
        if info.trap.is_some() {
            self.emit_policy_event(
                policy_trap(),
                info.reason as u32,
                self.sid.raw(),
                scope,
                lane,
            );
        }
        self.emit_policy_event(
            policy_abort(),
            info.reason as u32,
            self.sid.raw(),
            scope,
            lane,
        );
        SendError::PolicyAbort {
            reason: info.reason,
        }
    }

    fn apply_recv_policy(&self, action: Action, scope: ScopeId, lane: Lane) -> RecvResult<()> {
        if let Action::Tap { id, arg0, arg1 } = action {
            self.emit_policy_event(id, arg0, arg1, scope, lane);
        }

        match action.verdict() {
            epf::PolicyVerdict::Proceed | epf::PolicyVerdict::RouteArm(_) => Ok(()),
            epf::PolicyVerdict::Reject(reason) => {
                if let Action::Abort(info) = action {
                    return Err(self.policy_abort_recv(info, scope, lane));
                }
                self.emit_policy_event(policy_trap(), reason as u32, self.sid.raw(), scope, lane);
                self.emit_policy_event(policy_abort(), reason as u32, self.sid.raw(), scope, lane);
                Err(RecvError::PolicyAbort { reason })
            }
        }
    }

    fn policy_abort_recv(&self, info: AbortInfo, scope: ScopeId, lane: Lane) -> RecvError {
        if info.trap.is_some() {
            self.emit_policy_event(
                policy_trap(),
                info.reason as u32,
                self.sid.raw(),
                scope,
                lane,
            );
        }
        self.emit_policy_event(
            policy_abort(),
            info.reason as u32,
            self.sid.raw(),
            scope,
            lane,
        );
        RecvError::PolicyAbort {
            reason: info.reason,
        }
    }

    /// Create a CapFlow for the current send transition.
    ///
    /// This is the primary entry point for sending messages. Returns a `CapFlow`
    /// that must be consumed by calling `.send(arg).await`.
    ///
    /// Automatically handles routing: if the target label doesn't match the current
    /// cursor position, attempts to advance to the correct branch.
    fn prepare_flow<M>(mut self) -> SendResult<(Self, SendMeta)>
    where
        M: MessageSpec + SendableLabel,
        T: Transport + 'r,
        U: LabelUniverse,
        C: Clock,
        E: EpochTable,
        Mint: MintConfigMarker,
    {
        let target_label = <M as MessageSpec>::LABEL;
        self.try_select_lane_for_label(target_label);

        // For Route scopes, handle cursor repositioning at controller arm entry points.
        // This covers both linger (loops) and non-linger routes when the controller
        // needs to select a different arm than the current cursor position.
        if let Some(region) = self.cursor.scope_region() {
            if region.kind == ScopeKind::Route {
                // For linger scopes (loops), follow any pending Jump nodes first.
                // LoopContinue jumps back to loop_start, then we reposition to the target arm.
                if region.linger && self.cursor.is_jump() {
                    self.set_cursor(
                        self.cursor
                            .try_follow_jumps()
                            .map_err(|_| SendError::PhaseInvariant)?,
                    );
                }

                let scope_id = region.scope_id;
                if self.cursor.is_route_controller(scope_id) {
                    let at_route_start = self.cursor.index() == region.start;
                    let at_arm_entry = self.cursor.is_at_controller_arm_entry(scope_id);
                    let at_decision =
                        at_arm_entry || at_route_start || self.cursor.label().is_none();
                    if at_decision {
                        // Use O(1) controller_arm_entry registry lookup to reposition
                        // cursor to the arm entry matching target_label.
                        if let Some(entry_idx) = self
                            .cursor
                            .controller_arm_entry_for_label(scope_id, target_label)
                        {
                            self.set_cursor(
                                self.cursor.with_index(state_index_to_usize(entry_idx)),
                            );
                        }
                    }
                }
            }
        }

        let mut flow_iter = 0u32;
        loop {
            flow_iter += 1;
            debug_assert!(
                flow_iter <= crate::eff::meta::MAX_EFF_NODES as u32,
                "flow(): exceeded MAX_EFF_NODES iterations - CFG cycle bug"
            );
            if flow_iter > crate::eff::meta::MAX_EFF_NODES as u32 {
                return Err(SendError::PhaseInvariant);
            }

            // Follow Jump nodes (LoopContinue, LoopBreak, RouteArmEnd are auto-followed).
            // Only PassiveObserverBranch stops here.
            if self.cursor.is_jump() {
                self.set_cursor(
                    self.cursor
                        .try_follow_jumps()
                        .map_err(|_| SendError::PhaseInvariant)?,
                );
            }

            // Handle PassiveObserverBranch Jump: use structured arm navigation
            // instead of scanning the entire scope for the target label.
            if self.cursor.is_jump() {
                if let Some(JumpReason::PassiveObserverBranch) = self.cursor.jump_reason() {
                    // Find which arm contains the target label and follow the corresponding Jump
                    if let Some(new_cursor) =
                        self.cursor.follow_passive_observer_for_label(target_label)
                    {
                        self.set_cursor(new_cursor);
                        continue;
                    }
                }
            }

            // Accept both Send and Local actions
            if !self.cursor.is_send() && !self.cursor.is_local_action() {
                if let Some(region) = self.cursor.scope_region() {
                    if region.kind == ScopeKind::Route
                        && self.can_advance_route_scope(region.scope_id, target_label)
                    {
                        if let Some(cursor) = self.cursor.advance_scope_if_kind(ScopeKind::Route) {
                            self.set_cursor(cursor);
                            continue;
                        }
                    }
                }
                return Err(SendError::PhaseInvariant);
            }

            // Get metadata: for Local actions, create SendMeta with peer=ROLE
            let current_meta = if self.cursor.is_local_action() {
                let local = self
                    .cursor
                    .try_local_meta()
                    .ok_or(SendError::PhaseInvariant)?;
                SendMeta::new(
                    local.eff_index,
                    ROLE,
                    local.label,
                    local.resource,
                    local.is_control,
                    local.next,
                    local.scope,
                    local.route_arm,
                    local.shot,
                    local.policy,
                    local.lane,
                )
            } else {
                self.cursor
                    .try_send_meta()
                    .ok_or(SendError::PhaseInvariant)?
            };

            if current_meta.label == target_label {
                self.evaluate_dynamic_policy(&current_meta, target_label)?;
                return Ok((self, current_meta));
            }

            // Label mismatch: try advancing past Route scope boundary.
            // No O(n) seek_label scan - cursor must be at correct position.
            if let Some(region) = self.cursor.scope_region() {
                if region.kind == ScopeKind::Route
                    && self.can_advance_route_scope(region.scope_id, target_label)
                {
                    if let Some(cursor) = self.cursor.advance_scope_if_kind(ScopeKind::Route) {
                        self.set_cursor(cursor);
                        continue;
                    }
                }
            }

            return Err(SendError::LabelMismatch {
                expected: current_meta.label,
                actual: target_label,
            });
        }
    }

    pub(crate) fn flow<M>(self) -> SendResult<CapFlow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>>
    where
        M: MessageSpec + SendableLabel,
        T: Transport + 'r,
        U: LabelUniverse,
        C: Clock,
        E: EpochTable,
        Mint: MintConfigMarker,
    {
        let (endpoint, meta) = self.prepare_flow::<M>()?;
        Ok(CapFlow::new(endpoint, meta))
    }

    fn evaluate_dynamic_policy(&mut self, meta: &SendMeta, target_label: u8) -> SendResult<()> {
        if !meta.policy().is_dynamic() {
            return Ok(());
        }
        let dynamic_kind = classify_dynamic_label(target_label);
        if matches!(dynamic_kind, DynamicLabelClass::SpliceOrReroute) {
            return Ok(());
        }
        let route_signals = self.policy_signals_for_slot(Slot::Route);
        match dynamic_kind {
            DynamicLabelClass::Loop => self.evaluate_loop_policy(meta, route_signals),
            DynamicLabelClass::Route => {
                self.evaluate_route_policy(meta, target_label, route_signals)
            }
            DynamicLabelClass::SpliceOrReroute => Ok(()),
        }
    }

    fn evaluate_route_arm_from_epf(
        &self,
        scope_id: ScopeId,
        lane: u8,
        policy_id: u16,
        signals: crate::transport::context::PolicySignals,
    ) -> RoutePolicyDecision {
        if scope_id.is_none() {
            return RoutePolicyDecision::Abort(policy_id);
        }
        let port = self.port_for_lane(lane as usize);
        let _ = port.flush_transport_events();
        let transport_metrics = port.transport().metrics().snapshot();
        let policy_input = signals.input;
        let arg0 = route_policy_input_arg0(&policy_input);
        let mut event = events::RawEvent::new(port.now32(), ids::ROUTE_DECISION)
            .with_arg0(arg0)
            .with_arg1(policy_id as u32);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        let policy_digest = port.host_slots().active_digest(Slot::Route);
        let event_hash = epf::hash_tap_event(&event);
        let signals_input_hash = epf::hash_policy_input(policy_input);
        let signals_attrs_hash = signals.attrs.hash32();
        let transport_snapshot_hash = epf::hash_transport_snapshot(transport_metrics);
        let replay_transport = epf::replay_transport_inputs(transport_metrics);
        let replay_transport_presence = epf::replay_transport_presence(transport_metrics);
        let mode_id = epf::policy_mode_tag(port.host_slots().policy_mode(Slot::Route));
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT,
            policy_digest,
            event_hash,
            signals_input_hash,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_EXT,
            signals_attrs_hash,
            transport_snapshot_hash,
            ((epf::slot_tag(Slot::Route) as u32) << 24) | ((mode_id as u32) << 16),
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT,
            event.ts,
            event.id as u32,
            event.arg0,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_EVENT_EXT,
            event.arg1,
            event.arg2,
            event.causal_key as u32,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT0,
            policy_input[0],
            policy_input[1],
            policy_input[2],
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_INPUT1,
            policy_input[3],
            0,
            0,
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT0,
            replay_transport[0],
            replay_transport[1],
            replay_transport[2],
            port.lane(),
        );
        self.emit_policy_audit_event(
            ids::POLICY_REPLAY_TRANSPORT1,
            replay_transport[3],
            replay_transport_presence as u32,
            0,
            port.lane(),
        );
        let action = epf::run_with(
            port.host_slots(),
            Slot::Route,
            &event,
            port.caps_mask(),
            Some(self.sid),
            Some(port.lane()),
            move |ctx| {
                ctx.set_transport_snapshot(transport_metrics);
                ctx.set_policy_input(policy_input);
            },
        );
        let verdict = action.verdict();
        let verdict_meta =
            ((epf::verdict_tag(verdict) as u32) << 24) | ((epf::verdict_arm(verdict) as u32) << 16);
        self.emit_policy_audit_event(
            ids::POLICY_AUDIT_RESULT,
            verdict_meta,
            epf::verdict_reason(verdict) as u32,
            port.host_slots().last_fuel_used(Slot::Route) as u32,
            port.lane(),
        );
        route_policy_decision_from_action(action, policy_id)
    }

    fn evaluate_route_policy(
        &mut self,
        meta: &SendMeta,
        target_label: u8,
        signals: crate::transport::context::PolicySignals,
    ) -> SendResult<()> {
        let policy = meta.policy();
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(SendError::PhaseInvariant)?;

        if meta.label != target_label {
            return Err(SendError::PhaseInvariant);
        }
        // Route decisions are fixed at the offer/decode decision point.
        // Re-evaluating dynamic route policy for local self-send can diverge from
        // the selected arm and introduce non-deterministic PolicyAbort.
        if meta.peer == ROLE {
            return Ok(());
        }

        let scope_id = meta.scope;
        let arm_index = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != policy.scope() {
            return Err(SendError::PhaseInvariant);
        }

        match self.evaluate_route_arm_from_epf(scope_id, meta.lane, policy_id, signals) {
            RoutePolicyDecision::RouteArm(arm) => {
                return if arm == arm_index {
                    Ok(())
                } else {
                    Err(SendError::PolicyAbort { reason: policy_id })
                };
            }
            RoutePolicyDecision::Abort(reason) => {
                return Err(SendError::PolicyAbort { reason });
            }
            RoutePolicyDecision::DelegateResolver | RoutePolicyDecision::Defer { .. } => {}
        }

        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let metrics = port.transport().metrics().snapshot();
        let attrs = signals.attrs;
        let resolution = cluster
            .resolve_dynamic_policy(
                self.rendezvous_id(),
                Some(SessionId::new(self.sid.raw())),
                Lane::new(port.lane().raw()),
                meta.eff_index,
                tag,
                metrics,
                signals.input,
                attrs,
            )
            .map_err(Self::map_cp_error)?;

        match resolution {
            DynamicResolution::RouteArm { arm } if arm == arm_index => Ok(()),
            DynamicResolution::RouteArm { .. } => Err(SendError::PolicyAbort { reason: policy_id }),
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }

    fn evaluate_loop_policy(
        &mut self,
        meta: &SendMeta,
        signals: crate::transport::context::PolicySignals,
    ) -> SendResult<()> {
        // For CanonicalControl (self-send), the caller explicitly chooses continue/break.
        // No resolver validation is needed - the caller's choice is authoritative.
        if meta.peer == ROLE {
            return Ok(());
        }

        let policy = meta.policy();
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(SendError::PhaseInvariant)?;
        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;

        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        let port = self.port_for_lane(meta.lane as usize);
        port.flush_transport_events();
        let metrics = port.transport().metrics().snapshot();
        let attrs = signals.attrs;
        let resolution = cluster
            .resolve_dynamic_policy(
                self.rendezvous_id(),
                Some(SessionId::new(self.sid.raw())),
                Lane::new(port.lane().raw()),
                meta.eff_index,
                tag,
                metrics,
                signals.input,
                attrs,
            )
            .map_err(Self::map_cp_error)?;

        if meta.scope.is_none() || meta.scope != policy.scope() {
            return Err(SendError::PhaseInvariant);
        }

        match resolution {
            DynamicResolution::Loop { decision } => {
                let disposition = if decision {
                    LoopDisposition::Continue
                } else {
                    LoopDisposition::Break
                };
                let expected_label = match disposition {
                    LoopDisposition::Continue => LABEL_LOOP_CONTINUE,
                    LoopDisposition::Break => LABEL_LOOP_BREAK,
                };
                if expected_label != meta.label {
                    return Err(SendError::PolicyAbort { reason: policy_id });
                }
                Ok(())
            }
            _ => Err(SendError::PolicyAbort { reason: policy_id }),
        }
    }

    /// Materialize recv metadata from a precomputed route-arm entry table.
    fn select_cached_route_arm_recv_meta(
        &mut self,
        materialization_meta: ScopeArmMaterializationMeta,
        target_arm: u8,
    ) -> Option<RecvMeta> {
        let idx = state_index_to_usize(materialization_meta.recv_entry(target_arm)?);
        self.set_cursor(self.cursor.with_index(idx));
        let mut meta = self.cursor.try_recv_meta()?;
        if meta.route_arm.is_none() {
            meta.route_arm = Some(target_arm);
        }
        Some(meta)
    }

    #[inline]
    fn cached_recv_meta_from_recv(
        cursor_index: usize,
        mut meta: RecvMeta,
        route_arm: Option<u8>,
    ) -> Option<CachedRecvMeta> {
        let cursor_index = checked_state_index(cursor_index)?;
        let next = checked_state_index(meta.next)?;
        if let Some(route_arm) = route_arm {
            meta.route_arm = Some(route_arm);
        }
        Some(CachedRecvMeta {
            cursor_index,
            eff_index: meta.eff_index,
            peer: meta.peer,
            label: meta.label,
            resource: meta.resource,
            is_control: meta.is_control,
            next,
            scope: meta.scope,
            route_arm: meta.route_arm.unwrap_or(u8::MAX),
            is_choice_determinant: meta.is_choice_determinant,
            shot: meta.shot,
            policy: meta.policy,
            lane: meta.lane,
            flags: CachedRecvMeta::FLAG_RECV_STEP,
        })
    }

    #[inline]
    fn cached_recv_meta_from_send(
        cursor_index: usize,
        scope_id: ScopeId,
        route_arm: u8,
        meta: SendMeta,
    ) -> Option<CachedRecvMeta> {
        Some(CachedRecvMeta {
            cursor_index: checked_state_index(cursor_index)?,
            eff_index: meta.eff_index,
            peer: meta.peer,
            label: meta.label,
            resource: meta.resource,
            is_control: meta.is_control,
            next: checked_state_index(meta.next)?,
            scope: scope_id,
            route_arm,
            is_choice_determinant: false,
            shot: meta.shot,
            policy: meta.policy(),
            lane: meta.lane,
            flags: 0,
        })
    }

    #[inline]
    fn synthetic_cached_recv_meta(
        cursor_index: usize,
        scope_id: ScopeId,
        route_arm: u8,
        label: u8,
        next: usize,
        lane: u8,
    ) -> Option<CachedRecvMeta> {
        Some(CachedRecvMeta {
            cursor_index: checked_state_index(cursor_index)?,
            eff_index: EffIndex::ZERO,
            peer: ROLE,
            label,
            resource: None,
            is_control: true,
            next: checked_state_index(next)?,
            scope: scope_id,
            route_arm,
            is_choice_determinant: false,
            shot: None,
            policy: PolicyMode::static_mode(),
            lane,
            flags: 0,
        })
    }

    fn compute_passive_arm_recv_meta(
        &self,
        materialization_meta: ScopeArmMaterializationMeta,
        scope_id: ScopeId,
        target_arm: u8,
        offer_lane: u8,
    ) -> CachedRecvMeta {
        let Some(entry) = materialization_meta.passive_arm_entry(target_arm) else {
            return CachedRecvMeta::EMPTY;
        };
        let target_cursor = self.cursor.with_index(state_index_to_usize(entry));
        if let Some(recv_meta) = target_cursor.try_recv_meta() {
            return Self::cached_recv_meta_from_recv(target_cursor.index(), recv_meta, None)
                .unwrap_or(CachedRecvMeta::EMPTY);
        }
        if let Some(send_meta) = target_cursor.try_send_meta() {
            return Self::cached_recv_meta_from_send(
                target_cursor.index(),
                scope_id,
                target_arm,
                send_meta,
            )
            .unwrap_or(CachedRecvMeta::EMPTY);
        }
        let Some(region) = self.cursor.scope_region_by_id(scope_id) else {
            return CachedRecvMeta::EMPTY;
        };
        if target_cursor.is_jump() {
            let Some(scope_end) = target_cursor.jump_target() else {
                return CachedRecvMeta::EMPTY;
            };
            let scope_end_cursor = self.cursor.with_index(scope_end);
            if region.linger {
                let synthetic_label = match target_arm {
                    0 => LABEL_LOOP_CONTINUE,
                    1 => LABEL_LOOP_BREAK,
                    _ => return CachedRecvMeta::EMPTY,
                };
                return Self::synthetic_cached_recv_meta(
                    scope_end,
                    scope_id,
                    target_arm,
                    synthetic_label,
                    scope_end,
                    offer_lane,
                )
                .unwrap_or(CachedRecvMeta::EMPTY);
            }
            if let Some(recv_meta) = scope_end_cursor.try_recv_meta() {
                return Self::cached_recv_meta_from_recv(scope_end, recv_meta, None)
                    .unwrap_or(CachedRecvMeta::EMPTY);
            }
            if let Some(send_meta) = scope_end_cursor.try_send_meta() {
                return Self::cached_recv_meta_from_send(
                    scope_end, scope_id, target_arm, send_meta,
                )
                .unwrap_or(CachedRecvMeta::EMPTY);
            }
            return CachedRecvMeta::EMPTY;
        }
        if region.linger {
            let synthetic_label = match target_arm {
                0 => LABEL_LOOP_CONTINUE,
                1 => LABEL_LOOP_BREAK,
                _ => return CachedRecvMeta::EMPTY,
            };
            return Self::synthetic_cached_recv_meta(
                target_cursor.index(),
                scope_id,
                target_arm,
                synthetic_label,
                target_cursor.index(),
                offer_lane,
            )
            .unwrap_or(CachedRecvMeta::EMPTY);
        }
        if let Some(target_idx) =
            self.preview_passive_materialization_index_for_selected_arm(scope_id, target_arm)
        {
            let target_cursor = self.cursor.with_index(target_idx);
            if let Some(recv_meta) = target_cursor.try_recv_meta() {
                return Self::cached_recv_meta_from_recv(target_idx, recv_meta, Some(target_arm))
                    .unwrap_or(CachedRecvMeta::EMPTY);
            }
            if let Some(send_meta) = target_cursor.try_send_meta() {
                return Self::cached_recv_meta_from_send(target_idx, scope_id, target_arm, send_meta)
                    .unwrap_or(CachedRecvMeta::EMPTY);
            }
        }
        CachedRecvMeta::EMPTY
    }

    #[inline]
    fn compute_scope_passive_recv_meta(
        &self,
        materialization_meta: ScopeArmMaterializationMeta,
        scope_id: ScopeId,
        offer_lane: u8,
    ) -> [CachedRecvMeta; 2] {
        [
            self.compute_passive_arm_recv_meta(materialization_meta, scope_id, 0, offer_lane),
            self.compute_passive_arm_recv_meta(materialization_meta, scope_id, 1, offer_lane),
        ]
    }

    #[inline]
    fn selection_arm_has_recv(&self, selection: OfferScopeSelection, arm: u8) -> bool {
        selection.materialization_meta.recv_entry(arm).is_some()
            || selection.materialization_meta.controller_arm_is_recv(arm)
            || selection.materialization_meta.arm_has_first_recv_dispatch(arm)
            || selection
                .passive_recv_meta
                .get(arm as usize)
                .copied()
                .map(CachedRecvMeta::is_recv_step)
                .unwrap_or(false)
    }

    #[inline]
    fn selection_arm_requires_materialization_ready_evidence(
        &self,
        selection: OfferScopeSelection,
        is_route_controller: bool,
        arm: u8,
    ) -> bool {
        if is_route_controller && selection.at_route_offer_entry {
            if selection
                .materialization_meta
                .controller_arm_entry(arm)
                .is_some()
            {
                return selection
                    .materialization_meta
                    .controller_arm_requires_ready_evidence(arm);
            }
        }
        if selection.at_route_offer_entry
            && selection
                .materialization_meta
                .passive_arm_entry(arm)
                .is_some()
        {
            if selection.materialization_meta.arm_has_first_recv_dispatch(arm) {
                return !self.selection_arm_dispatch_materializes_without_ready_evidence(
                    selection, arm,
                );
            }
            return false;
        }
        let Some(passive_meta) = selection.passive_recv_meta.get(arm as usize).copied() else {
            return selection.materialization_meta.recv_entry(arm).is_some();
        };
        if passive_meta.is_recv_step() {
            if passive_meta.peer == ROLE {
                return false;
            }
            if passive_meta.is_control {
                if selection
                    .materialization_meta
                    .controller_arm_entry(arm)
                    .map(|(_, label)| label)
                    == Some(passive_meta.label)
                {
                    return false;
                }
                if !is_route_controller
                    && matches!(passive_meta.label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK)
                {
                    return false;
                }
            }
            return true;
        }
        selection.materialization_meta.recv_entry(arm).is_some()
    }

    #[inline]
    fn selection_arm_dispatch_materializes_without_ready_evidence(
        &self,
        selection: OfferScopeSelection,
        arm: u8,
    ) -> bool {
        let Some(entry) = selection.materialization_meta.passive_arm_entry(arm) else {
            return false;
        };
        let target_cursor = self.cursor.with_index(state_index_to_usize(entry));
        if target_cursor.is_recv()
            || target_cursor.is_send()
            || target_cursor.is_local_action()
            || target_cursor.is_jump()
        {
            return true;
        }
        selection
            .materialization_meta
            .passive_arm_scope(arm)
            .or_else(|| {
                let scope = target_cursor.node_scope_id();
                (scope != selection.scope_id && scope.kind() == ScopeKind::Route).then_some(scope)
            })
            .filter(|scope| scope.kind() == ScopeKind::Route)
            .and_then(|scope| self.preview_selected_arm_for_scope(scope))
            .is_some()
    }

    #[inline]
    fn selection_non_wire_loop_control_recv(
        &self,
        selection: OfferScopeSelection,
        is_route_controller: bool,
        arm: u8,
        label: u8,
    ) -> bool {
        let Some(passive_meta) = selection.passive_recv_meta.get(arm as usize).copied() else {
            return false;
        };
        passive_meta.is_recv_step()
            && passive_meta.is_control
            && passive_meta.label == label
            && (passive_meta.peer == ROLE
                || (!is_route_controller
                    && matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK)))
    }

    /// Materialize recv metadata from a precomputed first-recv dispatch table.
    fn select_cached_dispatch_recv_meta(
        &mut self,
        materialization_meta: ScopeArmMaterializationMeta,
        target_arm: u8,
        resolved_label_hint: Option<u8>,
    ) -> Option<RecvMeta> {
        let label = resolved_label_hint?;
        let (dispatch_arm, target_idx) = materialization_meta.first_recv_target(label)?;
        if dispatch_arm != ARM_SHARED && dispatch_arm != target_arm {
            return None;
        }
        let target_cursor = self.cursor.with_index(state_index_to_usize(target_idx));
        self.set_cursor(target_cursor);
        let mut meta = target_cursor.try_recv_meta()?;
        if meta.route_arm.is_none() {
            meta.route_arm = Some(if dispatch_arm == ARM_SHARED {
                target_arm
            } else {
                dispatch_arm
            });
        }
        Some(meta)
    }

    fn materialize_selected_arm_meta(
        &mut self,
        selection: OfferScopeSelection,
        selected_arm: u8,
        resolved_label_hint: Option<u8>,
    ) -> RecvResult<RecvMeta> {
        let scope_id = selection.scope_id;
        let selected_label_meta = selection.label_meta;
        let materialization_meta = selection.materialization_meta;
        let passive_recv_meta = selection.passive_recv_meta;
        let controller_arm_entry = if selection.at_route_offer_entry {
            materialization_meta.controller_arm_entry(selected_arm)
        } else {
            None
        };
        let dispatch_meta = if controller_arm_entry.is_none() {
            self.select_cached_dispatch_recv_meta(
                materialization_meta,
                selected_arm,
                resolved_label_hint,
            )
        } else {
            None
        };

        let direct_meta = if let Some((arm_entry_idx, arm_entry_label)) = controller_arm_entry {
            let target_cursor = self.cursor.with_index(state_index_to_usize(arm_entry_idx));
            self.set_cursor(target_cursor);

            if let Some(local_meta) = target_cursor.try_local_meta() {
                Some(RecvMeta {
                    eff_index: local_meta.eff_index,
                    label: local_meta.label,
                    peer: ROLE,
                    resource: local_meta.resource,
                    is_control: local_meta.is_control,
                    next: local_meta.next,
                    scope: local_meta.scope,
                    route_arm: Some(selected_arm),
                    is_choice_determinant: false,
                    shot: local_meta.shot,
                    policy: local_meta.policy,
                    lane: local_meta.lane,
                })
            } else {
                Some(RecvMeta {
                    eff_index: EffIndex::ZERO,
                    label: arm_entry_label,
                    peer: ROLE,
                    resource: None,
                    is_control: true,
                    next: target_cursor.index(),
                    scope: scope_id,
                    route_arm: Some(selected_arm),
                    is_choice_determinant: false,
                    shot: None,
                    policy: crate::global::const_dsl::PolicyMode::static_mode(),
                    lane: selection.offer_lane,
                })
            }
        } else if let Some(meta) = dispatch_meta {
            Some(meta)
        } else if selected_arm < materialization_meta.arm_count {
            self.select_cached_route_arm_recv_meta(materialization_meta, selected_arm)
        } else {
            None
        };

        let mut meta = if let Some(meta) = direct_meta {
            meta
        } else {
            let (cursor_index, meta) = passive_recv_meta
                .get(selected_arm as usize)
                .copied()
                .and_then(CachedRecvMeta::recv_meta)
                .ok_or(RecvError::PhaseInvariant)?;
            self.set_cursor(self.cursor.with_index(cursor_index));
            meta
        };

        if self.selection_arm_has_recv(selection, selected_arm)
            && let Some(resolved_label) = resolved_label_hint
        {
            if Self::scope_label_to_arm(selected_label_meta, resolved_label) == Some(selected_arm) {
                meta.label = resolved_label;
            }
        }

        Ok(meta)
    }

    fn descend_selected_passive_route(
        &mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteDecision,
    ) -> RecvResult<bool> {
        if resolved.resolved_label_hint.is_some() {
            return Ok(false);
        }
        let scope_id = selection.scope_id;
        let selected_arm = resolved.selected_arm;
        let Some(nested_scope) = selection
            .materialization_meta
            .passive_arm_scope(selected_arm)
        else {
            return Ok(false);
        };
        if nested_scope == scope_id || nested_scope.kind() != ScopeKind::Route {
            return Ok(false);
        }
        self.propagate_recvless_parent_route_decision(scope_id, selected_arm);
        if matches!(resolved.route_token.source(), RouteDecisionSource::Poll) {
            self.emit_route_decision(
                scope_id,
                selected_arm,
                RouteDecisionSource::Poll,
                selection.offer_lane,
            );
        }
        self.set_route_arm(selection.offer_lane, scope_id, selected_arm)?;
        let mut target_scope = nested_scope;
        loop {
            let target_preview_arm = self.preview_selected_arm_for_scope(target_scope);
            if let Some(arm) = target_preview_arm {
                self.set_route_arm(selection.offer_lane, target_scope, arm)?;
                if let Some(child_scope) = self.cursor.passive_arm_scope_by_arm(target_scope, arm)
                    && child_scope.kind() == ScopeKind::Route
                {
                    target_scope = child_scope;
                    continue;
                }
            }
            let target_index = self
                .route_scope_materialization_index(target_scope)
                .ok_or(RecvError::PhaseInvariant)?;
            self.set_cursor(self.cursor.with_index(target_index));
            break;
        }
        self.align_cursor_to_selected_scope()?;
        Ok(true)
    }

    fn emit_route_decision(
        &self,
        scope_id: ScopeId,
        arm: u8,
        source: RouteDecisionSource,
        lane: u8,
    ) {
        let port = self.port_for_lane(lane as usize);
        let causal = TapEvent::make_causal_key(port.lane().as_wire(), source.as_tap_seq());
        let arg0 = self.sid.raw();
        let arg1 = ((scope_id.raw() as u32) << 16) | (arm as u32);
        let mut event = events::RouteDecision::with_causal(port.now32(), causal, arg0, arg1);
        if let Some(trace) = self.scope_trace(scope_id) {
            event = event.with_arg2(trace.pack());
        }
        emit(port.tap(), event);
    }

    fn prepare_route_decision_from_resolver(
        &mut self,
        scope_id: ScopeId,
        signals: crate::transport::context::PolicySignals,
    ) -> RecvResult<RouteResolveStep> {
        let (policy, eff_index, tag) = self
            .cursor
            .route_scope_controller_policy(scope_id)
            .ok_or(RecvError::PhaseInvariant)?;
        if !policy.is_dynamic() {
            return Err(RecvError::PhaseInvariant);
        }
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(RecvError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != policy.scope() {
            return Err(RecvError::PhaseInvariant);
        }
        let offer_lane = self.offer_lane_for_scope(scope_id);
        match self.evaluate_route_arm_from_epf(scope_id, offer_lane, policy_id, signals) {
            RoutePolicyDecision::RouteArm(arm) => {
                let arm = Arm::new(arm).ok_or(RecvError::PhaseInvariant)?;
                self.record_route_decision_for_lane(offer_lane as usize, scope_id, arm.as_u8());
                self.record_scope_ack(scope_id, RouteDecisionToken::from_resolver(arm));
                self.emit_route_decision(
                    scope_id,
                    arm.as_u8(),
                    RouteDecisionSource::Resolver,
                    offer_lane,
                );
                return Ok(RouteResolveStep::Resolved(arm));
            }
            RoutePolicyDecision::Abort(reason) => return Ok(RouteResolveStep::Abort(reason)),
            RoutePolicyDecision::Defer { retry_hint, source } => {
                return Ok(RouteResolveStep::Deferred { retry_hint, source });
            }
            RoutePolicyDecision::DelegateResolver => {}
        }
        let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
        let rv_id = RendezvousId::new(self.rendezvous_id().raw());
        let port = self.port_for_lane(offer_lane as usize);
        let lane = Lane::new(port.lane().raw());
        let metrics = port.transport().metrics().snapshot();
        let attrs = signals.attrs;
        let resolution = match cluster.resolve_dynamic_policy(
            rv_id,
            None,
            lane,
            eff_index,
            tag,
            metrics,
            signals.input,
            attrs,
        ) {
            Ok(resolution) => resolution,
            Err(CpError::PolicyAbort { reason }) => return Ok(RouteResolveStep::Abort(reason)),
            Err(_) => return Err(RecvError::PhaseInvariant),
        };
        let arm = match resolution {
            DynamicResolution::RouteArm { arm } => arm,
            DynamicResolution::Loop { decision } => {
                if decision {
                    0
                } else {
                    1
                }
            }
            DynamicResolution::Defer { retry_hint } => {
                return Ok(RouteResolveStep::Deferred {
                    retry_hint,
                    source: DeferSource::Resolver,
                });
            }
            _ => return Err(RecvError::PhaseInvariant),
        };
        let arm = Arm::new(arm).ok_or(RecvError::PhaseInvariant)?;
        self.record_route_decision_for_lane(offer_lane as usize, scope_id, arm.as_u8());
        self.record_scope_ack(scope_id, RouteDecisionToken::from_resolver(arm));
        self.emit_route_decision(
            scope_id,
            arm.as_u8(),
            RouteDecisionSource::Resolver,
            offer_lane,
        );
        Ok(RouteResolveStep::Resolved(arm))
    }

    /// Route decision via controller_arm_entry labels.
    fn prepare_route_decision_from_resolver_via_arm_entry(
        &mut self,
        scope_id: ScopeId,
        signals: crate::transport::context::PolicySignals,
    ) -> RecvResult<RouteResolveStep> {
        // Get arm 0's entry to find the label used for resolver lookup
        let (arm0_entry, _arm0_label) = self
            .cursor
            .controller_arm_entry_by_arm(scope_id, 0)
            .ok_or(RecvError::PhaseInvariant)?;

        // Navigate to arm0_entry to get the node's metadata
        let arm0_cursor = self.cursor.with_index(state_index_to_usize(arm0_entry));

        // The arm entry node should be a Local (self-send) node with a policy annotation.
        let local_meta = arm0_cursor
            .try_local_meta()
            .ok_or(RecvError::PhaseInvariant)?;

        let policy = local_meta.policy;
        if !policy.is_dynamic() {
            return Ok(RouteResolveStep::Abort(0));
        }
        let policy_id = policy
            .dynamic_policy_id()
            .ok_or(RecvError::PhaseInvariant)?;
        if scope_id.is_none() || scope_id != policy.scope() {
            return Err(RecvError::PhaseInvariant);
        }
        match self.evaluate_route_arm_from_epf(scope_id, local_meta.lane, policy_id, signals) {
            RoutePolicyDecision::RouteArm(arm) => {
                let arm = Arm::new(arm).ok_or(RecvError::PhaseInvariant)?;
                self.record_route_decision_for_lane(
                    local_meta.lane as usize,
                    scope_id,
                    arm.as_u8(),
                );
                self.record_scope_ack(scope_id, RouteDecisionToken::from_resolver(arm));
                self.emit_route_decision(
                    scope_id,
                    arm.as_u8(),
                    RouteDecisionSource::Resolver,
                    local_meta.lane,
                );
                return Ok(RouteResolveStep::Resolved(arm));
            }
            RoutePolicyDecision::Abort(reason) => return Ok(RouteResolveStep::Abort(reason)),
            RoutePolicyDecision::Defer { retry_hint, source } => {
                return Ok(RouteResolveStep::Deferred { retry_hint, source });
            }
            RoutePolicyDecision::DelegateResolver => {}
        }

        let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
        let rv_id = RendezvousId::new(self.rendezvous_id().raw());
        let port = self.port_for_lane(local_meta.lane as usize);
        let lane = Lane::new(port.lane().raw());
        let metrics = port.transport().metrics().snapshot();
        let attrs = signals.attrs;
        let tag = local_meta.resource.unwrap_or(0);
        let resolution = match cluster.resolve_dynamic_policy(
            rv_id,
            None,
            lane,
            local_meta.eff_index,
            tag,
            metrics,
            signals.input,
            attrs,
        ) {
            Ok(resolution) => resolution,
            Err(CpError::PolicyAbort { reason }) => return Ok(RouteResolveStep::Abort(reason)),
            Err(_) => return Err(RecvError::PhaseInvariant),
        };

        let arm = match resolution {
            DynamicResolution::RouteArm { arm } => arm,
            DynamicResolution::Loop { decision } => {
                if decision {
                    0
                } else {
                    1
                }
            }
            DynamicResolution::Defer { retry_hint } => {
                return Ok(RouteResolveStep::Deferred {
                    retry_hint,
                    source: DeferSource::Resolver,
                });
            }
            _ => return Err(RecvError::PhaseInvariant),
        };
        let arm = Arm::new(arm).ok_or(RecvError::PhaseInvariant)?;
        self.record_route_decision_for_lane(local_meta.lane as usize, scope_id, arm.as_u8());
        self.record_scope_ack(scope_id, RouteDecisionToken::from_resolver(arm));
        self.emit_route_decision(
            scope_id,
            arm.as_u8(),
            RouteDecisionSource::Resolver,
            local_meta.lane,
        );
        Ok(RouteResolveStep::Resolved(arm))
    }

    fn map_cp_error(err: CpError) -> SendError {
        match err {
            CpError::PolicyAbort { reason } => SendError::PolicyAbort { reason },
            _ => SendError::PhaseInvariant,
        }
    }
    pub(crate) async fn send_with_meta<M>(
        mut self,
        meta: &SendMeta,
        payload: Option<&<M as MessageSpec>::Payload>,
    ) -> SendResult<(
        Self,
        ControlOutcome<'r, <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>,
    )>
    where
        M: MessageSpec + SendableLabel,
        M::Payload: WireEncode,
        M::ControlKind: CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
    {
        let control_handling = <M::ControlKind as ControlPayloadKind>::HANDLING;
        let expects_control = !matches!(control_handling, ControlHandling::None);
        if meta.is_control != expects_control {
            return Err(SendError::PhaseInvariant);
        }

        let mut control_outcome = ControlOutcome::<
            'r,
            <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
        >::None;
        let mut canonical_generic_token: Option<
            GenericCapToken<<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>,
        > = None;

        let policy_action = self.eval_endpoint_policy(
            Slot::EndpointTx,
            ids::ENDPOINT_SEND,
            self.sid.raw(),
            Self::endpoint_policy_args(
                Lane::new(meta.lane as u32),
                meta.label,
                FrameFlags::empty(),
            ),
            Lane::new(meta.lane as u32),
        );
        self.apply_send_policy(policy_action, meta.scope, Lane::new(meta.lane as u32))?;

        let cluster_ref = self.control.cluster();
        let rv_id = self.rendezvous_id();
        let sid_raw = self.sid.raw();
        let lane_wire = self.port_for_lane(meta.lane as usize).lane().as_wire();
        let scope_trace = self.scope_trace(meta.scope);
        let logical_meta =
            TapFrameMeta::new(sid_raw, lane_wire, ROLE, meta.label, FrameFlags::empty());

        // Auto-mint tokens for both Canonical and External control handling.
        // Canonical: token is used internally, not transmitted over wire.
        // External: token is minted AND transmitted over wire (cross-role).
        let minted_token = if matches!(
            control_handling,
            ControlHandling::Canonical | ControlHandling::External
        ) {
            let token_result = <M::ControlKind as CanonicalTokenProvider<
                'r,
                ROLE,
                T,
                U,
                C,
                E,
                Mint,
                MAX_RV,
                M,
                B,
            >>::into_token(&self, meta);
            token_result?
        } else {
            None
        };

        let mut dispatch_frame = None;
        {
            // Use lane-specific port for multi-lane parallel execution
            let port = self.port_for_lane_mut(meta.lane as usize);
            let payload_view = unsafe {
                let scratch = &mut *port.scratch_ptr();
                let len = match control_handling {
                    ControlHandling::None => {
                        let data = payload.ok_or(SendError::PhaseInvariant)?;
                        data.encode_into(scratch).map_err(SendError::Codec)?
                    }
                    ControlHandling::Canonical => {
                        let token = minted_token.ok_or(SendError::PhaseInvariant)?;
                        let frame = token.into_frame();
                        let bytes = *frame.bytes();
                        scratch[..CAP_TOKEN_LEN].copy_from_slice(&bytes);
                        canonical_generic_token = Some(frame.as_generic());
                        dispatch_frame = Some(frame);
                        CAP_TOKEN_LEN
                    }
                    ControlHandling::External => {
                        // External control: behavior depends on AUTO_MINT_EXTERNAL.
                        // - If auto-minted (e.g., splice): use minted token
                        // - Otherwise (e.g., management): use caller-provided payload
                        if let Some(token) = minted_token {
                            let frame = token.into_frame();
                            let bytes = *frame.bytes();
                            scratch[..CAP_TOKEN_LEN].copy_from_slice(&bytes);
                            control_outcome = ControlOutcome::External(frame.as_generic());
                            dispatch_frame = Some(frame);
                            CAP_TOKEN_LEN
                        } else {
                            let data = payload.ok_or(SendError::PhaseInvariant)?;
                            data.encode_into(scratch).map_err(SendError::Codec)?
                        }
                    }
                };
                let slice = &scratch[..len];
                Payload::new(slice)
            };

            let transport = port.transport();
            let tx_ptr = port.tx_ptr();

            let outgoing = crate::transport::Outgoing {
                meta: crate::transport::SendMeta {
                    eff_index: meta.eff_index,
                    label: meta.label,
                    peer: meta.peer,
                    lane: meta.lane,
                    direction: if meta.peer == ROLE {
                        crate::transport::LocalDirection::Local
                    } else {
                        crate::transport::LocalDirection::Send
                    },
                    is_control: meta.is_control,
                },
                payload: payload_view,
            };

            if !outgoing.meta.is_local() {
                unsafe {
                    transport
                        .send(&mut *tx_ptr, outgoing)
                        .await
                        .map_err(|err| SendError::Transport(err.into()))?;
                }
            }
        }

        // Advance typestate cursor (delegates to RoleTypestate).
        // Use try_advance_past_jumps to follow any Jump nodes (explicit control flow).
        self.set_cursor(
            self.cursor
                .try_advance_past_jumps()
                .map_err(|_| SendError::PhaseInvariant)?,
        );

        let lane_idx = meta.lane as usize;
        self.advance_lane_cursor(lane_idx, meta.eff_index);
        self.maybe_skip_remaining_route_arm(meta.scope, meta.lane, meta.route_arm, meta.eff_index);
        self.settle_scope_after_action(meta.scope, meta.route_arm, Some(meta.eff_index), meta.lane);
        self.maybe_advance_phase();

        let event_id = if meta.is_control {
            ids::ENDPOINT_CONTROL
        } else {
            ids::ENDPOINT_SEND
        };
        self.emit_endpoint_event(event_id, logical_meta, scope_trace, meta.lane);

        if let Some(frame) = dispatch_frame {
            let cluster = cluster_ref.ok_or(SendError::PhaseInvariant)?;
            match cluster.dispatch_typed_control_frame(rv_id, frame, None) {
                Ok(result) => {
                    if matches!(control_handling, ControlHandling::Canonical) {
                        let registered = result.ok_or(SendError::PhaseInvariant)?;
                        control_outcome = ControlOutcome::Canonical(registered);
                    }
                }
                Err(err) => match err {
                    CpError::Authorisation {
                        effect: CpEffect::SpliceAck,
                    } => {
                        if let Some(token) = canonical_generic_token.take() {
                            control_outcome = ControlOutcome::Canonical(
                                CapRegisteredToken::from_bytes(token.into_bytes()),
                            );
                        }
                    }
                    _ => return Err(SendError::PhaseInvariant),
                },
            }
        } else if matches!(control_handling, ControlHandling::Canonical) {
            return Err(SendError::PhaseInvariant);
        }

        Ok((self, control_outcome))
    }

    /// Receive a payload of type `M` according to the current typestate step.
    pub async fn recv<M>(mut self) -> RecvResult<(Self, <M as MessageSpec>::Payload)>
    where
        M: MessageSpec,
        M::Payload: WireDecodeOwned,
    {
        let target_label = <M as MessageSpec>::LABEL;
        self.try_select_lane_for_label(target_label);

        // Navigate to the correct recv position, handling route scopes via resolver
        let mut _iter_count = 0u32;
        loop {
            _iter_count += 1;
            // Defensive check: recv() should converge within 3 iterations.
            // If we exceed this, it indicates a bug in control flow logic.
            debug_assert!(
                _iter_count <= 3,
                "recv() infinite loop detected at iter={}",
                _iter_count
            );
            if _iter_count > 3 {
                return Err(RecvError::PhaseInvariant);
            }

            // Handle loop decision Jump nodes (passive observer at loop boundary).
            // When we land on a Jump(LoopContinue), we need to consult the resolver
            // to decide which arm to take for the next iteration.
            // Note: LoopBreak is NOT handled here - it's followed automatically by
            // advance_past_jumps() since Break arm is already selected.
            if let Some(reason) = self.cursor.jump_reason() {
                if matches!(reason, JumpReason::LoopContinue) {
                    if let Some(region) = self.cursor.scope_region() {
                        if region.kind == ScopeKind::Route && region.linger {
                            let scope_id = region.scope_id;
                            // Consult resolver for loop decision
                            let route_signals = self.policy_signals_for_slot(Slot::Route);
                            if let Ok(step) =
                                self.prepare_route_decision_from_resolver(scope_id, route_signals)
                            {
                                match step {
                                    RouteResolveStep::Resolved(arm) => {
                                        // Navigate based on resolver decision using O(1) registry lookup
                                        if arm.as_u8() == 0 {
                                            // Continue: follow LoopContinue jump to loop start
                                            self.set_cursor(self.cursor.advance());
                                        } else {
                                            // Break: use PassiveObserverBranch registry for O(1) lookup
                                            if let Some(nav) =
                                                self.cursor.follow_passive_observer_arm(arm.as_u8())
                                            {
                                                let PassiveArmNavigation::WithinArm { entry } = nav;
                                                self.set_cursor(
                                                    self.cursor
                                                        .with_index(state_index_to_usize(entry)),
                                                );
                                            }
                                        }
                                        continue;
                                    }
                                    RouteResolveStep::Abort(reason) => {
                                        return Err(RecvError::PolicyAbort { reason });
                                    }
                                    RouteResolveStep::Deferred { .. } => {}
                                }
                            }
                        }
                    }
                }
            }

            // Check if we're at a route scope boundary where we need resolver to select arm
            // This must be checked BEFORE is_recv() because we might be at recv position
            // but in the wrong arm of the route
            if let Some(region) = self.cursor.scope_region() {
                if region.kind == ScopeKind::Route && self.cursor.index() == region.start {
                    let scope_id = region.scope_id;
                    let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
                    // Check if we already have arm info for this scope
                    let existing_arm = self.route_arm_for(lane_wire, scope_id);
                    if let Some(arm) = existing_arm {
                        // Navigate to the recv position for this arm.
                        // Controller roles use route_recv_indices O(1) lookup.
                        // Passive observers use PassiveObserverBranch O(1) registry.
                        let recv_idx = self.cursor.route_scope_arm_recv_index(scope_id, arm);
                        if let Some(idx) = recv_idx {
                            self.set_cursor(self.cursor.with_index(idx));
                            self.set_route_arm(lane_wire, scope_id, arm)?;
                            continue;
                        }
                        // Passive observer: use PassiveObserverBranch registry for O(1) lookup
                        if let Some(nav) = self.cursor.follow_passive_observer_arm(arm) {
                            let PassiveArmNavigation::WithinArm { entry } = nav;
                            self.set_cursor(self.cursor.with_index(state_index_to_usize(entry)));
                            self.set_route_arm(lane_wire, scope_id, arm)?;
                            continue;
                        }
                        // If arm has no recv (e.g., Break arm of loop), advance past route scope
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

            // If not at recv and not at route start, try to advance past route scope
            if let Some(region) = self.cursor.scope_region() {
                if region.kind == ScopeKind::Route
                    && self.can_advance_route_scope(region.scope_id, target_label)
                {
                    if let Some(cursor) = self.cursor.advance_scope_if_kind(ScopeKind::Route) {
                        self.set_cursor(cursor);
                        continue;
                    }
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

        // Try the binding-backed recv path first. This reads framed payloads from
        // the binding via `on_recv()`. Raw transport frames continue on the direct
        // transport recv path below.
        //
        let mut binding_buf: [u8; 65536] = [0; 65536];
        let logical_lane = meta.lane;

        let binding_data =
            self.try_recv_from_binding(logical_lane, meta.label, &mut binding_buf)?;

        let payload_bytes: &[u8] = if let Some(n) = binding_data {
            &binding_buf[..n]
        } else {
            'recv_loop: loop {
                let payload = {
                    let port = self.port_for_lane_mut(meta.lane as usize);
                    let transport = port.transport();
                    let rx_ptr = port.rx_ptr();
                    unsafe {
                        transport
                            .recv(&mut *rx_ptr)
                            .await
                            .map_err(|err| RecvError::Transport(err.into()))?
                    }
                };

                if let Some(n) =
                    self.try_recv_from_binding(logical_lane, meta.label, &mut binding_buf)?
                {
                    break 'recv_loop &binding_buf[..n];
                }

                if payload.as_bytes().is_empty() {
                    let binding_active = self.binding.policy_signals_provider().is_some();
                    if !binding_active {
                        break 'recv_loop payload.as_bytes();
                    }
                    if M::Payload::decode_owned(&[]).is_ok() {
                        break 'recv_loop payload.as_bytes();
                    }
                    // Empty payload likely signals stream data queued for binding path.
                    continue;
                }

                break 'recv_loop payload.as_bytes();
            }
        };

        let policy_action = self.eval_endpoint_policy(
            Slot::EndpointRx,
            ids::ENDPOINT_RECV,
            sid_raw,
            Self::endpoint_policy_args(
                Lane::new(meta.lane as u32),
                meta.label,
                FrameFlags::empty(),
            ),
            Lane::new(meta.lane as u32),
        );
        self.apply_recv_policy(policy_action, meta.scope, Lane::new(meta.lane as u32))?;

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

        // Advance typestate cursor (delegates to RoleTypestate).
        // Use try_advance_past_jumps to follow any Jump nodes (explicit control flow).
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

    fn record_loop_decision(
        &self,
        metadata: &LoopMetadata<ROLE>,
        decision: LoopDecision,
        lane: u8,
    ) -> SendResult<()> {
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
            self.port_for_lane(lane as usize)
                .record_route_decision(metadata.scope, arm);
            self.emit_route_decision(metadata.scope, arm, RouteDecisionSource::Ack, lane);
        }
        Ok(())
    }

    fn select_scope(&mut self) -> RecvResult<OfferScopeSelection> {
        self.align_cursor_to_selected_scope()?;
        // O(1) entry: offer() must be called at a Route decision point.
        // Use the node's scope directly (no parent traversal).
        let node_scope = self.current_offer_scope_id();
        let region = match self.cursor.scope_region_by_id(node_scope) {
            Some(region) => region,
            None => return Err(RecvError::PhaseInvariant),
        };
        if region.kind != ScopeKind::Route {
            return Err(RecvError::PhaseInvariant);
        }
        let scope_id = region.scope_id;
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id)
            && !offer_entry.is_max()
            && self.cursor.index() != state_index_to_usize(offer_entry)
        {
            let selected_arm = self.selected_arm_for_scope(scope_id);
            let current_arm = self.cursor.typestate_node(self.cursor.index()).route_arm();
            if selected_arm.is_none() || current_arm != selected_arm {
                return Err(RecvError::PhaseInvariant);
            }
        }
        let current_idx = self.cursor.index();
        let cached_entry_state = self
            .offer_entry_state
            .get(current_idx)
            .copied()
            .filter(|state| state.active_mask != 0 && state.scope_id == scope_id);
        // Route hints are offer-scoped; hints are consumed per-offer via take_scope_hint().
        let (offer_lanes, offer_lane_mask, offer_lanes_len, label_meta, materialization_meta) =
            if let Some(entry_state) = cached_entry_state {
                (
                    entry_state.offer_lanes,
                    entry_state.offer_lane_mask,
                    entry_state.offer_lanes_len as usize,
                    entry_state.label_meta,
                    entry_state.materialization_meta,
                )
            } else {
                let (offer_lanes, offer_lanes_len) = self.offer_lanes_for_scope(scope_id);
                if offer_lanes_len == 0 {
                    return Err(RecvError::PhaseInvariant);
                }
                let mut offer_lane_mask = 0u8;
                let mut offer_lane_idx = 0usize;
                while offer_lane_idx < offer_lanes_len {
                    let lane = offer_lanes[offer_lane_idx] as usize;
                    if lane < MAX_LANES {
                        offer_lane_mask |= 1u8 << lane;
                    }
                    offer_lane_idx += 1;
                }
                let offer_lane = offer_lanes[0];
                let offer_lane_idx = offer_lane as usize;
                (
                    offer_lanes,
                    offer_lane_mask,
                    offer_lanes_len,
                    self.offer_scope_label_meta(scope_id, offer_lane_idx),
                    self.offer_scope_materialization_meta(scope_id, offer_lane_idx),
                )
            };
        if offer_lanes_len == 0 {
            return Err(RecvError::PhaseInvariant);
        }
        let offer_lane = offer_lanes[0];
        let offer_lane_idx = offer_lane as usize;
        let passive_recv_meta =
            self.compute_scope_passive_recv_meta(materialization_meta, scope_id, offer_lane);
        let at_route_offer_entry = self
            .cursor
            .route_scope_offer_entry(scope_id)
            .map(|entry| entry.is_max() || current_idx == state_index_to_usize(entry))
            .unwrap_or(true);
        Ok(OfferScopeSelection {
            scope_id,
            frontier_parallel_root: Self::parallel_scope_root(&self.cursor, scope_id),
            offer_lanes,
            offer_lane_mask,
            offer_lanes_len,
            offer_lane,
            offer_lane_idx,
            label_meta,
            materialization_meta,
            passive_recv_meta,
            at_route_offer_entry,
        })
    }

    /// Observe an inbound route branch.
    ///
    /// Route hints are drained once per call and consumed only when they match
    /// the current route scope.
    /// Loop control evidence that resolves a recv-less branch is treated as
    /// EmptyArmTerminal and skip decode.
    pub async fn offer(self) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        let mut self_endpoint = self;
        let mut frontier_visited = FrontierVisitSet::EMPTY;
        let mut carried_binding_classification = None;
        let mut carried_transport_payload = None;
        if let Some(cluster) = self_endpoint.control.cluster() {
            let rv_id = self_endpoint.control.rendezvous_id();
            // Policy epoch switches are applied only at this boundary to keep
            // a single offer->route->decode cycle on one lease epoch.
            cluster
                .on_decision_boundary(rv_id)
                .map_err(|_| RecvError::PhaseInvariant)?;
        }
        'offer_frontier: loop {
            // Stage 1: select a single route scope and its poll lane.
            let selection = self_endpoint.select_scope()?;
            let scope_id = selection.scope_id;
            frontier_visited.record(scope_id);
            let offer_lane_mask = selection.offer_lane_mask;
            let offer_lane = selection.offer_lane;
            let offer_lane_idx = selection.offer_lane_idx;
            let label_meta = selection.label_meta;
            let at_route_offer_entry = selection.at_route_offer_entry;

            // Self-send controller routes have no recv nodes in this scope.
            let cursor_is_not_recv = !self_endpoint.cursor.is_recv();
            let is_route_controller = self_endpoint.cursor.is_route_controller(scope_id);
            let controller_selected_recv_step = is_route_controller
                && !at_route_offer_entry
                && self_endpoint
                    .cursor
                    .try_recv_meta()
                    .map(|recv_meta| recv_meta.peer != ROLE)
                    .unwrap_or(false);
            let loop_meta = label_meta.loop_meta();

            let route_policy_is_dynamic = self_endpoint
                .cursor
                .route_scope_controller_policy(scope_id)
                .map(|(policy, _, _)| policy.is_dynamic())
                .unwrap_or(false);
            // Route classification is binary: merged or dynamic.
            // Non-merged scopes require dynamic policy metadata regardless of role.
            let is_dynamic_route_scope = route_policy_is_dynamic;
            // Dynamic route scopes resolve arm via scope ack + EPF(Route)/resolver only.
            // Binding classification and hint promotion remain demux-only and must not
            // influence dynamic arm selection.
            let suppress_scope_hint = is_dynamic_route_scope;
            self_endpoint.ingest_scope_evidence_for_offer(
                scope_id,
                offer_lane_idx,
                selection.offer_lane_mask,
                suppress_scope_hint,
                label_meta,
            );
            let preview_route_decision = self_endpoint.preview_scope_ack_token_non_consuming(
                scope_id,
                offer_lane_idx,
                selection.offer_lane_mask,
            );
            let preview_ready_arm_evidence = self_endpoint.scope_has_ready_arm_evidence(scope_id);
            let recvless_loop_control_scope = !is_route_controller
                && !is_dynamic_route_scope
                && loop_meta.control_scope()
                && !loop_meta.arm_has_recv(0)
                && !loop_meta.arm_has_recv(1);

            let is_self_send_controller = cursor_is_not_recv
                && is_route_controller
                && !Self::scope_has_controller_arm_entry(&self_endpoint.cursor, scope_id);
            let controller_non_entry_cursor_ready = cursor_is_not_recv
                && is_route_controller
                && self_endpoint.controller_arm_at_cursor(scope_id).is_none();

            let early_route_decision = if is_route_controller {
                preview_route_decision
            } else {
                preview_route_decision
                    .filter(|token| !self_endpoint.arm_has_recv(scope_id, token.arm().as_u8()))
            };

            // Skip recv loop when the selected arm has no recv node for this role.
            let early_decision_arm_has_no_recv = early_route_decision
                .map(|token| !self_endpoint.arm_has_recv(scope_id, token.arm().as_u8()))
                .unwrap_or(false);
            let early_hint_resolves_recvless = false;

            // Skip recv loop when the decision is available without wire data.
            let controller_static_entry_ready = false;
            let controller_pending_materialization = is_route_controller
                && self_endpoint
                    .selected_arm_for_scope(scope_id)
                    .map(|arm| {
                        self_endpoint.arm_requires_materialization_ready_evidence(scope_id, arm)
                            && !self_endpoint.scope_has_ready_arm(scope_id, arm)
                    })
                    .unwrap_or(false);
            let controller_can_skip_recv = is_route_controller
                && !controller_pending_materialization
                && ((at_route_offer_entry
                    && (is_dynamic_route_scope
                        || controller_non_entry_cursor_ready
                        || is_self_send_controller
                        || early_route_decision.is_some()
                        || controller_static_entry_ready))
                    || (!at_route_offer_entry && cursor_is_not_recv));
            let passive_dynamic_scope_has_recv =
                self_endpoint.arm_has_recv(scope_id, 0) || self_endpoint.arm_has_recv(scope_id, 1);
            let passive_ack_is_materializable = self_endpoint
                .preview_scope_ack_token_non_consuming(scope_id, offer_lane_idx, offer_lane_mask)
                .map(|token| {
                    let arm = token.arm().as_u8();
                    self_endpoint.scope_has_ready_arm(scope_id, arm)
                        || !self_endpoint.arm_has_recv(scope_id, arm)
                })
                .unwrap_or(false);
            let passive_dynamic_can_skip_recv = !is_route_controller
                && is_dynamic_route_scope
                && (!passive_dynamic_scope_has_recv
                    || preview_ready_arm_evidence
                    || passive_ack_is_materializable);
            let skip_recv_loop = passive_dynamic_can_skip_recv
                || controller_can_skip_recv
                || early_decision_arm_has_no_recv
                || early_hint_resolves_recvless;
            let mut binding_classification = carried_binding_classification.take();
            let (mut transport_payload_len, mut transport_payload_lane) =
                carried_transport_payload.take().unwrap_or((0, offer_lane));
            if binding_classification.is_none() && transport_payload_len == 0 {
                let payload_view = if skip_recv_loop {
                    crate::transport::wire::Payload::new(&[])
                } else {
                    'offer_recv: loop {
                        if !is_route_controller || controller_selected_recv_step {
                            if let Some((_, classification)) = self_endpoint.poll_binding_for_offer(
                                scope_id,
                                offer_lane_idx,
                                offer_lane_mask,
                                label_meta,
                                selection.materialization_meta,
                            ) {
                                binding_classification = Some(classification);
                                break 'offer_recv crate::transport::wire::Payload::new(&[]);
                            }
                            if recvless_loop_control_scope
                                && let Some((_, classification)) = self_endpoint
                                    .poll_binding_any_for_offer(offer_lane_idx, offer_lane_mask)
                            {
                                binding_classification = Some(classification);
                                break 'offer_recv crate::transport::wire::Payload::new(&[]);
                            }
                        }

                        let payload = {
                            let port = self_endpoint.port_for_lane(offer_lane_idx);
                            let transport = port.transport();
                            let rx_ptr = port.rx_ptr();
                            unsafe {
                                transport
                                    .recv(&mut *rx_ptr)
                                    .await
                                    .map_err(|err| RecvError::Transport(err.into()))?
                            }
                        };

                        if !is_route_controller || controller_selected_recv_step {
                            if let Some((_, classification)) = self_endpoint.poll_binding_for_offer(
                                scope_id,
                                offer_lane_idx,
                                offer_lane_mask,
                                label_meta,
                                selection.materialization_meta,
                            ) {
                                binding_classification = Some(classification);
                                break 'offer_recv crate::transport::wire::Payload::new(&[]);
                            }
                            if recvless_loop_control_scope
                                && let Some((_, classification)) = self_endpoint
                                    .poll_binding_any_for_offer(offer_lane_idx, offer_lane_mask)
                            {
                                binding_classification = Some(classification);
                                break 'offer_recv crate::transport::wire::Payload::new(&[]);
                            }
                        }

                        break 'offer_recv payload;
                    }
                };
                if !payload_view.as_bytes().is_empty() {
                    let payload_bytes = payload_view.as_bytes();
                    let port = self_endpoint.port_for_lane_mut(offer_lane_idx);
                    let scratch_ptr = port.scratch_ptr();
                    let scratch = unsafe { &mut *scratch_ptr };
                    transport_payload_len = stage_transport_payload(scratch, payload_bytes)?;
                    transport_payload_lane = offer_lane;
                }
            }
            if let Some(classification) = binding_classification.as_ref() {
                self_endpoint.ingest_binding_scope_evidence(
                    scope_id,
                    classification.label,
                    suppress_scope_hint,
                    label_meta,
                );
            }
            self_endpoint.ingest_scope_evidence_for_offer(
                scope_id,
                offer_lane_idx,
                selection.offer_lane_mask,
                suppress_scope_hint,
                label_meta,
            );
            if self_endpoint.scope_evidence_conflicted(scope_id)
                && !self_endpoint.recover_scope_evidence_conflict(
                    scope_id,
                    is_dynamic_route_scope,
                    is_route_controller,
                )
            {
                return Err(RecvError::PhaseInvariant);
            }

            // Stage 2: resolve arm authority. The order is fixed:
            // Ack -> Resolver -> Poll.
            let resolved = match self_endpoint
                .resolve_token(
                    selection,
                    is_route_controller,
                    is_dynamic_route_scope,
                    &mut binding_classification,
                    &mut transport_payload_len,
                    &mut transport_payload_lane,
                    &mut frontier_visited,
                )
                .await?
            {
                ResolveTokenOutcome::RestartFrontier => {
                    carried_binding_classification = binding_classification;
                    carried_transport_payload = (transport_payload_len != 0)
                        .then_some((transport_payload_len, transport_payload_lane));
                    continue 'offer_frontier;
                }
                ResolveTokenOutcome::Resolved(resolved) => resolved,
            };
            if !is_route_controller
                && self_endpoint.descend_selected_passive_route(selection, resolved)?
            {
                carried_binding_classification = binding_classification;
                carried_transport_payload = (transport_payload_len != 0)
                    .then_some((transport_payload_len, transport_payload_lane));
                continue 'offer_frontier;
            }
            return self_endpoint.materialize_branch(
                selection,
                resolved,
                is_route_controller,
                binding_classification,
                transport_payload_len,
                transport_payload_lane,
            );
        }
    }

    // Stage 3 of the offer kernel: materialize the selected branch from
    // precomputed route metadata and late binding demux state. This stage must
    // not perform arm arbitration.
    fn materialize_branch(
        mut self,
        selection: OfferScopeSelection,
        resolved: ResolvedRouteDecision,
        is_route_controller: bool,
        mut binding_classification: Option<crate::binding::IncomingClassification>,
        mut transport_payload_len: usize,
        transport_payload_lane: u8,
    ) -> RecvResult<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> {
        let scope_id = selection.scope_id;
        let offer_lanes = selection.offer_lanes;
        let offer_lanes_len = selection.offer_lanes_len;
        let offer_lane = selection.offer_lane;
        let route_token = resolved.route_token;
        let selected_arm = resolved.selected_arm;
        let resolved_label_hint = resolved.resolved_label_hint;
        let binding_channel: Option<crate::binding::Channel> = None;
        if !is_route_controller {
            self.propagate_recvless_parent_route_decision(scope_id, selected_arm);
        }
        let broadcast_controller_route_decision =
            is_route_controller && matches!(route_token.source(), RouteDecisionSource::Ack);
        if broadcast_controller_route_decision {
            let decision_source = route_token.source();
            let mut lane_idx = 0usize;
            while lane_idx < offer_lanes_len {
                let lane = offer_lanes[lane_idx];
                self.record_route_decision_for_lane(lane as usize, scope_id, selected_arm);
                self.emit_route_decision(scope_id, selected_arm, decision_source, lane);
                lane_idx += 1;
            }
        } else if matches!(route_token.source(), RouteDecisionSource::Poll) {
            self.emit_route_decision(
                scope_id,
                selected_arm,
                RouteDecisionSource::Poll,
                offer_lane,
            );
        }

        let meta =
            self.materialize_selected_arm_meta(selection, selected_arm, resolved_label_hint)?;

        self.skip_unselected_arm_lanes(scope_id, selected_arm, meta.lane);

        let policy_action = self.eval_endpoint_policy(
            Slot::EndpointRx,
            ids::ENDPOINT_RECV,
            self.sid.raw(),
            Self::endpoint_policy_args(
                Lane::new(meta.lane as u32),
                meta.label,
                FrameFlags::empty(),
            ),
            Lane::new(meta.lane as u32),
        );
        self.apply_recv_policy(policy_action, meta.scope, Lane::new(meta.lane as u32))?;

        let lane_wire = meta.lane;
        self.set_route_arm(lane_wire, scope_id, selected_arm)?;

        // Determine BranchKind before late binding resolution so wire-bound
        // branches can decide whether to wait for one additional ingress turn.
        let passive_linger_loop_label = !is_route_controller
            && self.is_linger_route(scope_id)
            && matches!(meta.label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK);
        let branch_kind = if self.cursor.is_recv() {
            if passive_linger_loop_label
                || (!is_route_controller
                    && matches!(meta.label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK)
                    && self.selection_non_wire_loop_control_recv(
                        selection,
                        is_route_controller,
                        selected_arm,
                        meta.label,
                    ))
            {
                BranchKind::LocalControl
            } else {
                BranchKind::WireRecv
            }
        } else if self.cursor.is_send() {
            BranchKind::ArmSendHint
        } else if self.cursor.is_local_action() || self.cursor.is_jump() {
            BranchKind::LocalControl
        } else {
            BranchKind::EmptyArmTerminal
        };

        // Late binding channel resolution: for wire recv branches, prefer
        // binding ingress even when transport payload bytes were staged earlier.
        let label_meta = selection.label_meta;
        let binding_channel = if transport_payload_len == 0
            || matches!(branch_kind, BranchKind::WireRecv)
        {
            let mut channel = binding_channel;
            let lane_idx = meta.lane as usize;
            if let Some(expected_label) = label_meta.preferred_binding_label(Some(selected_arm)) {
                if binding_classification
                    .as_ref()
                    .map(|classification| classification.label == expected_label)
                    .unwrap_or(false)
                {
                    if let Some(classification) = binding_classification.take() {
                        channel = Some(classification.channel);
                    }
                } else if binding_classification.as_ref().and_then(|classification| {
                    Self::scope_label_to_arm(label_meta, classification.label)
                }) == Some(selected_arm)
                {
                    if let Some(classification) = binding_classification.take() {
                        channel = Some(classification.channel);
                    }
                } else if let Some(classification) =
                    self.take_matching_binding_for_lane(lane_idx, expected_label)
                {
                    channel = Some(classification.channel);
                }
            } else {
                (channel, _, _) = self.take_binding_for_selected_arm(
                    lane_idx,
                    selected_arm,
                    label_meta,
                    &mut binding_classification,
                );
            }
            channel
        } else {
            binding_channel
        };
        if transport_payload_len != 0
            && (!matches!(branch_kind, BranchKind::WireRecv) || binding_channel.is_some())
        {
            let port = self.port_for_lane(transport_payload_lane as usize);
            let transport = port.transport();
            let rx_ptr = port.rx_ptr();
            unsafe {
                transport.requeue(&mut *rx_ptr);
            }
            transport_payload_len = 0;
        }
        if self.selection_arm_has_recv(selection, selected_arm) {
            // Ready-arm evidence is one-shot per successful materialization.
            // Do not consume it before defer paths complete.
            self.consume_scope_ready_arm(scope_id, selected_arm);
        }
        let branch_progress_eff = self
            .cursor
            .scope_lane_last_eff_for_arm(scope_id, selected_arm, lane_wire)
            .or_else(|| self.cursor.scope_lane_last_eff(scope_id, lane_wire))
            .unwrap_or(meta.eff_index);
        let branch_meta = BranchMeta {
            scope_id,
            selected_arm,
            lane_wire,
            eff_index: branch_progress_eff,
            kind: branch_kind,
        };

        // Scope evidence is one-shot per offer.
        // Once a branch is selected, do not carry hint/ack into the next offer.
        self.clear_scope_evidence(scope_id);
        if lane_wire == 5 {
            // Route hints on this lane are one-shot and must not leak into
            // subsequent offers after branch materialization.
            let port = self.port_for_lane(lane_wire as usize);
            port.clear_route_hints();
        }

        Ok(RouteBranch {
            label: meta.label,
            transport_payload_len,
            transport_payload_lane,
            endpoint: self,
            binding_channel,
            branch_meta,
        })
    }

    // Stage 2 of the offer kernel: resolve arm authority in fixed order
    // Ack -> Resolver -> Poll. This stage may defer/yield/restart frontier
    // evaluation, but it must not materialize the selected branch.
    async fn resolve_token(
        &mut self,
        selection: OfferScopeSelection,
        is_route_controller: bool,
        is_dynamic_route_scope: bool,
        binding_classification: &mut Option<crate::binding::IncomingClassification>,
        transport_payload_len: &mut usize,
        transport_payload_lane: &mut u8,
        frontier_visited: &mut FrontierVisitSet,
    ) -> RecvResult<ResolveTokenOutcome> {
        let scope_id = selection.scope_id;
        let frontier_parallel_root = selection.frontier_parallel_root;
        let offer_lanes = selection.offer_lanes;
        let offer_lane_mask = selection.offer_lane_mask;
        let offer_lanes_len = selection.offer_lanes_len;
        let offer_lane = selection.offer_lane;
        let offer_lane_idx = selection.offer_lane_idx;
        let label_meta = selection.label_meta;
        let at_route_offer_entry = selection.at_route_offer_entry;

        let mut resolved_label_hint = self
            .take_scope_hint(scope_id)
            .and_then(ScopeHint::new)
            .map(ScopeHint::label);
        if *transport_payload_len != 0
            && let Some(label) = resolved_label_hint
        {
            self.mark_scope_ready_arm_from_label(scope_id, label, label_meta);
        }

        let mut liveness = OfferLivenessState::new(self.liveness_policy);
        let mut liveness_exhausted = false;

        let mut route_token = if is_route_controller {
            self.take_scope_ack(scope_id)
        } else {
            self.peek_scope_ack(scope_id)
        };
        if route_token.is_none() && is_route_controller && is_dynamic_route_scope {
            let is_self_send_route = !Self::scope_has_controller_arm_entry(&self.cursor, scope_id);
            loop {
                let route_signals = self.policy_signals_for_slot(Slot::Route);
                let resolver_step = if is_self_send_route {
                    self.prepare_route_decision_from_resolver_via_arm_entry(
                        scope_id,
                        route_signals,
                    )?
                } else {
                    self.prepare_route_decision_from_resolver(scope_id, route_signals)?
                };
                match resolver_step {
                    RouteResolveStep::Resolved(resolver_arm) => {
                        route_token = Some(RouteDecisionToken::from_resolver(resolver_arm));
                        break;
                    }
                    RouteResolveStep::Abort(reason) => {
                        return Err(RecvError::PolicyAbort { reason });
                    }
                    RouteResolveStep::Deferred { retry_hint, source } => {
                        match self.on_frontier_defer(
                            &mut liveness,
                            scope_id,
                            frontier_parallel_root,
                            source,
                            DeferReason::Unsupported,
                            retry_hint,
                            offer_lane,
                            binding_classification.is_some(),
                            None,
                            frontier_visited,
                        ) {
                            FrontierDeferOutcome::Continue => {}
                            FrontierDeferOutcome::Yielded => {
                                return Ok(ResolveTokenOutcome::RestartFrontier);
                            }
                            FrontierDeferOutcome::Exhausted => {
                                liveness_exhausted = true;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if route_token.is_none() && !is_route_controller {
            let mut passive_waited_for_wire = false;
            loop {
                let staged_payload_for_offer_lane =
                    *transport_payload_len != 0 && *transport_payload_lane == offer_lane;
                if !staged_payload_for_offer_lane {
                    self.cache_binding_classification_for_offer(
                        scope_id,
                        offer_lane_idx,
                        offer_lane_mask,
                        label_meta,
                        selection.materialization_meta,
                        binding_classification,
                    );

                    self.ingest_scope_evidence_for_offer(
                        scope_id,
                        offer_lane_idx,
                        offer_lane_mask,
                        is_dynamic_route_scope,
                        label_meta,
                    );
                    if let Some(classification) = binding_classification.as_ref() {
                        self.ingest_binding_scope_evidence(
                            scope_id,
                            classification.label,
                            is_dynamic_route_scope,
                            label_meta,
                        );
                    }
                    if self.scope_evidence_conflicted(scope_id)
                        && !self.recover_scope_evidence_conflict(
                            scope_id,
                            is_dynamic_route_scope,
                            is_route_controller,
                        )
                    {
                        return Err(RecvError::PhaseInvariant);
                    }

                    if let Some(label) = self
                        .take_scope_hint(scope_id)
                        .and_then(ScopeHint::new)
                        .map(ScopeHint::label)
                    {
                        resolved_label_hint = Some(label);
                    }
                }
                if let Some(token) = self.peek_scope_ack(scope_id) {
                    route_token = Some(token);
                    break;
                }

                if *transport_payload_len != 0 {
                    break;
                }

                if resolved_label_hint.is_some() && passive_waited_for_wire {
                    break;
                }

                if self.scope_has_ready_arm_evidence(scope_id) {
                    let needs_wire_turn_for_materialization = !passive_waited_for_wire
                        && *transport_payload_len == 0
                        && binding_classification.is_none();
                    if !needs_wire_turn_for_materialization {
                        break;
                    }
                }

                if !passive_waited_for_wire {
                    let recv_lane_idx = offer_lane as usize;
                    let recv_lane = recv_lane_idx as u8;
                    let port = self.port_for_lane(recv_lane_idx);
                    let transport = port.transport();
                    let rx_ptr = port.rx_ptr();
                    let mut recv_fut = core::pin::pin!(unsafe { transport.recv(&mut *rx_ptr) });
                    let payload = poll_fn(|cx| match recv_fut.as_mut().poll(cx) {
                        Poll::Ready(result) => Poll::Ready(Some(result)),
                        Poll::Pending => Poll::Ready(None),
                    })
                    .await;
                    if let Some(payload) = payload {
                        let payload = payload.map_err(|err| RecvError::Transport(err.into()))?;
                        if *transport_payload_len == 0 && !payload.as_bytes().is_empty() {
                            let payload_bytes = payload.as_bytes();
                            let port = self.port_for_lane_mut(recv_lane_idx);
                            let scratch_ptr = port.scratch_ptr();
                            let scratch = unsafe { &mut *scratch_ptr };
                            *transport_payload_len =
                                stage_transport_payload(scratch, payload_bytes)?;
                            *transport_payload_lane = recv_lane;
                        }
                    }
                    passive_waited_for_wire = true;
                    continue;
                }

                match self.on_frontier_defer(
                    &mut liveness,
                    scope_id,
                    frontier_parallel_root,
                    DeferSource::Resolver,
                    DeferReason::NoEvidence,
                    1,
                    offer_lane,
                    binding_classification.is_some(),
                    None,
                    frontier_visited,
                ) {
                    FrontierDeferOutcome::Continue => {
                        break;
                    }
                    FrontierDeferOutcome::Yielded => {
                        return Ok(ResolveTokenOutcome::RestartFrontier);
                    }
                    FrontierDeferOutcome::Exhausted => {
                        liveness_exhausted = true;
                        break;
                    }
                }
            }
        }

        if route_token.is_none()
            && !is_route_controller
            && is_dynamic_route_scope
            && self.binding.policy_signals_provider().is_some()
        {
            let route_signals = self.policy_signals_for_slot(Slot::Route);
            match self.prepare_route_decision_from_resolver(scope_id, route_signals)? {
                RouteResolveStep::Resolved(resolver_arm) => {
                    route_token = Some(RouteDecisionToken::from_resolver(resolver_arm));
                }
                RouteResolveStep::Abort(reason) => {
                    if reason != 0 {
                        return Err(RecvError::PolicyAbort { reason });
                    }
                }
                RouteResolveStep::Deferred { retry_hint, source } => {
                    match self.on_frontier_defer(
                        &mut liveness,
                        scope_id,
                        frontier_parallel_root,
                        source,
                        DeferReason::Unsupported,
                        retry_hint,
                        offer_lane,
                        binding_classification.is_some(),
                        None,
                        frontier_visited,
                    ) {
                        FrontierDeferOutcome::Continue => {}
                        FrontierDeferOutcome::Yielded => {
                            yield_once().await;
                            return Ok(ResolveTokenOutcome::RestartFrontier);
                        }
                        FrontierDeferOutcome::Exhausted => {
                            liveness_exhausted = true;
                        }
                    }
                }
            }
        }

        if route_token.is_none()
            && !is_route_controller
            && *transport_payload_len == 0
            && binding_classification.is_none()
            && resolved_label_hint.is_none()
            && !liveness_exhausted
        {
            match self.on_frontier_defer(
                &mut liveness,
                scope_id,
                frontier_parallel_root,
                DeferSource::Resolver,
                DeferReason::NoEvidence,
                1,
                offer_lane,
                false,
                None,
                frontier_visited,
            ) {
                FrontierDeferOutcome::Continue => {
                    yield_once().await;
                    return Ok(ResolveTokenOutcome::RestartFrontier);
                }
                FrontierDeferOutcome::Yielded => {
                    yield_once().await;
                    return Ok(ResolveTokenOutcome::RestartFrontier);
                }
                FrontierDeferOutcome::Exhausted => {
                    liveness_exhausted = true;
                }
            }
        }

        if route_token.is_none() && liveness_exhausted {
            while route_token.is_none() && liveness.can_force_poll() {
                liveness.mark_forced_poll();
                if let Some(poll_arm) = self
                    .try_poll_route_decision_for_offer(scope_id, &offer_lanes, offer_lanes_len)
                    .await
                {
                    route_token = Some(RouteDecisionToken::from_poll(poll_arm));
                    break;
                }
            }
            if route_token.is_none() {
                return Err(RecvError::PolicyAbort {
                    reason: liveness.exhaust_reason(),
                });
            }
        }

        if route_token.is_none() {
            if !is_route_controller
                && *transport_payload_len != 0
                && *transport_payload_lane != offer_lane
            {
                return Ok(ResolveTokenOutcome::RestartFrontier);
            }
            if let Some(poll_arm) = self
                .try_poll_route_decision_for_offer(scope_id, &offer_lanes, offer_lanes_len)
                .await
            {
                route_token = Some(RouteDecisionToken::from_poll(poll_arm));
            } else {
                match self.on_frontier_defer(
                    &mut liveness,
                    scope_id,
                    frontier_parallel_root,
                    DeferSource::Resolver,
                    DeferReason::NoEvidence,
                    1,
                    offer_lane,
                    binding_classification.is_some(),
                    None,
                    frontier_visited,
                ) {
                    FrontierDeferOutcome::Continue => {
                        yield_once().await;
                        return Ok(ResolveTokenOutcome::RestartFrontier);
                    }
                    FrontierDeferOutcome::Yielded => {
                        yield_once().await;
                        return Ok(ResolveTokenOutcome::RestartFrontier);
                    }
                    FrontierDeferOutcome::Exhausted => {
                        while route_token.is_none() && liveness.can_force_poll() {
                            liveness.mark_forced_poll();
                            if let Some(poll_arm) = self
                                .try_poll_route_decision_for_offer(
                                    scope_id,
                                    &offer_lanes,
                                    offer_lanes_len,
                                )
                                .await
                            {
                                route_token = Some(RouteDecisionToken::from_poll(poll_arm));
                                break;
                            }
                        }
                        if route_token.is_none() {
                            return Err(RecvError::PolicyAbort {
                                reason: liveness.exhaust_reason(),
                            });
                        }
                    }
                }
            }
        }

        let mut route_token = route_token.ok_or(RecvError::PhaseInvariant)?;
        if let Some(classification) = binding_classification.as_ref()
            && let Some(binding_arm) = Self::scope_label_to_arm(label_meta, classification.label)
            && binding_arm == route_token.arm().as_u8()
        {
            // Binding classification is demux-only. Once Ack/Resolver/Poll has
            // fixed the arm, matching classification may still contribute
            // readiness for branch materialization.
            self.mark_scope_ready_arm(scope_id, binding_arm);
        }
        if *transport_payload_len != 0 && *transport_payload_lane == offer_lane {
            if !is_route_controller
                && is_dynamic_route_scope
                && matches!(route_token.source(), RouteDecisionSource::Ack)
            {
                self.mark_scope_ready_arm(scope_id, route_token.arm().as_u8());
            } else if is_route_controller
                && is_dynamic_route_scope
                && matches!(
                    route_token.source(),
                    RouteDecisionSource::Resolver | RouteDecisionSource::Poll
                )
            {
                self.mark_scope_ready_arm(scope_id, route_token.arm().as_u8());
            }
        }

        let selected_arm = loop {
            let selected_arm = route_token.arm().as_u8();
            if self.selection_arm_requires_materialization_ready_evidence(
                selection,
                is_route_controller,
                selected_arm,
            ) && !self.scope_has_ready_arm(scope_id, selected_arm)
            {
                if matches!(route_token.source(), RouteDecisionSource::Resolver)
                    && let Some(poll_arm) = self
                        .try_poll_route_decision_for_offer(scope_id, &offer_lanes, offer_lanes_len)
                        .await
                {
                    route_token = RouteDecisionToken::from_poll(poll_arm);
                    continue;
                }
                if *transport_payload_len != 0 {
                    let port = self.port_for_lane(*transport_payload_lane as usize);
                    let transport = port.transport();
                    let rx_ptr = port.rx_ptr();
                    unsafe {
                        transport.requeue(&mut *rx_ptr);
                    }
                }
                if matches!(route_token.source(), RouteDecisionSource::Resolver) {
                    let _ = self.take_scope_ack(scope_id);
                }
                let keep_current_scope = is_route_controller
                    && is_dynamic_route_scope
                    && !at_route_offer_entry
                    && matches!(route_token.source(), RouteDecisionSource::Resolver);
                if keep_current_scope {
                    yield_once().await;
                    return Ok(ResolveTokenOutcome::RestartFrontier);
                }
                match self.on_frontier_defer(
                    &mut liveness,
                    scope_id,
                    frontier_parallel_root,
                    DeferSource::Resolver,
                    DeferReason::NoEvidence,
                    1,
                    offer_lane,
                    binding_classification.is_some(),
                    Some(route_token.arm().as_u8()),
                    frontier_visited,
                ) {
                    FrontierDeferOutcome::Continue => {
                        if !is_route_controller && !is_dynamic_route_scope {
                            self.await_static_passive_progress(
                                selection,
                                Some(route_token.arm().as_u8()),
                                binding_classification,
                                transport_payload_len,
                                transport_payload_lane,
                            )
                            .await?;
                            return Ok(ResolveTokenOutcome::RestartFrontier);
                        }
                        yield_once().await;
                        return Ok(ResolveTokenOutcome::RestartFrontier);
                    }
                    FrontierDeferOutcome::Yielded => {
                        yield_once().await;
                        return Ok(ResolveTokenOutcome::RestartFrontier);
                    }
                    FrontierDeferOutcome::Exhausted => {
                        while liveness.can_force_poll() {
                            liveness.mark_forced_poll();
                            if self
                                .try_poll_route_decision_for_offer(
                                    scope_id,
                                    &offer_lanes,
                                    offer_lanes_len,
                                )
                                .await
                                .is_some()
                            {
                                return Ok(ResolveTokenOutcome::RestartFrontier);
                            }
                        }
                        return Err(RecvError::PolicyAbort {
                            reason: liveness.exhaust_reason(),
                        });
                    }
                }
            }
            break selected_arm;
        };
        Ok(ResolveTokenOutcome::Resolved(ResolvedRouteDecision {
            route_token,
            selected_arm,
            resolved_label_hint,
        }))
    }

    pub(crate) fn canonical_control_token<K>(&self, meta: &SendMeta) -> SendResult<CapFlowToken<K>>
    where
        K: ResourceKind + ControlMint,
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsCanonical,
    {
        let tag = meta.resource.ok_or(SendError::PhaseInvariant)?;
        let shot = meta.shot.ok_or(SendError::PhaseInvariant)?;
        let cp_sid = SessionId::new(self.sid.raw());
        let port = self.port_for_lane(meta.lane as usize);
        let lane = port.lane();
        let cp_lane = Lane::new(lane.raw());
        let src_rv = RendezvousId::new(self.rendezvous_id().raw());
        port.flush_transport_events();
        let transport_metrics = port.transport().metrics().snapshot();
        let signals = self.policy_signals_for_slot(Slot::Route);
        let attrs = signals.attrs;
        let bytes = match tag {
            LoopContinueKind::TAG => {
                if K::TAG != LoopContinueKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                // Record loop decision before minting token
                let mut loop_scope = meta.scope;
                let mut recorded_via_loop_metadata = false;
                if let Some(metadata) = self.cursor.loop_metadata_inner()
                    && metadata.role == LoopRole::Controller
                    && metadata.controller == ROLE
                {
                    self.record_loop_decision(&metadata, LoopDecision::Continue, meta.lane)?;
                    loop_scope = metadata.scope;
                    recorded_via_loop_metadata = true;
                }
                if loop_scope.is_none() {
                    return Err(SendError::PhaseInvariant);
                }
                if !recorded_via_loop_metadata && loop_scope.kind() == ScopeKind::Route {
                    self.port_for_lane(meta.lane as usize)
                        .record_route_decision(loop_scope, 0);
                    self.emit_route_decision(loop_scope, 0, RouteDecisionSource::Ack, meta.lane);
                }
                let scope = loop_scope;
                let handle = LoopDecisionHandle {
                    sid: self.sid.raw(),
                    lane: lane.raw() as u16,
                    scope,
                };
                self.mint_control_token_with_handle::<LoopContinueKind>(
                    meta.peer, shot, lane, handle,
                )?
                .into_bytes()
            }
            LoopBreakKind::TAG => {
                if K::TAG != LoopBreakKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                // Record loop decision before minting token
                let mut loop_scope = meta.scope;
                let mut recorded_via_loop_metadata = false;
                if let Some(metadata) = self.cursor.loop_metadata_inner()
                    && metadata.role == LoopRole::Controller
                    && metadata.controller == ROLE
                {
                    self.record_loop_decision(&metadata, LoopDecision::Break, meta.lane)?;
                    loop_scope = metadata.scope;
                    recorded_via_loop_metadata = true;
                }
                if loop_scope.is_none() {
                    return Err(SendError::PhaseInvariant);
                }
                if !recorded_via_loop_metadata && loop_scope.kind() == ScopeKind::Route {
                    self.port_for_lane(meta.lane as usize)
                        .record_route_decision(loop_scope, 1);
                    self.emit_route_decision(loop_scope, 1, RouteDecisionSource::Ack, meta.lane);
                }
                let scope = loop_scope;
                let handle = LoopDecisionHandle {
                    sid: self.sid.raw(),
                    lane: lane.raw() as u16,
                    scope,
                };
                self.mint_control_token_with_handle::<LoopBreakKind>(meta.peer, shot, lane, handle)?
                    .into_bytes()
            }
            RerouteKind::TAG => {
                if K::TAG != RerouteKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let policy = cluster
                    .policy_mode_for(src_rv, cp_lane, meta.eff_index, tag)
                    .map_err(|_| SendError::PhaseInvariant)?;
                let handle = cluster
                    .prepare_reroute_handle_from_policy(
                        src_rv,
                        cp_lane,
                        meta.eff_index,
                        tag,
                        policy,
                        transport_metrics,
                        signals.input,
                        attrs,
                    )
                    .map_err(|_| SendError::PhaseInvariant)?;
                self.mint_control_token_with_handle::<RerouteKind>(meta.peer, shot, lane, handle)?
                    .into_bytes()
            }
            RouteDecisionKind::TAG => {
                if K::TAG != RouteDecisionKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let policy = cluster
                    .policy_mode_for(src_rv, cp_lane, meta.eff_index, tag)
                    .map_err(|_| SendError::PhaseInvariant)?;
                let scope = meta.scope;
                let policy_scope = policy.scope();
                validate_route_decision_scope(scope, policy_scope)?;
                // Route arm is fixed by the offer/decode decision point.
                // Canonical route token minting must not re-evaluate policy.
                let arm = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
                if arm > 1 {
                    return Err(SendError::PhaseInvariant);
                }
                let handle = RouteDecisionHandle { scope, arm };
                self.port_for_lane(meta.lane as usize)
                    .record_route_decision(scope, arm);
                self.emit_route_decision(scope, arm, RouteDecisionSource::Resolver, meta.lane);
                self.mint_control_token_with_handle::<RouteDecisionKind>(
                    meta.peer, shot, lane, handle,
                )?
                .into_bytes()
            }
            SpliceIntentKind::TAG => {
                if K::TAG != SpliceIntentKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let policy = cluster
                    .policy_mode_for(src_rv, cp_lane, meta.eff_index, tag)
                    .map_err(|_| SendError::PhaseInvariant)?;
                let operands = cluster
                    .prepare_splice_operands_from_policy(
                        src_rv,
                        cp_sid,
                        cp_lane,
                        meta.eff_index,
                        tag,
                        policy,
                        transport_metrics,
                        signals.input,
                        attrs,
                    )
                    .map_err(|_| SendError::PhaseInvariant)?;
                self.mint_control_token_with_handle::<SpliceIntentKind>(
                    meta.peer,
                    shot,
                    lane,
                    Self::splice_handle_from_operands(operands),
                )?
                .into_bytes()
            }
            SpliceAckKind::TAG => {
                if K::TAG != SpliceAckKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
                let operands = cluster
                    .take_cached_splice_operands(cp_sid)
                    .or_else(|| cluster.distributed_operands(cp_sid))
                    .ok_or(SendError::PhaseInvariant)?;
                let token = self.mint_control_token_with_handle::<SpliceAckKind>(
                    meta.peer,
                    shot,
                    lane,
                    Self::splice_handle_from_operands(operands),
                )?;
                token.into_bytes()
            }
            CommitKind::TAG => {
                if K::TAG != CommitKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                self.mint_control_token::<CommitKind>(meta.peer, shot, lane)?
                    .into_bytes()
            }
            CheckpointKind::TAG => {
                if K::TAG != CheckpointKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                self.mint_control_token::<CheckpointKind>(meta.peer, shot, lane)?
                    .into_bytes()
            }
            RollbackKind::TAG => {
                if K::TAG != RollbackKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                self.mint_control_token::<RollbackKind>(meta.peer, shot, lane)?
                    .into_bytes()
            }
            CancelKind::TAG => {
                if K::TAG != CancelKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                self.mint_control_token::<CancelKind>(meta.peer, shot, lane)?
                    .into_bytes()
            }
            CancelAckKind::TAG => {
                if K::TAG != CancelAckKind::TAG {
                    return Err(SendError::PhaseInvariant);
                }
                self.mint_control_token::<CancelAckKind>(meta.peer, shot, lane)?
                    .into_bytes()
            }
            // Generic path for external control kinds (e.g., adapter AcceptHookKind).
            // Uses ControlMint trait for extensibility without modifying hibana core.
            _ => {
                let handle = K::mint_handle(self.sid, lane, meta.scope);
                self.mint_control_token_with_handle::<K>(meta.peer, shot, lane, handle)?
                    .into_bytes()
            }
        };
        Ok(CapFlowToken::new(GenericCapToken::<K>::from_bytes(bytes)))
    }

    #[inline]
    pub(crate) fn settle_scope_after_action(
        &mut self,
        scope: ScopeId,
        route_arm: Option<u8>,
        _eff_index: Option<EffIndex>,
        lane: u8,
    ) {
        let region = if scope.kind() == ScopeKind::Route {
            self.cursor.scope_region_by_id(scope)
        } else {
            None
        };
        let linger = region.as_ref().map_or(false, |r| r.linger);
        let lane_wire = lane;
        let mut exited_scope = false;

        // For linger scopes (loops), if cursor has advanced past the region boundary,
        // rewind to region.start so the next offer() can find the recv node.
        // This is essential for passive observers whose projection has fewer steps.
        // BUT: do NOT rewind if we're in the Break arm (arm > 0 for standard 2-arm loops).
        // The Break arm should exit the loop, not loop back.
        if linger {
            if let Some(ref reg) = region {
                let current_arm = route_arm.or_else(|| self.route_arm_for(lane_wire, scope));
                let is_break_arm = current_arm.map_or(false, |arm| arm > 0);
                if self.cursor.index() >= reg.end {
                    self.clear_descendant_route_state_for_lane(lane_wire, scope);
                    if is_break_arm {
                        self.pop_route_arm(lane_wire, scope);
                        exited_scope = true;
                        let mut current_scope = scope;
                        while let Some(parent) = self.cursor.scope_parent(current_scope) {
                            if !matches!(parent.kind(), ScopeKind::Route | ScopeKind::Loop) {
                                break;
                            }

                            if let Some(parent_region) = self.cursor.scope_region_by_id(parent) {
                                if parent_region.linger {
                                    if let Some(parent_arm) = self.route_arm_for(lane_wire, parent)
                                    {
                                        if parent_arm == 0 {
                                            self.set_cursor(
                                                self.cursor.with_index(parent_region.start),
                                            );
                                            break;
                                        }
                                    }
                                }
                                let should_advance = self.cursor.index() >= parent_region.end;

                                if should_advance {
                                    self.clear_descendant_route_state_for_lane(lane_wire, parent);
                                    if let Some(cursor) = self.cursor.advance_scope_by_id(parent) {
                                        self.set_cursor(cursor);
                                    }
                                    self.pop_route_arm(lane_wire, parent);
                                    current_scope = parent;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    } else {
                        self.set_cursor(self.cursor.with_index(reg.start));
                    }
                }
                if !is_break_arm {
                    let at_scope_start = self.cursor.index() == reg.start;
                    let at_passive_branch = self.cursor.jump_reason()
                        == Some(JumpReason::PassiveObserverBranch)
                        && self
                            .cursor
                            .scope_region()
                            .map(|region| region.scope_id == scope)
                            .unwrap_or(false);
                    if at_scope_start || at_passive_branch {
                        if let Some(first_eff) = self.cursor.scope_lane_first_eff(scope, lane_wire)
                        {
                            let lane_idx = lane_wire as usize;
                            self.set_lane_cursor_to_eff_index(lane_idx, first_eff);
                        }
                    }
                }
            }
        } else if let Some(ref reg) = region {
            if self.cursor.index() >= reg.end {
                exited_scope = true;
            }
        }

        if exited_scope {
            if let Some(eff_index) = self.cursor.scope_lane_last_eff(scope, lane_wire) {
                let lane_idx = lane_wire as usize;
                self.advance_lane_cursor(lane_idx, eff_index);
            }
        }

        if scope.kind() == ScopeKind::Route {
            if exited_scope {
                self.pop_route_arm(lane_wire, scope);
            } else if let Some(arm) = route_arm {
                let _ = self.set_route_arm(lane_wire, scope, arm);
            }
            if exited_scope {
                self.clear_scope_evidence(scope);
            }
        }

        // If we rewound into a parent linger scope, sync its lane cursor to the
        // entry eff_index so offer()/flow() can locate the next iteration.
        let mut parent_scope = scope;
        loop {
            let Some(parent) = self.cursor.scope_parent(parent_scope) else {
                break;
            };
            if !matches!(parent.kind(), ScopeKind::Route | ScopeKind::Loop) {
                break;
            }
            if let Some(parent_region) = self.cursor.scope_region_by_id(parent) {
                if parent.kind() == ScopeKind::Route
                    && !parent_region.linger
                    && self.cursor.index() >= parent_region.end
                {
                    self.pop_route_arm(lane_wire, parent);
                    self.clear_scope_evidence(parent);
                }
                if parent_region.linger && self.cursor.index() == parent_region.start {
                    if let Some(parent_arm) = self.route_arm_for(lane_wire, parent) {
                        if parent_arm == 0 {
                            if let Some(first_eff) =
                                self.cursor.scope_lane_first_eff(parent, lane_wire)
                            {
                                let lane_idx = lane_wire as usize;
                                self.set_lane_cursor_to_eff_index(lane_idx, first_eff);
                            }
                        }
                    }
                }
            }
            parent_scope = parent;
        }
        self.prune_route_state_to_cursor_path_for_lane(lane_wire);
    }

    /// Rendezvous id for the primary port.
    #[inline]
    pub(crate) fn rendezvous_id(&self) -> RendezvousId {
        self.port().rv_id()
    }

    /// Get the primary lane's port (typically Lane 0).
    ///
    /// # Safety invariant
    /// The primary port is always retained by construction. This is enforced
    /// at attach time and preserved throughout the endpoint's lifetime.
    fn port(&self) -> &Port<'r, T, E> {
        debug_assert!(
            self.ports[self.primary_lane].is_some(),
            "port: primary lane {} has no port (invariant violation)",
            self.primary_lane
        );
        // SAFETY: Primary port is always present by construction invariant.
        // In release builds, unwrap_unchecked could be used, but we keep
        // expect for defense-in-depth.
        self.ports[self.primary_lane]
            .as_ref()
            .expect("cursor endpoint retains primary port")
    }

    /// Get the primary lane's port mutably.
    ///
    /// # Safety invariant
    /// The primary port is always retained by construction.
    fn port_mut(&mut self) -> &mut Port<'r, T, E> {
        debug_assert!(
            self.ports[self.primary_lane].is_some(),
            "port_mut: primary lane {} has no port (invariant violation)",
            self.primary_lane
        );
        self.ports[self.primary_lane]
            .as_mut()
            .expect("cursor endpoint retains primary port")
    }

    /// Get port for a specific lane.
    ///
    /// # Panics
    /// Panics if the port for `lane_idx` was not acquired.
    fn port_for_lane(&self, lane_idx: usize) -> &Port<'r, T, E> {
        debug_assert!(
            self.ports[lane_idx].is_some(),
            "port_for_lane: lane {} has no port",
            lane_idx
        );
        self.ports[lane_idx]
            .as_ref()
            .expect("port not acquired for lane")
    }

    /// Get port for a specific lane mutably.
    ///
    /// # Panics
    /// Panics if the port for `lane_idx` was not acquired.
    fn port_for_lane_mut(&mut self, lane_idx: usize) -> &mut Port<'r, T, E> {
        debug_assert!(
            self.ports[lane_idx].is_some(),
            "port not acquired for lane {}",
            lane_idx
        );
        self.ports[lane_idx]
            .as_mut()
            .expect("port not acquired for lane")
    }

    fn loop_index(scope: ScopeId) -> Option<u8> {
        u8::try_from(scope.ordinal()).ok()
    }

    #[inline]
    fn offer_lanes_for_scope(&self, scope_id: ScopeId) -> ([u8; MAX_LANES], usize) {
        self.cursor
            .route_scope_offer_lane_list(scope_id)
            .unwrap_or(([0; MAX_LANES], 0))
    }

    #[inline]
    fn offer_lane_for_scope(&self, scope_id: ScopeId) -> u8 {
        let (lanes, len) = self.offer_lanes_for_scope(scope_id);
        if len == 0 {
            self.primary_lane as u8
        } else {
            lanes[0]
        }
    }

    #[inline]
    fn scope_slot_for_route(&self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() || scope_id.kind() != ScopeKind::Route {
            return None;
        }
        self.cursor.route_scope_slot(scope_id)
    }

    #[inline]
    fn scope_evidence_generation_for_scope(&self, scope_id: ScopeId) -> u32 {
        self.scope_slot_for_route(scope_id)
            .and_then(|slot| self.scope_evidence_generations.get(slot).copied())
            .unwrap_or(0)
    }

    #[inline]
    fn record_scope_ack(&mut self, scope_id: ScopeId, token: RouteDecisionToken) {
        let arm = token.arm().as_u8();
        if let Some(slot) = self.scope_slot_for_route(scope_id) {
            let changed = {
                let evidence = &mut self.scope_evidence[slot];
                if (evidence.flags & ScopeEvidence::FLAG_ACK_CONFLICT) != 0 {
                    return;
                }
                if let Some(existing) = evidence.ack
                    && existing.arm().as_u8() != arm
                {
                    evidence.flags |= ScopeEvidence::FLAG_ACK_CONFLICT;
                    evidence.ack = None;
                    evidence.ready_arm_mask = 0;
                    evidence.poll_ready_arm_mask = 0;
                    true
                } else if evidence.ack != Some(token) {
                    evidence.ack = Some(token);
                    true
                } else {
                    false
                }
            };
            if changed {
                self.bump_scope_evidence_generation_for_scope(scope_id, slot);
            }
        }
    }

    #[inline]
    fn peek_scope_ack(&self, scope_id: ScopeId) -> Option<RouteDecisionToken> {
        let slot = self.scope_slot_for_route(scope_id)?;
        let evidence = self.scope_evidence[slot];
        if (evidence.flags & ScopeEvidence::FLAG_ACK_CONFLICT) != 0 {
            return None;
        }
        evidence.ack
    }

    #[inline]
    fn take_scope_ack(&mut self, scope_id: ScopeId) -> Option<RouteDecisionToken> {
        let slot = self.scope_slot_for_route(scope_id)?;
        let token = {
            let evidence = &mut self.scope_evidence[slot];
            if (evidence.flags & ScopeEvidence::FLAG_ACK_CONFLICT) != 0 {
                return None;
            }
            let token = evidence.ack;
            evidence.ack = None;
            token
        };
        if token.is_some() {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
        token
    }

    #[inline]
    fn record_scope_hint(&mut self, scope_id: ScopeId, label: u8) {
        if label == 0 {
            return;
        }
        if let Some(slot) = self.scope_slot_for_route(scope_id) {
            let changed = {
                let (flags, existing_label) = {
                    let evidence = self.scope_evidence[slot];
                    (evidence.flags, evidence.hint_label)
                };
                if (flags & ScopeEvidence::FLAG_HINT_CONFLICT) != 0 {
                    return;
                }
                if existing_label == ScopeEvidence::NONE {
                    self.scope_evidence[slot].hint_label = label;
                    true
                } else if existing_label == label {
                    false
                } else {
                    let evidence = &mut self.scope_evidence[slot];
                    evidence.flags |= ScopeEvidence::FLAG_HINT_CONFLICT;
                    evidence.hint_label = ScopeEvidence::NONE;
                    true
                }
            };
            if changed {
                self.bump_scope_evidence_generation_for_scope(scope_id, slot);
            }
        }
    }

    #[inline]
    fn record_scope_hint_dynamic(&mut self, scope_id: ScopeId, label: u8) {
        if label == 0 {
            return;
        }
        if let Some(slot) = self.scope_slot_for_route(scope_id) {
            let changed = {
                let evidence = &mut self.scope_evidence[slot];
                let old_label = evidence.hint_label;
                let old_flags = evidence.flags;
                evidence.hint_label = label;
                evidence.flags &= !ScopeEvidence::FLAG_HINT_CONFLICT;
                evidence.hint_label != old_label || evidence.flags != old_flags
            };
            if changed {
                self.bump_scope_evidence_generation_for_scope(scope_id, slot);
            }
        }
    }

    #[inline]
    fn mark_scope_ready_arm_inner(&mut self, scope_id: ScopeId, arm: u8, poll_ready: bool) {
        if let Some(slot) = self.scope_slot_for_route(scope_id) {
            let bit = ScopeEvidence::arm_bit(arm);
            let changed = {
                let evidence = &mut self.scope_evidence[slot];
                let ready_changed = (evidence.ready_arm_mask & bit) == 0;
                let poll_changed = poll_ready && (evidence.poll_ready_arm_mask & bit) == 0;
                if ready_changed {
                    evidence.ready_arm_mask |= bit;
                }
                if poll_changed {
                    evidence.poll_ready_arm_mask |= bit;
                }
                ready_changed || poll_changed
            };
            if changed {
                self.bump_scope_evidence_generation_for_scope(scope_id, slot);
            }
        }
    }

    #[inline]
    fn mark_scope_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        self.mark_scope_ready_arm_inner(scope_id, arm, true);
    }

    #[inline]
    fn mark_scope_materialization_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        self.mark_scope_ready_arm_inner(scope_id, arm, false);
    }

    #[inline]
    fn mark_scope_ready_arm_from_label(
        &mut self,
        scope_id: ScopeId,
        label: u8,
        label_meta: ScopeLabelMeta,
    ) {
        let exact_static_passive_arm =
            self.static_passive_dispatch_arm_from_exact_label(scope_id, label, label_meta);
        let arm =
            Self::scope_evidence_label_to_arm(label_meta, label).or(exact_static_passive_arm);
        if let Some(arm) = arm {
            if matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK)
                && label_meta.loop_meta().arm_has_recv(arm)
            {
                // Loop-control labels can carry decision evidence without proving
                // recv readiness for recv-required arms.
                return;
            }
            if self.static_passive_scope_evidence_materializes_poll(scope_id) {
                self.mark_scope_ready_arm(scope_id, arm);
            } else {
                self.mark_scope_materialization_ready_arm(scope_id, arm);
            }
            if exact_static_passive_arm.is_some() {
                self.mark_static_passive_descendant_path_ready(scope_id, label);
            }
        }
    }

    #[inline]
    fn mark_scope_ready_arm_from_binding_label(
        &mut self,
        scope_id: ScopeId,
        label: u8,
        label_meta: ScopeLabelMeta,
    ) {
        let exact_static_passive_arm =
            self.static_passive_dispatch_arm_from_exact_label(scope_id, label, label_meta);
        let arm =
            Self::binding_scope_evidence_label_to_arm(label_meta, label).or(exact_static_passive_arm);
        if let Some(arm) = arm {
            if matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK)
                && label_meta.loop_meta().arm_has_recv(arm)
            {
                return;
            }
            if self.static_passive_scope_evidence_materializes_poll(scope_id) {
                self.mark_scope_ready_arm(scope_id, arm);
            } else {
                self.mark_scope_materialization_ready_arm(scope_id, arm);
            }
            if exact_static_passive_arm.is_some() {
                self.mark_static_passive_descendant_path_ready(scope_id, label);
            }
        }
    }

    #[inline]
    fn static_passive_scope_evidence_materializes_poll(&self, scope_id: ScopeId) -> bool {
        !self.cursor.is_route_controller(scope_id)
            && !self
                .cursor
                .route_scope_controller_policy(scope_id)
                .map(|(policy, _, _)| policy.is_dynamic())
                .unwrap_or(false)
    }

    #[inline]
    fn static_passive_dispatch_arm_from_exact_label(
        &self,
        scope_id: ScopeId,
        label: u8,
        label_meta: ScopeLabelMeta,
    ) -> Option<u8> {
        if !self.static_passive_scope_evidence_materializes_poll(scope_id) {
            return None;
        }
        let _ = label_meta;
        self.static_passive_descendant_dispatch_arm_from_exact_label(scope_id, label)
    }

    fn static_passive_descendant_dispatch_arm_from_exact_label(
        &self,
        scope_id: ScopeId,
        label: u8,
    ) -> Option<u8> {
        if let Some((dispatch_arm, _)) = self.cursor.first_recv_target(scope_id, label) {
            if dispatch_arm != ARM_SHARED {
                return Some(dispatch_arm);
            }
        }
        let mut matched_arm = None;
        for arm in [0u8, 1u8] {
            let Some(child_scope) = self.cursor.passive_arm_scope_by_arm(scope_id, arm) else {
                continue;
            };
            if self
                .static_passive_descendant_dispatch_arm_from_exact_label(child_scope, label)
                .is_some()
            {
                if matched_arm.is_some_and(|prev| prev != arm) {
                    return None;
                }
                matched_arm = Some(arm);
            }
        }
        matched_arm
    }

    fn mark_static_passive_descendant_path_ready(&mut self, scope_id: ScopeId, label: u8) {
        let Some(arm) = self.static_passive_descendant_dispatch_arm_from_exact_label(scope_id, label)
        else {
            return;
        };
        self.mark_scope_ready_arm(scope_id, arm);
        if self.selected_arm_for_scope(scope_id).is_none() {
            let lane = self.offer_lane_for_scope(scope_id);
            let _ = self.set_route_arm(lane, scope_id, arm);
        }
        let Some(child_scope) = self.cursor.passive_arm_scope_by_arm(scope_id, arm) else {
            return;
        };
        self.mark_static_passive_descendant_path_ready(child_scope, label);
    }

    #[inline]
    fn scope_ready_arm_mask(&self, scope_id: ScopeId) -> u8 {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return 0;
        };
        self.scope_evidence[slot].ready_arm_mask
    }

    #[inline]
    fn scope_poll_ready_arm_mask(&self, scope_id: ScopeId) -> u8 {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return 0;
        };
        self.scope_evidence[slot].poll_ready_arm_mask
    }

    #[inline]
    fn scope_has_ready_arm(&self, scope_id: ScopeId, arm: u8) -> bool {
        (self.scope_ready_arm_mask(scope_id) & ScopeEvidence::arm_bit(arm)) != 0
    }

    #[inline]
    fn scope_has_ready_arm_evidence(&self, scope_id: ScopeId) -> bool {
        self.scope_ready_arm_mask(scope_id) != 0
    }

    #[inline]
    fn consume_scope_ready_arm(&mut self, scope_id: ScopeId, arm: u8) {
        if let Some(slot) = self.scope_slot_for_route(scope_id) {
            let bit = ScopeEvidence::arm_bit(arm);
            let changed = {
                let evidence = &mut self.scope_evidence[slot];
                let ready_changed = (evidence.ready_arm_mask & bit) != 0;
                let poll_changed = (evidence.poll_ready_arm_mask & bit) != 0;
                if ready_changed {
                    evidence.ready_arm_mask &= !bit;
                }
                if poll_changed {
                    evidence.poll_ready_arm_mask &= !bit;
                }
                ready_changed || poll_changed
            };
            if changed {
                self.bump_scope_evidence_generation_for_scope(scope_id, slot);
            }
        }
    }

    #[inline]
    fn peek_scope_hint(&self, scope_id: ScopeId) -> Option<u8> {
        let slot = self.scope_slot_for_route(scope_id)?;
        let evidence = self.scope_evidence[slot];
        if (evidence.flags & ScopeEvidence::FLAG_HINT_CONFLICT) != 0 {
            return None;
        }
        let label = evidence.hint_label;
        if label == ScopeEvidence::NONE {
            None
        } else {
            Some(label)
        }
    }

    #[inline]
    fn take_scope_hint(&mut self, scope_id: ScopeId) -> Option<u8> {
        let slot = self.scope_slot_for_route(scope_id)?;
        let label = {
            let evidence = &mut self.scope_evidence[slot];
            if (evidence.flags & ScopeEvidence::FLAG_HINT_CONFLICT) != 0 {
                return None;
            }
            let label = evidence.hint_label;
            evidence.hint_label = ScopeEvidence::NONE;
            label
        };
        if label != ScopeEvidence::NONE {
            self.bump_scope_evidence_generation_for_scope(scope_id, slot);
        }
        if label == ScopeEvidence::NONE {
            None
        } else {
            Some(label)
        }
    }

    #[inline]
    fn clear_scope_evidence(&mut self, scope_id: ScopeId) {
        if let Some(slot) = self.scope_slot_for_route(scope_id) {
            let changed = {
                let evidence = self.scope_evidence[slot];
                evidence.ack.is_some()
                    || evidence.hint_label != ScopeEvidence::NONE
                    || evidence.ready_arm_mask != 0
                    || evidence.poll_ready_arm_mask != 0
                    || evidence.flags != 0
            };
            if changed {
                self.scope_evidence[slot] = ScopeEvidence::EMPTY;
                self.bump_scope_evidence_generation_for_scope(scope_id, slot);
            }
        }
    }

    #[inline]
    fn scope_evidence_conflicted(&self, scope_id: ScopeId) -> bool {
        let Some(slot) = self.scope_slot_for_route(scope_id) else {
            return false;
        };
        self.scope_evidence[slot].flags != 0
    }

    #[inline]
    fn recover_scope_evidence_conflict(
        &mut self,
        scope_id: ScopeId,
        is_dynamic_scope: bool,
        is_route_controller: bool,
    ) -> bool {
        if is_dynamic_scope {
            self.clear_scope_evidence(scope_id);
            return true;
        }
        if is_route_controller {
            return false;
        }
        self.clear_scope_evidence(scope_id);
        true
    }

    #[inline]
    fn can_advance_route_scope(&self, scope_id: ScopeId, target_label: u8) -> bool {
        let lane_wire = self.lane_for_label_or_offer(scope_id, target_label);
        self.route_arm_for(lane_wire, scope_id).is_some()
    }

    #[inline]
    fn lane_for_label_or_offer(&self, scope_id: ScopeId, target_label: u8) -> u8 {
        if let Some((lane_idx, _)) = self.cursor.find_step_for_label(target_label) {
            lane_idx as u8
        } else {
            self.offer_lane_for_scope(scope_id)
        }
    }

    /// Obtain slot-scoped policy signals from the binding.
    fn policy_signals_for_slot(&self, slot: Slot) -> crate::transport::context::PolicySignals {
        match self.binding.policy_signals_provider() {
            Some(provider) => provider.signals(slot),
            None => crate::transport::context::PolicySignals::ZERO,
        }
    }

    async fn await_transport_payload_for_offer_lane(
        &mut self,
        offer_lane: u8,
        transport_payload_len: &mut usize,
        transport_payload_lane: &mut u8,
    ) -> RecvResult<()> {
        let lane_idx = offer_lane as usize;
        let payload = {
            let port = self.port_for_lane(lane_idx);
            let transport = port.transport();
            let rx_ptr = port.rx_ptr();
            unsafe { transport.recv(&mut *rx_ptr).await }
                .map_err(|err| RecvError::Transport(err.into()))?
        };
        if *transport_payload_len == 0 && !payload.as_bytes().is_empty() {
            let payload_bytes = payload.as_bytes();
            let port = self.port_for_lane_mut(lane_idx);
            let scratch_ptr = port.scratch_ptr();
            let scratch = unsafe { &mut *scratch_ptr };
            *transport_payload_len = stage_transport_payload(scratch, payload_bytes)?;
            *transport_payload_lane = offer_lane;
        }
        Ok(())
    }

    async fn await_static_passive_progress(
        &mut self,
        selection: OfferScopeSelection,
        selected_arm: Option<u8>,
        binding_classification: &mut Option<crate::binding::IncomingClassification>,
        transport_payload_len: &mut usize,
        transport_payload_lane: &mut u8,
    ) -> RecvResult<()> {
        if let Some(arm) = selected_arm
            && selection.at_route_offer_entry
            && let Some(entry) = selection.materialization_meta.passive_arm_entry(arm)
        {
            let target_cursor = self.cursor.with_index(state_index_to_usize(entry));
            if !target_cursor.is_recv() {
                return Ok(());
            }
        }
        if binding_classification.is_none()
            && let Some((_, classification)) = self.poll_binding_for_offer(
                selection.scope_id,
                selection.offer_lane_idx,
                selection.offer_lane_mask,
                selection.label_meta,
                selection.materialization_meta,
            )
        {
            *binding_classification = Some(classification);
            return Ok(());
        }
        if *transport_payload_len == 0 {
            self.await_transport_payload_for_offer_lane(
                selection.offer_lane,
                transport_payload_len,
                transport_payload_lane,
            )
            .await?;
        }
        Ok(())
    }

    #[inline]
    fn evidence_fingerprint(&self, scope_id: ScopeId, binding_ready: bool) -> EvidenceFingerprint {
        EvidenceFingerprint::new(
            self.peek_scope_ack(scope_id).is_some(),
            self.scope_has_ready_arm_evidence(scope_id),
            binding_ready,
        )
    }

    async fn try_poll_route_decision_immediate(
        &self,
        scope_id: ScopeId,
        offer_lanes: &[u8; MAX_LANES],
        offer_lanes_len: usize,
    ) -> Option<Arm> {
        let arm = poll_fn(|cx| {
            let mut lane_idx = 0usize;
            while lane_idx < offer_lanes_len {
                let lane = offer_lanes[lane_idx];
                let port = self.port_for_lane(lane as usize);
                if let Poll::Ready(arm) = port.poll_route_decision(scope_id, ROLE, cx) {
                    return Poll::Ready(Some(arm));
                }
                lane_idx += 1;
            }
            Poll::Ready(None)
        })
        .await?;
        Arm::new(arm)
    }

    #[inline]
    fn poll_arm_from_ready_mask(&self, scope_id: ScopeId) -> Option<Arm> {
        let mask = self.scope_poll_ready_arm_mask(scope_id);
        if mask.count_ones() != 1 {
            return None;
        }
        Arm::new(mask.trailing_zeros() as u8)
    }

    async fn try_poll_route_decision_for_offer(
        &self,
        scope_id: ScopeId,
        offer_lanes: &[u8; MAX_LANES],
        offer_lanes_len: usize,
    ) -> Option<Arm> {
        self.try_poll_route_decision_immediate(scope_id, offer_lanes, offer_lanes_len)
            .await
            .or_else(|| self.poll_arm_from_ready_mask(scope_id))
    }

    fn try_select_lane_for_label(&mut self, target_label: u8) -> bool {
        if let Some(meta) = self.cursor.try_recv_meta() {
            if meta.label == target_label {
                return true;
            }
        }
        if let Some(meta) = self.cursor.try_send_meta() {
            if meta.label == target_label {
                return true;
            }
        }
        if let Some(meta) = self.cursor.try_local_meta() {
            if meta.label == target_label {
                return true;
            }
        }
        if let Some(region) = self.cursor.scope_region()
            && region.kind == ScopeKind::Route
            && self.cursor.is_route_controller(region.scope_id)
            && self
                .cursor
                .controller_arm_entry_for_label(region.scope_id, target_label)
                .is_some()
        {
            // Route-controller arm selection is scope-local. Do not let the
            // phase lane cursor jump to a later same-lane occurrence of the
            // same label inside a descendant route; prepare_flow() will
            // reposition to the current scope's arm entry explicitly.
            return true;
        }
        let Some((lane_idx, _)) = self.cursor.find_step_for_label(target_label) else {
            return false;
        };
        let Some(idx) = self.cursor.index_for_lane_step(lane_idx) else {
            return false;
        };
        if idx != self.cursor.index() {
            self.set_cursor(self.cursor.with_index(idx));
        }
        true
    }

    fn hint_matches_scope(label_meta: ScopeLabelMeta, label: u8, suppress_hint: bool) -> bool {
        if suppress_hint {
            return false;
        }
        label_meta.matches_hint_label(label)
    }

    #[inline]
    fn scope_has_controller_arm_entry(cursor: &PhaseCursor<ROLE>, scope_id: ScopeId) -> bool {
        cursor.controller_arm_entry_by_arm(scope_id, 0).is_some()
            || cursor.controller_arm_entry_by_arm(scope_id, 1).is_some()
    }

    #[inline]
    fn current_recv_is_scope_local(
        cursor: &PhaseCursor<ROLE>,
        scope_id: ScopeId,
        loop_meta: ScopeLoopMeta,
        label: u8,
        arm: u8,
    ) -> bool {
        cursor
            .first_recv_target(scope_id, label)
            .map(|(target_arm, _)| target_arm == arm)
            .unwrap_or(false)
            || (loop_meta.loop_label_scope()
                && matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK))
    }

    fn scope_label_to_arm(label_meta: ScopeLabelMeta, label: u8) -> Option<u8> {
        label_meta.arm_for_label(label)
    }

    fn scope_evidence_label_to_arm(label_meta: ScopeLabelMeta, label: u8) -> Option<u8> {
        label_meta.evidence_arm_for_label(label)
    }

    #[inline]
    fn binding_scope_evidence_label_to_arm(label_meta: ScopeLabelMeta, label: u8) -> Option<u8> {
        label_meta.binding_evidence_arm_for_label(label)
    }

    #[inline]
    fn scope_arm_has_recv(cursor: &PhaseCursor<ROLE>, scope_id: ScopeId, arm: u8) -> bool {
        if cursor.route_scope_arm_recv_index(scope_id, arm).is_some() {
            return true;
        }
        if let Some((entry, _label)) = cursor.controller_arm_entry_by_arm(scope_id, arm) {
            let target_cursor = cursor.with_index(state_index_to_usize(entry));
            if target_cursor.is_recv() {
                return true;
            }
        }
        if let Some(PassiveArmNavigation::WithinArm { entry }) =
            cursor.follow_passive_observer_arm_for_scope(scope_id, arm)
        {
            let target_cursor = cursor.with_index(state_index_to_usize(entry));
            return target_cursor.is_recv();
        }
        let mut dispatch_idx = 0usize;
        while let Some((_label, dispatch_arm, target)) =
            cursor.route_scope_first_recv_dispatch_entry(scope_id, dispatch_idx)
        {
            if (dispatch_arm == arm || dispatch_arm == ARM_SHARED)
                && cursor
                    .with_index(state_index_to_usize(target))
                    .try_recv_meta()
                    .is_some()
            {
                return true;
            }
            dispatch_idx += 1;
        }
        false
    }

    #[inline]
    fn arm_requires_materialization_ready_evidence(&self, scope_id: ScopeId, arm: u8) -> bool {
        let at_scope_offer_entry = self
            .cursor
            .route_scope_offer_entry(scope_id)
            .map(|entry| entry.is_max() || self.cursor.index() == state_index_to_usize(entry))
            .unwrap_or(true);
        if self.cursor.is_route_controller(scope_id) && at_scope_offer_entry {
            if let Some((entry, _label)) = self.cursor.controller_arm_entry_by_arm(scope_id, arm) {
                let target_cursor = self.cursor.with_index(state_index_to_usize(entry));
                if let Some(recv_meta) = target_cursor.try_recv_meta() {
                    return recv_meta.peer != ROLE;
                }
                return false;
            }
        }
        if let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(scope_id, arm)
        {
            let target_cursor = self.cursor.with_index(state_index_to_usize(entry));
            let Some(recv_meta) = target_cursor.try_recv_meta() else {
                return false;
            };
            if recv_meta.peer == ROLE {
                return false;
            }
            if recv_meta.is_control {
                if let Some((_controller_entry, controller_label)) =
                    self.cursor.controller_arm_entry_by_arm(scope_id, arm)
                    && recv_meta.label == controller_label
                {
                    return false;
                }
                if !self.cursor.is_route_controller(scope_id)
                    && matches!(recv_meta.label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK)
                {
                    return false;
                }
            }
            return true;
        }
        self.cursor
            .route_scope_arm_recv_index(scope_id, arm)
            .is_some()
    }

    #[cfg(test)]
    #[inline]
    fn route_token_has_materialization_evidence(
        &self,
        scope_id: ScopeId,
        token: RouteDecisionToken,
    ) -> bool {
        let arm = token.arm().as_u8();
        if !self.arm_requires_materialization_ready_evidence(scope_id, arm) {
            return true;
        }
        self.scope_has_ready_arm(scope_id, arm)
    }

    fn take_hint_for_lane(
        &mut self,
        lane_idx: usize,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) -> Option<u8> {
        if suppress_hint {
            return None;
        }
        let previous_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        let port = self.port_for_lane(lane_idx);
        let taken = if !port.has_route_hint_for_label_mask(label_meta.hint_label_mask) {
            None
        } else {
            port.take_route_hint_for_label_mask(label_meta.hint_label_mask)
        };
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
        taken
    }

    #[inline]
    fn pending_scope_ack_lane_mask(
        &self,
        lane_idx: usize,
        scope_id: ScopeId,
        offer_lane_mask: u8,
    ) -> u8 {
        let port = self.port_for_lane(lane_idx);
        (port.pending_route_decision_lane_mask(scope_id, ROLE) as u8) & offer_lane_mask
    }

    #[inline]
    fn pending_scope_hint_lane_mask(
        &mut self,
        lane_idx: usize,
        offer_lane_mask: u8,
        label_meta: ScopeLabelMeta,
    ) -> u8 {
        let previous_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        let port = self.port_for_lane(lane_idx);
        let lane_mask =
            (port.pending_route_hint_lane_mask_for_label_mask(label_meta.hint_label_mask) as u8)
                & offer_lane_mask;
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
        lane_mask
    }

    #[inline]
    fn preview_scope_ack_token_non_consuming(
        &self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lane_mask: u8,
    ) -> Option<RouteDecisionToken> {
        if let Some(token) = self.peek_scope_ack(scope_id) {
            return Some(token);
        }
        if summary_lane_idx >= MAX_LANES {
            return None;
        }
        let mut pending_ack_mask =
            self.pending_scope_ack_lane_mask(summary_lane_idx, scope_id, offer_lane_mask);
        while let Some(lane_idx) = Self::next_lane_in_mask(&mut pending_ack_mask) {
            let Some(arm) = self
                .port_for_lane(lane_idx)
                .peek_route_decision(scope_id, ROLE)
            else {
                continue;
            };
            if let Some(arm) = Arm::new(arm) {
                return Some(RouteDecisionToken::from_ack(arm));
            }
        }
        None
    }

    #[inline]
    fn next_lane_in_mask(lane_mask: &mut u8) -> Option<usize> {
        if *lane_mask == 0 {
            return None;
        }
        let lane_idx = lane_mask.trailing_zeros() as usize;
        *lane_mask &= !(1u8 << lane_idx);
        Some(lane_idx)
    }

    #[inline]
    fn take_preferred_lane_in_mask(preferred_lane_idx: usize, lane_mask: &mut u8) -> Option<usize> {
        if preferred_lane_idx < MAX_LANES && (*lane_mask & (1u8 << preferred_lane_idx)) != 0 {
            *lane_mask &= !(1u8 << preferred_lane_idx);
            return Some(preferred_lane_idx);
        }
        Self::next_lane_in_mask(lane_mask)
    }

    #[inline]
    fn ack_route_decision_for_lane(
        &mut self,
        lane_idx: usize,
        scope_id: ScopeId,
        role: u8,
    ) -> Option<u8> {
        let previous_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        let arm = self
            .port_for_lane(lane_idx)
            .ack_route_decision(scope_id, role);
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
        arm
    }

    #[inline]
    fn record_route_decision_for_lane(&mut self, lane_idx: usize, scope_id: ScopeId, arm: u8) {
        let previous_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        self.port_for_lane(lane_idx)
            .record_route_decision(scope_id, arm);
        self.refresh_frontier_observation_cache_for_route_lane(lane_idx, previous_change_epoch);
    }

    fn ingest_binding_scope_evidence(
        &mut self,
        scope_id: ScopeId,
        label: u8,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        let hint_matches_scope = Self::hint_matches_scope(label_meta, label, false);
        let exact_static_passive_arm =
            self.static_passive_dispatch_arm_from_exact_label(scope_id, label, label_meta);
        if !hint_matches_scope && exact_static_passive_arm.is_none() {
            return;
        }
        if suppress_hint || !hint_matches_scope {
            self.mark_scope_ready_arm_from_binding_label(scope_id, label, label_meta);
            return;
        }
        self.record_scope_hint(scope_id, label);
        self.mark_scope_ready_arm_from_binding_label(scope_id, label, label_meta);
    }

    fn ingest_scope_evidence_for_lane(
        &mut self,
        lane_idx: usize,
        scope_id: ScopeId,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        if suppress_hint {
            // Dynamic scope route-arm authority is ACK/resolver/poll only.
            // Transport hints remain dispatch/readiness evidence only.
            if let Some(label) = self.take_hint_for_lane(lane_idx, false, label_meta) {
                self.record_scope_hint_dynamic(scope_id, label);
                self.mark_scope_ready_arm_from_label(scope_id, label, label_meta);
            }

            if let Some(arm) = self.ack_route_decision_for_lane(lane_idx, scope_id, ROLE) {
                if let Some(arm) = Arm::new(arm) {
                    self.record_scope_ack(scope_id, RouteDecisionToken::from_ack(arm));
                }
            }
            return;
        }
        if let Some(arm) = self.ack_route_decision_for_lane(lane_idx, scope_id, ROLE) {
            if let Some(arm) = Arm::new(arm) {
                self.record_scope_ack(scope_id, RouteDecisionToken::from_ack(arm));
            }
        }
        if let Some(label) = self.take_hint_for_lane(lane_idx, suppress_hint, label_meta) {
            self.record_scope_hint(scope_id, label);
        }
    }

    fn ingest_scope_evidence_for_offer(
        &mut self,
        scope_id: ScopeId,
        summary_lane_idx: usize,
        offer_lane_mask: u8,
        suppress_hint: bool,
        label_meta: ScopeLabelMeta,
    ) {
        if offer_lane_mask == 0 {
            return;
        }
        let pending_ack_mask =
            self.pending_scope_ack_lane_mask(summary_lane_idx, scope_id, offer_lane_mask);
        let pending_hint_mask =
            self.pending_scope_hint_lane_mask(summary_lane_idx, offer_lane_mask, label_meta);
        let mut pending_evidence_mask = pending_ack_mask | pending_hint_mask;
        if pending_evidence_mask == 0 {
            return;
        }
        while let Some(lane_idx) = Self::next_lane_in_mask(&mut pending_evidence_mask) {
            self.ingest_scope_evidence_for_lane(lane_idx, scope_id, suppress_hint, label_meta);
        }
    }

    fn arm_has_recv(&self, scope_id: ScopeId, arm: u8) -> bool {
        if Self::scope_arm_has_recv(&self.cursor, scope_id, arm) {
            return true;
        }
        self.preview_passive_materialization_index_for_selected_arm(scope_id, arm)
            .map(|target_idx| self.cursor.with_index(target_idx).try_recv_meta().is_some())
            .unwrap_or(false)
    }

    fn propagate_recvless_parent_route_decision(&mut self, child_scope: ScopeId, arm: u8) {
        let Some(parent_scope) = self.cursor.scope_parent(child_scope) else {
            return;
        };
        if parent_scope.kind() != ScopeKind::Route {
            return;
        }
        let Some(parent_region) = self.cursor.scope_region_by_id(parent_scope) else {
            return;
        };
        if !parent_region.linger {
            return;
        }
        if self.cursor.is_route_controller(parent_scope) {
            return;
        }
        let parent_is_dynamic = self
            .cursor
            .route_scope_controller_policy(parent_scope)
            .map(|(policy, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        if parent_is_dynamic {
            return;
        }
        let parent_requires_wire_recv = {
            let mut arm = 0u8;
            let mut requires_wire = false;
            while arm <= 1 {
                if self.arm_has_recv(parent_scope, arm) {
                    let label = self
                        .cursor
                        .controller_arm_entry_by_arm(parent_scope, arm)
                        .map(|(_, label)| label);
                    if let Some(label) = label {
                        if !self.is_non_wire_loop_control_recv(parent_scope, arm, label) {
                            requires_wire = true;
                            break;
                        }
                    } else {
                        requires_wire = true;
                        break;
                    }
                }
                if arm == 1 {
                    break;
                }
                arm += 1;
            }
            requires_wire
        };
        if parent_requires_wire_recv {
            return;
        }
        let Some(parent_arm) = Arm::new(arm) else {
            return;
        };
        self.record_scope_ack(parent_scope, RouteDecisionToken::from_ack(parent_arm));
        let parent_lane = self.offer_lane_for_scope(parent_scope);
        self.record_route_decision_for_lane(parent_lane as usize, parent_scope, parent_arm.as_u8());
        self.emit_route_decision(
            parent_scope,
            parent_arm.as_u8(),
            RouteDecisionSource::Ack,
            parent_lane,
        );
    }

    #[inline]
    fn controller_arm_at_cursor(&self, scope_id: ScopeId) -> Option<u8> {
        let idx = self.cursor.index();
        if let Some((entry, _label)) = self.cursor.controller_arm_entry_by_arm(scope_id, 0)
            && idx == state_index_to_usize(entry)
        {
            return Some(0);
        }
        if let Some((entry, _label)) = self.cursor.controller_arm_entry_by_arm(scope_id, 1)
            && idx == state_index_to_usize(entry)
        {
            return Some(1);
        }
        None
    }

    fn is_non_wire_loop_control_recv(&self, scope_id: ScopeId, arm: u8, label: u8) -> bool {
        let Some(PassiveArmNavigation::WithinArm { entry }) = self
            .cursor
            .follow_passive_observer_arm_for_scope(scope_id, arm)
        else {
            return false;
        };
        let target_cursor = self.cursor.with_index(state_index_to_usize(entry));
        let Some(recv_meta) = target_cursor.try_recv_meta() else {
            return false;
        };
        if !recv_meta.is_control || recv_meta.label != label {
            return false;
        }
        if recv_meta.peer == ROLE {
            return true;
        }
        // Passive observers model controller self-send loop control as cross-role
        // control recv nodes; treat these labels as non-wire arm selectors.
        !self.cursor.is_route_controller(scope_id)
            && matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK)
    }

    fn take_binding_for_lane(
        &mut self,
        lane_idx: usize,
    ) -> Option<crate::binding::IncomingClassification> {
        let previous_nonempty_mask = self.binding_inbox.nonempty_mask;
        let classification = self.binding_inbox.take_or_poll(&mut self.binding, lane_idx);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty_mask);
        classification
    }

    fn put_back_binding_for_lane(
        &mut self,
        lane_idx: usize,
        classification: crate::binding::IncomingClassification,
    ) {
        let previous_nonempty_mask = self.binding_inbox.nonempty_mask;
        self.binding_inbox.put_back(lane_idx, classification);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty_mask);
    }

    fn take_matching_binding_for_lane(
        &mut self,
        lane_idx: usize,
        expected_label: u8,
    ) -> Option<crate::binding::IncomingClassification> {
        let previous_nonempty_mask = self.binding_inbox.nonempty_mask;
        let classification =
            self.binding_inbox
                .take_matching_or_poll(&mut self.binding, lane_idx, expected_label);
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty_mask);
        classification
    }

    fn take_matching_mask_binding_for_lane<F: FnMut(u8) -> bool>(
        &mut self,
        lane_idx: usize,
        label_mask: u128,
        drop_label_mask: u128,
        drop_mismatch: F,
    ) -> Option<crate::binding::IncomingClassification> {
        let previous_nonempty_mask = self.binding_inbox.nonempty_mask;
        let classification = self.binding_inbox.take_matching_mask_or_poll(
            &mut self.binding,
            lane_idx,
            label_mask,
            drop_label_mask,
            drop_mismatch,
        );
        self.refresh_frontier_observation_cache_for_binding_lane(lane_idx, previous_nonempty_mask);
        classification
    }

    fn take_binding_for_selected_arm(
        &mut self,
        lane_idx: usize,
        selected_arm: u8,
        label_meta: ScopeLabelMeta,
        binding_classification: &mut Option<crate::binding::IncomingClassification>,
    ) -> (Option<crate::binding::Channel>, Option<u16>, bool) {
        let label_mask = label_meta.binding_demux_label_mask_for_arm(selected_arm);
        let drop_label_mask = ScopeLabelMeta::label_bit(LABEL_LOOP_CONTINUE)
            | ScopeLabelMeta::label_bit(LABEL_LOOP_BREAK);
        let mut channel = None;
        let mut instance = None;
        let mut has_fin = false;

        if let Some(classification) = binding_classification.take() {
            let label_bit = ScopeLabelMeta::label_bit(classification.label);
            if (label_mask & label_bit) != 0 {
                channel = Some(classification.channel);
                instance = Some(classification.instance);
                has_fin = classification.has_fin;
            } else {
                self.put_back_binding_for_lane(lane_idx, classification);
            }
        }

        if (channel.is_none() || instance.is_none())
            && let Some(classification) = self.take_matching_mask_binding_for_lane(
                lane_idx,
                label_mask,
                drop_label_mask,
                |label| matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK),
            )
        {
            if channel.is_none() {
                channel = Some(classification.channel);
            }
            if instance.is_none() {
                instance = Some(classification.instance);
            }
            if classification.has_fin {
                has_fin = true;
            }
        }

        (channel, instance, has_fin)
    }

    fn poll_binding_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if offer_lane_mask == 0 {
            return None;
        }
        let preferred_arm = self
            .peek_scope_ack(scope_id)
            .map(|token| token.arm().as_u8());
        let mut label_mask = label_meta.preferred_binding_label_mask(preferred_arm);
        if label_mask == 0 && self.static_passive_scope_evidence_materializes_poll(scope_id) {
            label_mask = label_meta.binding_demux_label_mask_for_arm(0)
                | label_meta.binding_demux_label_mask_for_arm(1);
        }
        if label_mask == 0 {
            return None;
        }
        let authoritative_lane_mask = preferred_arm
            .map(|arm| materialization_meta.binding_demux_lane_mask(Some(arm)) & offer_lane_mask)
            .unwrap_or(0);
        let label_lane_mask = materialization_meta
            .binding_demux_lane_mask_for_label_mask(label_meta, label_mask)
            & offer_lane_mask;
        let base_lane_mask = if authoritative_lane_mask != 0 {
            authoritative_lane_mask
        } else if label_lane_mask != 0 {
            label_lane_mask
        } else {
            offer_lane_mask
        };
        if let Some(expected_label) = label_meta.preferred_binding_label(preferred_arm) {
            let buffered_lane_mask = self
                .binding_inbox
                .buffered_lane_mask_for_labels(ScopeLabelMeta::label_bit(expected_label))
                & offer_lane_mask;
            if let Some(picked) = self.poll_binding_exact_for_offer(
                offer_lane_idx,
                base_lane_mask | buffered_lane_mask,
                expected_label,
            ) {
                return Some(picked);
            }
        }
        let binding_lane_mask = base_lane_mask
            | (self.binding_inbox.buffered_lane_mask_for_labels(label_mask) & offer_lane_mask);
        if let Some(classification) =
            self.poll_binding_mask_for_offer(offer_lane_idx, binding_lane_mask, label_mask)
        {
            return Some(classification);
        }
        if self.static_passive_scope_evidence_materializes_poll(scope_id)
            && let Some((lane_idx, classification)) =
                self.poll_binding_any_for_offer(offer_lane_idx, offer_lane_mask)
        {
            if self
                .static_passive_dispatch_arm_from_exact_label(scope_id, classification.label, label_meta)
                .is_some()
            {
                return Some((lane_idx, classification));
            }
            self.put_back_binding_for_lane(lane_idx, classification);
        }
        None
    }

    fn poll_binding_mask_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
        label_mask: u128,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        let drop_label_mask = ScopeLabelMeta::label_bit(LABEL_LOOP_CONTINUE)
            | ScopeLabelMeta::label_bit(LABEL_LOOP_BREAK);
        let matching_buffered_lane_mask =
            self.binding_inbox.buffered_lane_mask_for_labels(label_mask) & offer_lane_mask;
        if let Some(classification) = self.poll_buffered_binding_mask_for_offer(
            offer_lane_idx,
            matching_buffered_lane_mask,
            label_mask,
            drop_label_mask,
        ) {
            return Some(classification);
        }
        let drop_buffered_lane_mask = (self
            .binding_inbox
            .buffered_lane_mask_for_labels(drop_label_mask)
            & offer_lane_mask)
            & !matching_buffered_lane_mask;
        if let Some(classification) = self.poll_buffered_binding_mask_for_offer(
            offer_lane_idx,
            drop_buffered_lane_mask,
            label_mask,
            drop_label_mask,
        ) {
            return Some(classification);
        }
        self.poll_binding_mask_in_lane_mask(
            offer_lane_idx,
            offer_lane_mask & !(matching_buffered_lane_mask | drop_buffered_lane_mask),
            label_mask,
            drop_label_mask,
        )
    }

    fn poll_buffered_binding_mask_for_offer(
        &mut self,
        offer_lane_idx: usize,
        lane_mask: u8,
        label_mask: u128,
        drop_label_mask: u128,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if lane_mask == 0 {
            return None;
        }
        let mut remaining_lane_mask = lane_mask;
        while let Some(lane_slot) =
            Self::take_preferred_lane_in_mask(offer_lane_idx, &mut remaining_lane_mask)
        {
            if let Some(classification) = self.take_matching_mask_binding_for_lane(
                lane_slot,
                label_mask,
                drop_label_mask,
                |label| matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK),
            ) {
                return Some((lane_slot, classification));
            }
        }
        None
    }

    fn poll_binding_mask_in_lane_mask(
        &mut self,
        offer_lane_idx: usize,
        lane_mask: u8,
        label_mask: u128,
        drop_label_mask: u128,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if lane_mask == 0 {
            return None;
        }
        let mut selected_lane_mask = lane_mask;
        let Some(lane_slot) =
            Self::take_preferred_lane_in_mask(offer_lane_idx, &mut selected_lane_mask)
        else {
            return None;
        };
        if let Some(classification) = self.take_matching_mask_binding_for_lane(
            lane_slot,
            label_mask,
            drop_label_mask,
            |label| matches!(label, LABEL_LOOP_CONTINUE | LABEL_LOOP_BREAK),
        ) {
            // Classification is demux evidence only.
            return Some((lane_slot, classification));
        }
        None
    }

    fn poll_binding_exact_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
        expected_label: u8,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if offer_lane_mask == 0 {
            return None;
        }
        let buffered_lane_mask = self
            .binding_inbox
            .buffered_lane_mask_for_labels(ScopeLabelMeta::label_bit(expected_label))
            & offer_lane_mask;
        if let Some(classification) =
            self.poll_binding_exact_in_lane_mask(offer_lane_idx, buffered_lane_mask, expected_label)
        {
            return Some(classification);
        }
        self.poll_binding_exact_in_lane_mask(
            offer_lane_idx,
            offer_lane_mask & !buffered_lane_mask,
            expected_label,
        )
    }

    fn poll_binding_exact_in_lane_mask(
        &mut self,
        offer_lane_idx: usize,
        lane_mask: u8,
        expected_label: u8,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if lane_mask == 0 {
            return None;
        }
        let mut remaining_lane_mask = lane_mask;
        while let Some(lane_idx) =
            Self::take_preferred_lane_in_mask(offer_lane_idx, &mut remaining_lane_mask)
        {
            if let Some(classification) =
                self.take_matching_binding_for_lane(lane_idx, expected_label)
            {
                return Some((lane_idx, classification));
            }
        }
        None
    }

    fn poll_binding_any_for_offer(
        &mut self,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
    ) -> Option<(usize, crate::binding::IncomingClassification)> {
        if offer_lane_mask == 0 {
            return None;
        }
        let mut remaining_lane_mask = offer_lane_mask;
        while let Some(lane_idx) =
            Self::take_preferred_lane_in_mask(offer_lane_idx, &mut remaining_lane_mask)
        {
            if let Some(classification) = self.take_binding_for_lane(lane_idx) {
                return Some((lane_idx, classification));
            }
        }
        None
    }

    #[inline]
    fn cache_binding_classification_for_offer(
        &mut self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
        offer_lane_mask: u8,
        label_meta: ScopeLabelMeta,
        materialization_meta: ScopeArmMaterializationMeta,
        binding_classification: &mut Option<crate::binding::IncomingClassification>,
    ) {
        if binding_classification.is_some() {
            return;
        }
        if let Some((lane_idx, classification)) = self.poll_binding_for_offer(
            scope_id,
            offer_lane_idx,
            offer_lane_mask,
            label_meta,
            materialization_meta,
        ) {
            if binding_classification.is_none() {
                *binding_classification = Some(classification);
            } else {
                self.put_back_binding_for_lane(lane_idx, classification);
            }
        }
    }

    fn try_recv_from_binding(
        &mut self,
        logical_lane: u8,
        expected_label: u8,
        buf: &mut [u8],
    ) -> RecvResult<Option<usize>> {
        let lane_idx = logical_lane as usize;
        if let Some(classification) = self.take_matching_binding_for_lane(lane_idx, expected_label)
        {
            let n = self
                .binding
                .on_recv(classification.channel, buf)
                .map_err(RecvError::Binding)?;
            return Ok(Some(n));
        }
        Ok(None)
    }

    fn is_loop_control_scope(cursor: &PhaseCursor<ROLE>, scope_id: ScopeId) -> bool {
        let Some((_, label0)) = cursor.controller_arm_entry_by_arm(scope_id, 0) else {
            return false;
        };
        let Some((_, label1)) = cursor.controller_arm_entry_by_arm(scope_id, 1) else {
            return false;
        };
        (label0 == LABEL_LOOP_CONTINUE && label1 == LABEL_LOOP_BREAK)
            || (label0 == LABEL_LOOP_BREAK && label1 == LABEL_LOOP_CONTINUE)
    }

    fn parallel_scope_root(cursor: &PhaseCursor<ROLE>, mut scope_id: ScopeId) -> Option<ScopeId> {
        loop {
            if scope_id.kind() == ScopeKind::Parallel {
                return Some(scope_id);
            }
            let Some(parent) = cursor.scope_parent(scope_id) else {
                return None;
            };
            scope_id = parent;
        }
    }

    #[inline]
    fn frontier_kind_for_cursor(
        cursor: &PhaseCursor<ROLE>,
        scope_id: ScopeId,
        is_controller: bool,
    ) -> FrontierKind {
        if cursor.jump_reason() == Some(JumpReason::PassiveObserverBranch) {
            return FrontierKind::PassiveObserver;
        }
        let has_controller_entry = cursor.controller_arm_entry_by_arm(scope_id, 0).is_some()
            || cursor.controller_arm_entry_by_arm(scope_id, 1).is_some();
        if !is_controller && !has_controller_entry {
            return FrontierKind::PassiveObserver;
        }
        if let Some(region) = cursor.scope_region_by_id(scope_id)
            && region.linger
        {
            return FrontierKind::Loop;
        }
        if Self::parallel_scope_root(cursor, scope_id).is_some() {
            return FrontierKind::Parallel;
        }
        FrontierKind::Route
    }

    #[inline]
    fn scope_loop_meta(cursor: &PhaseCursor<ROLE>, scope_id: ScopeId) -> ScopeLoopMeta {
        let mut flags = 0u8;
        if cursor.typestate_node(cursor.index()).loop_scope().is_some() {
            flags |= ScopeLoopMeta::FLAG_SCOPE_ACTIVE;
        }
        if cursor
            .scope_region_by_id(scope_id)
            .map(|region| region.linger)
            .unwrap_or(false)
        {
            flags |= ScopeLoopMeta::FLAG_SCOPE_LINGER;
        }
        if Self::is_loop_control_scope(cursor, scope_id) {
            flags |= ScopeLoopMeta::FLAG_CONTROL_SCOPE;
        }
        if cursor.route_scope_arm_recv_index(scope_id, 0).is_some() {
            flags |= ScopeLoopMeta::FLAG_CONTINUE_HAS_RECV;
        }
        if cursor.route_scope_arm_recv_index(scope_id, 1).is_some() {
            flags |= ScopeLoopMeta::FLAG_BREAK_HAS_RECV;
        }
        ScopeLoopMeta { flags }
    }

    #[inline]
    fn scope_label_meta(
        cursor: &PhaseCursor<ROLE>,
        scope_id: ScopeId,
        loop_meta: ScopeLoopMeta,
    ) -> ScopeLabelMeta {
        let is_controller = cursor.is_route_controller(scope_id);
        let mut meta = ScopeLabelMeta {
            #[cfg(test)]
            scope_id,
            loop_meta,
            ..ScopeLabelMeta::EMPTY
        };
        if let Some(recv_meta) = cursor.try_recv_meta()
            && recv_meta.scope == scope_id
        {
            meta.recv_label = recv_meta.label;
            meta.flags |= ScopeLabelMeta::FLAG_CURRENT_RECV_LABEL;
            meta.record_hint_label(recv_meta.label);
            if let Some(arm) = recv_meta.route_arm {
                meta.recv_arm = arm;
                meta.flags |= ScopeLabelMeta::FLAG_CURRENT_RECV_ARM;
                meta.record_arm_label(arm, recv_meta.label);
                if !Self::current_recv_is_scope_local(
                    cursor,
                    scope_id,
                    loop_meta,
                    recv_meta.label,
                    arm,
                ) {
                    meta.flags |= ScopeLabelMeta::FLAG_CURRENT_RECV_BINDING_EXCLUDED;
                }
            }
        }
        if let Some((_, label)) = cursor.controller_arm_entry_by_arm(scope_id, 0) {
            meta.controller_labels[0] = label;
            meta.flags |= ScopeLabelMeta::FLAG_CONTROLLER_ARM0;
            if is_controller {
                meta.record_arm_label(0, label);
            } else {
                meta.record_dispatch_arm_label(0, label);
                meta.clear_evidence_arm_label(0, label);
            }
        }
        if let Some((_, label)) = cursor.controller_arm_entry_by_arm(scope_id, 1) {
            meta.controller_labels[1] = label;
            meta.flags |= ScopeLabelMeta::FLAG_CONTROLLER_ARM1;
            if is_controller {
                meta.record_arm_label(1, label);
            } else {
                meta.record_dispatch_arm_label(1, label);
                meta.clear_evidence_arm_label(1, label);
            }
        }
        if loop_meta.loop_label_scope() {
            meta.record_arm_label(0, LABEL_LOOP_CONTINUE);
            meta.record_arm_label(1, LABEL_LOOP_BREAK);
        }
        let mut dispatch_idx = 0usize;
        while let Some((label, arm, _)) =
            cursor.route_scope_first_recv_dispatch_entry(scope_id, dispatch_idx)
        {
            meta.record_dispatch_arm_label(arm, label);
            dispatch_idx += 1;
        }
        meta
    }

    #[inline]
    fn offer_scope_label_meta(&self, scope_id: ScopeId, offer_lane_idx: usize) -> ScopeLabelMeta {
        if offer_lane_idx < MAX_LANES {
            let info = self.lane_offer_state[offer_lane_idx];
            if info.scope == scope_id {
                if let Some(cached) =
                    self.offer_entry_label_meta(scope_id, state_index_to_usize(info.entry))
                {
                    return cached;
                }
                return info.label_meta;
            }
        }
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id) {
            let entry_idx = if offer_entry.is_max() {
                self.cursor.index()
            } else {
                state_index_to_usize(offer_entry)
            };
            if let Some(cached) = self.offer_entry_label_meta(scope_id, entry_idx) {
                return cached;
            }
        }
        let loop_meta = Self::scope_loop_meta(&self.cursor, scope_id);
        Self::scope_label_meta(&self.cursor, scope_id, loop_meta)
    }

    #[inline]
    fn offer_scope_materialization_meta(
        &self,
        scope_id: ScopeId,
        offer_lane_idx: usize,
    ) -> ScopeArmMaterializationMeta {
        if offer_lane_idx < MAX_LANES {
            let info = self.lane_offer_state[offer_lane_idx];
            if info.scope == scope_id {
                if let Some(cached) = self
                    .offer_entry_materialization_meta(scope_id, state_index_to_usize(info.entry))
                {
                    return cached;
                }
            }
        }
        if let Some(offer_entry) = self.cursor.route_scope_offer_entry(scope_id) {
            let entry_idx = if offer_entry.is_max() {
                self.cursor.index()
            } else {
                state_index_to_usize(offer_entry)
            };
            if let Some(cached) = self.offer_entry_materialization_meta(scope_id, entry_idx) {
                return cached;
            }
        }
        self.compute_scope_arm_materialization_meta(scope_id)
    }

    #[inline]
    fn frontier_static_facts(
        cursor: &PhaseCursor<ROLE>,
        scope_id: ScopeId,
        is_controller: bool,
        is_dynamic: bool,
    ) -> FrontierStaticFacts {
        let loop_meta = Self::scope_loop_meta(cursor, scope_id);
        let controller_local_ready =
            is_controller && Self::scope_has_controller_arm_entry(cursor, scope_id);
        let cursor_ready = cursor.is_recv()
            || cursor.try_recv_meta().is_some()
            || cursor.try_local_meta().is_some();
        FrontierStaticFacts {
            frontier: Self::frontier_kind_for_cursor(cursor, scope_id, is_controller),
            loop_meta,
            ready: loop_meta.recvless_ready()
                || controller_local_ready
                || is_dynamic
                || cursor_ready,
        }
    }

    #[inline]
    fn ack_is_progress_evidence(loop_meta: ScopeLoopMeta, has_ack: bool) -> bool {
        has_ack && !loop_meta.control_scope()
    }

    fn for_each_active_offer_candidate<R>(
        &mut self,
        current_parallel: Option<ScopeId>,
        mut visitor: impl FnMut(FrontierCandidate) -> ControlFlow<R>,
    ) -> Option<R> {
        let active_entries = self.active_frontier_entries(current_parallel);
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(entry_idx) =
            self.next_active_frontier_entry(active_entries, &mut remaining_entries)
        {
            let Some(candidate) = self.scan_offer_entry_candidate_non_consuming(entry_idx) else {
                continue;
            };
            if let ControlFlow::Break(result) = visitor(candidate) {
                return Some(result);
            }
        }
        None
    }

    fn on_frontier_defer(
        &mut self,
        liveness: &mut OfferLivenessState,
        scope_id: ScopeId,
        current_parallel: Option<ScopeId>,
        source: DeferSource,
        reason: DeferReason,
        retry_hint: u8,
        offer_lane: u8,
        binding_ready: bool,
        selected_arm: Option<u8>,
        visited: &mut FrontierVisitSet,
    ) -> FrontierDeferOutcome {
        let fingerprint = self.evidence_fingerprint(scope_id, binding_ready);
        let budget = liveness.on_defer(fingerprint);
        let exhausted = matches!(budget, DeferBudgetOutcome::Exhausted);
        let is_controller = self.cursor.is_route_controller(scope_id);
        let frontier = Self::frontier_kind_for_cursor(&self.cursor, scope_id, is_controller);
        let hint = self.peek_scope_hint(scope_id);
        let ready_arm_mask = self.scope_ready_arm_mask(scope_id);
        self.emit_policy_defer_event(
            source,
            reason,
            scope_id,
            frontier,
            selected_arm,
            hint,
            retry_hint,
            *liveness,
            ready_arm_mask,
            binding_ready,
            exhausted,
            offer_lane,
        );
        visited.record(scope_id);
        let current_entry_idx = self.cursor.index();
        let current_cursor = self.cursor.with_index(current_entry_idx);
        let current_is_controller = current_cursor.is_route_controller(scope_id);
        let mut snapshot = FrontierSnapshot {
            current_scope: scope_id,
            current_entry_idx,
            current_parallel_root: current_parallel.unwrap_or(ScopeId::none()),
            current_frontier: Self::frontier_kind_for_cursor(
                &current_cursor,
                scope_id,
                current_is_controller,
            ),
            candidates: [FrontierCandidate::EMPTY; MAX_LANES],
            candidate_len: 0,
        };
        self.for_each_active_offer_candidate(current_parallel, |candidate| {
            if snapshot.candidate_len < MAX_LANES {
                snapshot.candidates[snapshot.candidate_len] = candidate;
                snapshot.candidate_len += 1;
            }
            ControlFlow::<()>::Continue(())
        });
        if exhausted {
            let Some(candidate) = snapshot.select_exhausted_controller_candidate(*visited) else {
                return FrontierDeferOutcome::Exhausted;
            };
            visited.record(candidate.scope_id);
            if candidate.entry_idx != self.cursor.index() {
                self.set_cursor(self.cursor.with_index(candidate.entry_idx));
            }
            return FrontierDeferOutcome::Yielded;
        }
        let Some(candidate) = snapshot.select_yield_candidate(*visited) else {
            return FrontierDeferOutcome::Continue;
        };
        visited.record(candidate.scope_id);
        if candidate.entry_idx != self.cursor.index() {
            self.set_cursor(self.cursor.with_index(candidate.entry_idx));
        }
        FrontierDeferOutcome::Yielded
    }

    fn current_scope_selection_meta(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
        current_frontier: CurrentFrontierSelectionState,
    ) -> Option<CurrentScopeSelectionMeta> {
        if let Some(meta) = self.offer_entry_selection_meta(scope_id, current_idx) {
            return Some(meta);
        }
        let Some(region) = self.cursor.scope_region_by_id(scope_id) else {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        };
        if region.kind != ScopeKind::Route {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        }
        let offer_entry = self.cursor.route_scope_offer_entry(region.scope_id)?;
        let route_entry_idx = if offer_entry.is_max() {
            current_idx
        } else {
            state_index_to_usize(offer_entry)
        };
        if !offer_entry.is_max() && route_entry_idx != current_idx {
            return Some(CurrentScopeSelectionMeta::EMPTY);
        }
        let mut flags = CurrentScopeSelectionMeta::FLAG_ROUTE_ENTRY;
        if self.offer_lanes_for_scope(region.scope_id).1 != 0 {
            flags |= CurrentScopeSelectionMeta::FLAG_HAS_OFFER_LANES;
        }
        if current_frontier.is_controller() {
            flags |= CurrentScopeSelectionMeta::FLAG_CONTROLLER;
        }
        Some(CurrentScopeSelectionMeta { flags })
    }

    fn current_frontier_selection_state(
        &self,
        scope_id: ScopeId,
        current_idx: usize,
    ) -> CurrentFrontierSelectionState {
        if let Some(info) = self.offer_entry_lane_state(scope_id, current_idx) {
            let entry_state = self.offer_entry_state[current_idx];
            let entry_parallel = self.offer_entry_parallel_root(current_idx);
            let current_parallel = if !info.parallel_root.is_none()
                && self.root_frontier_active_mask(info.parallel_root) != 0
            {
                Some(info.parallel_root)
            } else {
                entry_parallel
            };
            let mut flags = 0u8;
            if entry_state.summary.is_controller() {
                flags |= CurrentFrontierSelectionState::FLAG_CONTROLLER;
            }
            if entry_state.summary.is_dynamic() {
                flags |= CurrentFrontierSelectionState::FLAG_DYNAMIC;
            }
            return CurrentFrontierSelectionState {
                frontier: entry_state.frontier,
                parallel_root: current_parallel.unwrap_or(ScopeId::none()),
                ready: entry_state.summary.static_ready(),
                has_progress_evidence: false,
                flags,
            };
        }
        let current_cursor = self.cursor.with_index(current_idx);
        let current_is_controller = current_cursor.is_route_controller(scope_id);
        let current_is_dynamic = current_is_controller
            && current_cursor
                .route_scope_controller_policy(scope_id)
                .map(|(policy, _, _)| policy.is_dynamic())
                .unwrap_or(false);
        let static_facts = Self::frontier_static_facts(
            &current_cursor,
            scope_id,
            current_is_controller,
            current_is_dynamic,
        );
        let cursor_parallel = Self::parallel_scope_root(&self.cursor, scope_id);
        let cursor_parallel_has_offer = cursor_parallel
            .map(|root| self.root_frontier_active_mask(root) != 0)
            .unwrap_or(false);
        let current_entry_has_offer = self.offer_entry_active_mask(current_idx) != 0;
        let current_entry_parallel = if cursor_parallel_has_offer || !current_entry_has_offer {
            None
        } else {
            self.offer_entry_parallel_root(current_idx)
        };
        let current_parallel = if cursor_parallel_has_offer {
            cursor_parallel
        } else {
            current_entry_parallel
        };
        let mut flags = 0u8;
        if current_is_controller {
            flags |= CurrentFrontierSelectionState::FLAG_CONTROLLER;
        }
        if current_is_dynamic {
            flags |= CurrentFrontierSelectionState::FLAG_DYNAMIC;
        }
        CurrentFrontierSelectionState {
            frontier: static_facts.frontier,
            parallel_root: current_parallel.unwrap_or(ScopeId::none()),
            ready: static_facts.ready,
            has_progress_evidence: false,
            flags,
        }
    }

    // Stage 1 prepass: align the cursor to the selected route scope entry.
    fn align_cursor_to_selected_scope(&mut self) -> RecvResult<()> {
        let node_scope = self.cursor.node_scope_id();
        let current_scope = self.current_offer_scope_id();
        if current_scope != node_scope
            && let Some(entry_idx) = self.route_scope_offer_entry_index(current_scope)
            && entry_idx != self.cursor.index()
        {
            self.set_cursor(self.cursor.with_index(entry_idx));
            self.sync_lane_offer_state();
            return self.align_cursor_to_selected_scope();
        }
        let node_scope = self.current_offer_scope_id();
        let current_idx = self.cursor.index();
        let mut current_frontier_state =
            self.current_frontier_selection_state(node_scope, current_idx);
        let current_frontier = current_frontier_state.frontier;
        let current_parallel = current_frontier_state.parallel();
        let current_parallel_root = current_frontier_state.parallel_root;
        let current_scope_selected = self.selected_arm_for_scope(node_scope).is_some();
        if current_scope_selected
            && self
                .current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
                .map(|meta| meta.is_route_entry())
                .unwrap_or(false)
        {
            // A descended route entry with a fixed arm is already the selected
            // scope. Stage 1 must not reopen ancestor frontier arbitration and
            // bounce the cursor back to a broader route family entry.
            return Ok(());
        }
        let use_root_observed_entries = current_parallel.is_some();
        let active_entries = self.active_frontier_entries(current_parallel);
        if active_entries.contains_only(current_idx) {
            let Some(current_scope_meta) =
                self.current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
            else {
                return Ok(());
            };
            if current_scope_meta.is_route_entry() && current_scope_meta.has_offer_lanes() {
                return Ok(());
            }
        }
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let mut observed_entries = if use_root_observed_entries {
            self.root_frontier_observed_entries(current_parallel_root)
        } else {
            self.global_frontier_observed_entries()
        };
        if self
            .cached_frontier_observed_entries(
                current_parallel_root,
                use_root_observed_entries,
                observation_key,
            )
            .is_none()
            && observed_entries.len != 0
        {
            self.refresh_frontier_observation_cache(
                current_parallel_root,
                use_root_observed_entries,
            );
            observed_entries = if use_root_observed_entries {
                self.root_frontier_observed_entries(current_parallel_root)
            } else {
                self.global_frontier_observed_entries()
            };
        }
        let reentry_ready_entry_idx =
            self.observed_reentry_entry_idx(observed_entries, current_idx, true);
        let reentry_any_entry_idx =
            self.observed_reentry_entry_idx(observed_entries, current_idx, false);
        let loop_controller_without_evidence =
            current_frontier_state.loop_controller_without_evidence();
        let progress_sibling_exists = if current_parallel_root.is_none() {
            self.global_frontier_progress_sibling_exists(
                current_idx,
                current_frontier,
                loop_controller_without_evidence,
            )
        } else {
            self.root_frontier_progress_sibling_exists(
                current_parallel_root,
                current_idx,
                current_frontier,
                loop_controller_without_evidence,
            )
        };
        let Some(current_scope_meta) =
            self.current_scope_selection_meta(node_scope, current_idx, current_frontier_state)
        else {
            return Ok(());
        };
        let current_is_route_entry = current_scope_meta.is_route_entry();
        let current_has_offer_lanes = current_scope_meta.has_offer_lanes();
        let current_is_controller = current_scope_meta.is_controller();
        let observed_mask = observed_entries.occupancy_mask();
        let current_entry_bit = observed_entries.entry_bit(current_idx);
        if current_entry_bit != 0 {
            current_frontier_state.ready |= (current_entry_bit & observed_entries.ready_mask) != 0;
            current_frontier_state.has_progress_evidence |=
                (current_entry_bit & observed_entries.progress_mask) != 0;
        }
        let current_matches_candidate = current_entry_bit != 0;
        let mut current_has_evidence = (current_entry_bit & observed_entries.progress_mask) != 0;
        let suppress_current_controller_without_evidence = current_is_controller
            && current_matches_candidate
            && (current_entry_bit & observed_entries.ready_arm_mask) == 0
            && (current_entry_bit & observed_entries.progress_mask) == 0
            && progress_sibling_exists;
        let controller_progress_sibling_exists = (observed_entries.progress_mask
            & observed_entries.controller_mask
            & !current_entry_bit)
            != 0;
        let mut static_controller_ready_mask = observed_mask & !observed_entries.controller_mask;
        static_controller_ready_mask |= current_entry_bit & observed_entries.controller_mask;
        static_controller_ready_mask |=
            observed_entries.progress_mask & observed_entries.controller_mask;
        if suppress_current_controller_without_evidence {
            static_controller_ready_mask &= !current_entry_bit;
        }
        let current_entry_unrunnable = current_is_route_entry && !current_has_offer_lanes;
        let mut candidate_mask = current_entry_bit | observed_entries.progress_mask;
        if current_entry_unrunnable {
            candidate_mask |= observed_mask & !current_entry_bit;
        }
        candidate_mask &= static_controller_ready_mask;
        let hinted_mask = candidate_mask & observed_entries.ready_arm_mask;
        let hinted_count = hinted_mask.count_ones() as usize;
        let hint_filter_mask = if hinted_count == 1 { hinted_mask } else { 0 };
        let hint_filter = observed_entries.first_entry_idx(hint_filter_mask);
        let candidate_mask = if hint_filter_mask != 0 {
            hinted_mask
        } else {
            candidate_mask
        };
        let controller_mask = candidate_mask & observed_entries.controller_mask;
        let dynamic_controller_mask = controller_mask & observed_entries.dynamic_controller_mask;
        let candidate_count = candidate_mask.count_ones() as usize;
        let controller_count = controller_mask.count_ones() as usize;
        let dynamic_controller_count = dynamic_controller_mask.count_ones() as usize;
        let candidate_idx = observed_entries.first_entry_idx(candidate_mask);
        let controller_idx = observed_entries.first_entry_idx(controller_mask);
        let dynamic_controller_idx = observed_entries.first_entry_idx(dynamic_controller_mask);
        current_has_evidence |= current_frontier_state.has_progress_evidence;
        let suppress_current_passive_without_evidence =
            should_suppress_current_passive_without_evidence(
                current_frontier,
                current_is_controller,
                current_has_evidence,
                controller_progress_sibling_exists,
            );
        let current_matches_filtered = current_entry_matches_after_filter(
            current_matches_candidate && !suppress_current_passive_without_evidence,
            current_has_offer_lanes,
            current_idx,
            hint_filter,
        );
        let current_is_candidate = current_entry_is_candidate(
            current_matches_filtered,
            current_is_controller,
            current_has_evidence,
            candidate_count,
            progress_sibling_exists,
        );
        let selection = match choose_offer_priority(
            current_is_candidate,
            dynamic_controller_count,
            controller_count,
            candidate_count,
        ) {
            Some(OfferSelectPriority::CurrentOfferEntry) => {
                Some((OfferSelectPriority::CurrentOfferEntry, current_idx))
            }
            Some(OfferSelectPriority::DynamicControllerUnique) => dynamic_controller_idx
                .map(|idx| (OfferSelectPriority::DynamicControllerUnique, idx)),
            Some(OfferSelectPriority::ControllerUnique) => {
                controller_idx.map(|idx| (OfferSelectPriority::ControllerUnique, idx))
            }
            Some(OfferSelectPriority::CandidateUnique) => {
                candidate_idx.map(|idx| (OfferSelectPriority::CandidateUnique, idx))
            }
            None => None,
        };
        if let Some((_priority, entry_idx)) = selection {
            if entry_idx != self.cursor.index() {
                self.set_cursor(self.cursor.with_index(entry_idx));
            }
            return Ok(());
        }
        if self.ensure_current_route_arm_state()?.is_some() {
            return Ok(());
        }
        if current_is_route_entry && current_has_offer_lanes {
            return Ok(());
        }
        if !current_is_route_entry {
            if let Some(entry_idx) = reentry_ready_entry_idx.or(reentry_any_entry_idx) {
                if entry_idx != self.cursor.index() {
                    self.set_cursor(self.cursor.with_index(entry_idx));
                }
                return Ok(());
            }
        }
        Err(RecvError::PhaseInvariant)
    }

    fn skip_unselected_arm_lanes(&mut self, scope: ScopeId, selected_arm: u8, skip_lane: u8) {
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return;
        }
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if lane_idx == skip_lane as usize {
                lane_idx += 1;
                continue;
            }
            let lane_wire = lane_idx as u8;
            if self
                .cursor
                .scope_lane_last_eff_for_arm(scope, selected_arm, lane_wire)
                .is_some()
            {
                lane_idx += 1;
                continue;
            }
            if let Some(eff_index) = self.cursor.scope_lane_last_eff(scope, lane_wire) {
                self.advance_lane_cursor(lane_idx, eff_index);
            }
            lane_idx += 1;
        }
    }

    fn maybe_skip_remaining_route_arm(
        &mut self,
        scope: ScopeId,
        lane: u8,
        arm: Option<u8>,
        eff_index: EffIndex,
    ) {
        let Some(arm) = arm else {
            return;
        };
        if scope.is_none() || scope.kind() != ScopeKind::Route {
            return;
        }
        if let Some(last_arm_eff) = self.cursor.scope_lane_last_eff_for_arm(scope, arm, lane) {
            if last_arm_eff == eff_index {
                if let Some(scope_last) = self.cursor.scope_lane_last_eff(scope, lane) {
                    if scope_last != last_arm_eff {
                        self.advance_lane_cursor(lane as usize, scope_last);
                    }
                }
            }
        }
    }

    #[inline]
    fn offer_refresh_mask(&self) -> u8 {
        self.cursor
            .current_phase()
            .map(|phase| phase.lane_mask)
            .unwrap_or(0)
            | self.lane_linger_mask
            | self.lane_offer_linger_mask
    }

    #[inline]
    fn scope_ordinal_index(scope: ScopeId) -> Option<usize> {
        if scope.is_none() {
            return None;
        }
        let ordinal = scope.canonical().ordinal() as usize;
        if ordinal >= ScopeId::ORDINAL_CAPACITY as usize {
            return None;
        }
        Some(ordinal)
    }

    #[inline]
    fn root_frontier_slot(&self, root: ScopeId) -> Option<usize> {
        if root.is_none() {
            return None;
        }
        let root = root.canonical();
        let ordinal = Self::scope_ordinal_index(root)?;
        let slot_idx = *self.root_frontier_slot_by_ordinal.get(ordinal)?;
        if slot_idx == u8::MAX {
            return None;
        }
        let slot_idx = slot_idx as usize;
        if slot_idx >= self.root_frontier_len as usize {
            return None;
        }
        let slot = self.root_frontier_state[slot_idx];
        (slot.root == root).then_some(slot_idx)
    }

    #[inline]
    fn root_frontier_active_mask(&self, root: ScopeId) -> u8 {
        self.root_frontier_slot(root)
            .map(|slot| self.root_frontier_state[slot].active_mask)
            .unwrap_or(0)
    }

    #[inline]
    fn root_frontier_active_entries(&self, root: ScopeId) -> ActiveEntrySet {
        self.root_frontier_slot(root)
            .map(|slot| self.root_frontier_state[slot].active_entries)
            .unwrap_or(ActiveEntrySet::EMPTY)
    }

    #[inline]
    fn root_frontier_offer_lane_mask(&self, root: ScopeId) -> u8 {
        self.root_frontier_slot(root)
            .map(|slot| self.root_frontier_state[slot].offer_lane_mask)
            .unwrap_or(0)
    }

    #[inline]
    fn offer_entry_active_mask(&self, entry_idx: usize) -> u8 {
        self.offer_entry_state
            .get(entry_idx)
            .map(|state| state.active_mask)
            .unwrap_or(0)
    }

    #[inline]
    fn active_frontier_entries(&self, current_parallel: Option<ScopeId>) -> ActiveEntrySet {
        current_parallel
            .map(|root| self.root_frontier_active_entries(root))
            .unwrap_or(self.global_active_entries)
    }

    #[inline]
    fn frontier_observation_lane_mask(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> u8 {
        if use_root_observed_entries {
            self.root_frontier_offer_lane_mask(current_parallel_root)
        } else {
            self.global_offer_lane_mask
        }
    }

    #[inline]
    fn frontier_observation_offer_lane_entry_slot_masks(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> [u8; MAX_LANES] {
        if use_root_observed_entries {
            return self
                .root_frontier_slot(current_parallel_root)
                .map(|slot_idx| self.root_frontier_state[slot_idx].offer_lane_entry_slot_masks)
                .unwrap_or([0; MAX_LANES]);
        }
        #[cfg(feature = "std")]
        {
            *self.global_offer_lane_entry_slot_masks
        }
        #[cfg(not(feature = "std"))]
        {
            self.global_offer_lane_entry_slot_masks
        }
    }

    #[inline]
    fn bump_scope_evidence_generation(&mut self, slot: usize) {
        let Some(generation) = self.scope_evidence_generations.get_mut(slot) else {
            return;
        };
        let next = generation.wrapping_add(1);
        *generation = if next == 0 { 1 } else { next };
    }

    #[inline]
    fn bump_scope_evidence_generation_for_scope(&mut self, scope_id: ScopeId, slot: usize) {
        self.bump_scope_evidence_generation(slot);
        self.refresh_frontier_observation_cache_for_scope(scope_id);
    }

    #[inline]
    fn frontier_observation_key(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> FrontierObservationKey {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let offer_lane_mask =
            self.frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        let active_entry_indices = active_entries.entries;
        let mut entry_summary_fingerprints = [0; MAX_LANES];
        let mut scope_generations = [0; MAX_LANES];
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
                continue;
            };
            entry_summary_fingerprints[slot_idx] = entry_state.summary.observation_fingerprint();
            scope_generations[slot_idx] =
                self.scope_evidence_generation_for_scope(entry_state.scope_id);
        }
        let mut route_change_epochs = [0; MAX_LANES];
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
                continue;
            };
            let lane_idx = entry_state.lane_idx as usize;
            if lane_idx >= MAX_LANES {
                continue;
            }
            route_change_epochs[slot_idx] = self.ports[lane_idx]
                .as_ref()
                .map(Port::route_change_epoch)
                .unwrap_or(0);
        }
        FrontierObservationKey {
            active_entries: active_entry_indices,
            entry_summary_fingerprints,
            scope_generations,
            offer_lane_mask,
            binding_nonempty_mask: self.binding_inbox.nonempty_mask & offer_lane_mask,
            route_change_epochs,
        }
    }

    #[inline]
    fn cached_frontier_observed_entries(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        key: FrontierObservationKey,
    ) -> Option<ObservedEntrySet> {
        if use_root_observed_entries {
            let slot_idx = self.root_frontier_slot(current_parallel_root)?;
            let slot = self.root_frontier_state[slot_idx];
            if slot.observed_key != key {
                return None;
            }
            if slot.observed_entries.dynamic_controller_mask != 0 {
                return None;
            }
            return Some(slot.observed_entries);
        }
        if self.global_frontier_observed_key != key
            || self.global_frontier_observed.dynamic_controller_mask != 0
        {
            return None;
        }
        Some(self.global_frontier_observed)
    }

    #[inline]
    fn frontier_observation_cache(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> (FrontierObservationKey, ObservedEntrySet) {
        if use_root_observed_entries {
            let Some(slot_idx) = self.root_frontier_slot(current_parallel_root) else {
                return (FrontierObservationKey::EMPTY, ObservedEntrySet::EMPTY);
            };
            let slot = self.root_frontier_state[slot_idx];
            return (slot.observed_key, slot.observed_entries);
        }
        (
            self.global_frontier_observed_key,
            self.global_frontier_observed,
        )
    }

    #[inline]
    fn store_frontier_observation(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        observed_epoch: u32,
        key: FrontierObservationKey,
        observed_entries: ObservedEntrySet,
    ) {
        if use_root_observed_entries {
            let Some(slot_idx) = self.root_frontier_slot(current_parallel_root) else {
                return;
            };
            let slot = &mut self.root_frontier_state[slot_idx];
            slot.observed_epoch = observed_epoch;
            slot.observed_key = key;
            slot.observed_entries = observed_entries;
            return;
        }
        self.global_frontier_observed_epoch = observed_epoch;
        self.global_frontier_observed_key = key;
        self.global_frontier_observed = observed_entries;
    }

    #[inline]
    fn refresh_frontier_observation_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if self.refresh_structural_frontier_observation_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            cached_key,
        ) {
            return;
        }
        let observed_entries = self.refresh_frontier_observed_entries(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        );
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            observation_key,
            observed_entries,
        );
    }

    fn refresh_cached_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let Some(slot_idx) = active_entries.slot_for_entry(entry_idx) else {
            return false;
        };
        let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
            return false;
        };
        if entry_state.active_mask == 0 {
            return false;
        }
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (mut cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.active_entries != active_entries.entries
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
        {
            return false;
        }
        let mut expected_fingerprints = cached_key.entry_summary_fingerprints;
        expected_fingerprints[slot_idx] = observation_key.entry_summary_fingerprints[slot_idx];
        let mut expected_scope_generations = cached_key.scope_generations;
        expected_scope_generations[slot_idx] = observation_key.scope_generations[slot_idx];
        let mut expected_route_change_epochs = cached_key.route_change_epochs;
        expected_route_change_epochs[slot_idx] = observation_key.route_change_epochs[slot_idx];
        if expected_fingerprints != observation_key.entry_summary_fingerprints
            || expected_scope_generations != observation_key.scope_generations
            || expected_route_change_epochs != observation_key.route_change_epochs
        {
            return false;
        }
        let slot_unchanged = cached_key.entry_summary_fingerprints[slot_idx]
            == observation_key.entry_summary_fingerprints[slot_idx]
            && cached_key.scope_generations[slot_idx]
                == observation_key.scope_generations[slot_idx]
            && cached_key.route_change_epochs[slot_idx]
                == observation_key.route_change_epochs[slot_idx];
        if slot_unchanged {
            return true;
        }
        let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
        else {
            return false;
        };
        if !cached_observed_entries.replace_observation(entry_idx, observed) {
            return false;
        }
        cached_key.entry_summary_fingerprints[slot_idx] =
            observation_key.entry_summary_fingerprints[slot_idx];
        cached_key.scope_generations[slot_idx] = observation_key.scope_generations[slot_idx];
        cached_key.route_change_epochs[slot_idx] = observation_key.route_change_epochs[slot_idx];
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            cached_key,
            cached_observed_entries,
        );
        true
    }

    fn refresh_structural_frontier_observation_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        cached_key: FrontierObservationKey,
    ) -> bool {
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_key.active_entries);
        if active_len == cached_len {
            if let Some(entry_idx) =
                Self::structural_replaced_entry_idx(active_entries, cached_key.active_entries)
                && self.refresh_replaced_frontier_observation_entry(
                    current_parallel_root,
                    use_root_observed_entries,
                    entry_idx,
                )
            {
                return true;
            }
            if Self::structural_shifted_entry_idx(active_entries, cached_key.active_entries)
                .is_some()
            {
                let mut remaining_slots = active_entries.occupancy_mask();
                while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
                    let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                        continue;
                    };
                    if active_entries.entries[slot_idx] == cached_key.active_entries[slot_idx] {
                        continue;
                    }
                    if self.refresh_shifted_frontier_observation_entry(
                        current_parallel_root,
                        use_root_observed_entries,
                        entry_idx,
                    ) {
                        return true;
                    }
                }
            }
            if Self::same_active_entry_set(active_entries, cached_key.active_entries)
                && self.refresh_permuted_frontier_observation_entries(
                    current_parallel_root,
                    use_root_observed_entries,
                    active_entries,
                )
            {
                return true;
            }
            if self.refresh_multi_replaced_frontier_observation_entries(
                current_parallel_root,
                use_root_observed_entries,
                active_entries,
            ) {
                return true;
            }
            return false;
        }
        if active_len + 1 == cached_len
            && let Some(entry_idx) =
                Self::structural_removed_entry_idx(active_entries, cached_key.active_entries)
            && self.refresh_removed_frontier_observation_entry(
                current_parallel_root,
                use_root_observed_entries,
                entry_idx,
            )
        {
            return true;
        }
        if active_len == cached_len + 1
            && let Some(entry_idx) =
                Self::structural_inserted_entry_idx(active_entries, cached_key.active_entries)
            && self.refresh_inserted_frontier_observation_entry(
                current_parallel_root,
                use_root_observed_entries,
                entry_idx,
            )
        {
            return true;
        }
        false
    }

    fn refresh_permuted_frontier_observation_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
    ) -> bool {
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || !Self::same_active_entry_set(active_entries, cached_key.active_entries)
        {
            return false;
        }
        let mut refreshed = ObservedEntrySet::EMPTY;
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
                return false;
            };
            if entry_state.active_mask == 0 {
                return false;
            }
            let observed = self
                .cached_offer_entry_observed_state_for_rebuild(
                    entry_idx,
                    entry_state,
                    observation_key,
                    cached_key,
                    cached_observed_entries,
                )
                .or_else(|| self.offer_entry_observed_state_cached(entry_idx))
                .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx));
            let Some(observed) = observed else {
                return false;
            };
            let Some((observed_bit, _)) = refreshed.insert_entry(entry_idx) else {
                return false;
            };
            refreshed.observe(observed_bit, observed);
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            observation_key,
            refreshed,
        );
        true
    }

    fn refresh_multi_replaced_frontier_observation_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
    ) -> bool {
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
        {
            return false;
        }
        let active_len = active_entries.len as usize;
        if active_len == 0
            || active_len != Self::cached_active_entries_len(cached_key.active_entries)
            || Self::same_active_entry_set(active_entries, cached_key.active_entries)
        {
            return false;
        }
        let mut refreshed = ObservedEntrySet::EMPTY;
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut reused_cached = false;
        let mut recomputed = false;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
                return false;
            };
            if entry_state.active_mask == 0 {
                return false;
            }
            let observed = if let Some(observed) = self
                .cached_offer_entry_observed_state_for_rebuild(
                    entry_idx,
                    entry_state,
                    observation_key,
                    cached_key,
                    cached_observed_entries,
                ) {
                reused_cached = true;
                observed
            } else if let Some(observed) = self.offer_entry_observed_state_cached(entry_idx) {
                reused_cached = true;
                observed
            } else {
                recomputed = true;
                let Some(observed) =
                    self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
                else {
                    return false;
                };
                observed
            };
            let Some((observed_bit, _)) = refreshed.insert_entry(entry_idx) else {
                return false;
            };
            refreshed.observe(observed_bit, observed);
        }
        if !reused_cached || !recomputed {
            return false;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            observation_key,
            refreshed,
        );
        true
    }

    fn refresh_shifted_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
        {
            return false;
        }
        let Some((old_slot_idx, new_slot_idx)) =
            Self::cached_entry_slot_move(active_entries, cached_key.active_entries, entry_idx)
        else {
            return false;
        };
        if !cached_observed_entries.move_entry_slot(entry_idx, new_slot_idx) {
            return false;
        }
        let mut shifted_key = cached_key;
        Self::move_slot_in_array(
            &mut shifted_key.active_entries,
            active_entries.len as usize,
            old_slot_idx,
            new_slot_idx,
        );
        Self::move_slot_in_array(
            &mut shifted_key.entry_summary_fingerprints,
            active_entries.len as usize,
            old_slot_idx,
            new_slot_idx,
        );
        Self::move_slot_in_array(
            &mut shifted_key.scope_generations,
            active_entries.len as usize,
            old_slot_idx,
            new_slot_idx,
        );
        Self::move_slot_in_array(
            &mut shifted_key.route_change_epochs,
            active_entries.len as usize,
            old_slot_idx,
            new_slot_idx,
        );
        if shifted_key.active_entries != observation_key.active_entries {
            return false;
        }
        if shifted_key.entry_summary_fingerprints[new_slot_idx]
            != observation_key.entry_summary_fingerprints[new_slot_idx]
            || shifted_key.scope_generations[new_slot_idx]
                != observation_key.scope_generations[new_slot_idx]
            || shifted_key.route_change_epochs[new_slot_idx]
                != observation_key.route_change_epochs[new_slot_idx]
        {
            let Some(observed) = self
                .offer_entry_observed_state_cached(entry_idx)
                .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx))
            else {
                return false;
            };
            if !cached_observed_entries.replace_observation(entry_idx, observed) {
                return false;
            }
        }
        shifted_key.entry_summary_fingerprints[new_slot_idx] =
            observation_key.entry_summary_fingerprints[new_slot_idx];
        shifted_key.scope_generations[new_slot_idx] =
            observation_key.scope_generations[new_slot_idx];
        shifted_key.route_change_epochs[new_slot_idx] =
            observation_key.route_change_epochs[new_slot_idx];
        if shifted_key.entry_summary_fingerprints != observation_key.entry_summary_fingerprints
            || shifted_key.scope_generations != observation_key.scope_generations
            || shifted_key.route_change_epochs != observation_key.route_change_epochs
        {
            return false;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            shifted_key,
            cached_observed_entries,
        );
        true
    }

    fn refresh_inserted_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
            return false;
        };
        if entry_state.active_mask == 0 {
            return false;
        }
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(insert_slot_idx) =
            Self::cached_entry_slot_insert(active_entries, cached_key.active_entries, entry_idx)
        else {
            return false;
        };
        if ((cached_key.offer_lane_mask ^ observation_key.offer_lane_mask)
            & !entry_state.offer_lane_mask)
            != 0
            || ((cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask)
                & !entry_state.offer_lane_mask)
                != 0
        {
            return false;
        }
        let len = cached_observed_entries.len as usize;
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        let mut inserted_key = cached_key;
        Self::insert_slot_in_array(
            &mut inserted_key.active_entries,
            len,
            insert_slot_idx,
            entry,
        );
        Self::insert_slot_in_array(
            &mut inserted_key.entry_summary_fingerprints,
            len,
            insert_slot_idx,
            observation_key.entry_summary_fingerprints[insert_slot_idx],
        );
        Self::insert_slot_in_array(
            &mut inserted_key.scope_generations,
            len,
            insert_slot_idx,
            observation_key.scope_generations[insert_slot_idx],
        );
        Self::insert_slot_in_array(
            &mut inserted_key.route_change_epochs,
            len,
            insert_slot_idx,
            observation_key.route_change_epochs[insert_slot_idx],
        );
        inserted_key.offer_lane_mask = observation_key.offer_lane_mask;
        inserted_key.binding_nonempty_mask = observation_key.binding_nonempty_mask;
        if inserted_key.active_entries != observation_key.active_entries
            || inserted_key.entry_summary_fingerprints != observation_key.entry_summary_fingerprints
            || inserted_key.scope_generations != observation_key.scope_generations
            || inserted_key.route_change_epochs != observation_key.route_change_epochs
        {
            return false;
        }
        let Some(observed) = self
            .offer_entry_observed_state_cached(entry_idx)
            .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx))
        else {
            return false;
        };
        if !cached_observed_entries.insert_observation_at_slot(entry_idx, insert_slot_idx, observed)
        {
            return false;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            inserted_key,
            cached_observed_entries,
        );
        true
    }

    fn refresh_removed_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            return false;
        }
        let Some(removed_slot_idx) =
            Self::cached_entry_slot_remove(active_entries, cached_key.active_entries, entry_idx)
        else {
            return false;
        };
        let changed_lane_mask = (cached_key.offer_lane_mask ^ observation_key.offer_lane_mask)
            | (cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask);
        if changed_lane_mask != 0 {
            let slot_masks = self.frontier_observation_offer_lane_entry_slot_masks(
                current_parallel_root,
                use_root_observed_entries,
            );
            let mut remaining_lanes = changed_lane_mask;
            while let Some(lane_idx) = Self::next_lane_in_mask(&mut remaining_lanes) {
                if slot_masks[lane_idx] != 0 {
                    return false;
                }
            }
        }
        if !cached_observed_entries.remove_observation(entry_idx) {
            return false;
        }
        let cached_len = cached_key
            .active_entries
            .iter()
            .position(|entry| entry.is_max())
            .unwrap_or(MAX_LANES);
        let mut removed_key = cached_key;
        Self::remove_slot_from_array(
            &mut removed_key.active_entries,
            cached_len,
            removed_slot_idx,
            StateIndex::MAX,
        );
        Self::remove_slot_from_array(
            &mut removed_key.entry_summary_fingerprints,
            cached_len,
            removed_slot_idx,
            0,
        );
        Self::remove_slot_from_array(
            &mut removed_key.scope_generations,
            cached_len,
            removed_slot_idx,
            0,
        );
        Self::remove_slot_from_array(
            &mut removed_key.route_change_epochs,
            cached_len,
            removed_slot_idx,
            0,
        );
        removed_key.offer_lane_mask = observation_key.offer_lane_mask;
        removed_key.binding_nonempty_mask = observation_key.binding_nonempty_mask;
        if removed_key.active_entries != observation_key.active_entries
            || removed_key.entry_summary_fingerprints != observation_key.entry_summary_fingerprints
            || removed_key.scope_generations != observation_key.scope_generations
            || removed_key.route_change_epochs != observation_key.route_change_epochs
        {
            return false;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            removed_key,
            cached_observed_entries,
        );
        true
    }

    fn refresh_replaced_frontier_observation_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.offer_lane_mask != observation_key.offer_lane_mask
            || cached_key.binding_nonempty_mask != observation_key.binding_nonempty_mask
        {
            return false;
        }
        let Some((slot_idx, old_entry_idx, new_entry_idx)) =
            Self::cached_entry_slot_replace(active_entries, cached_key.active_entries, entry_idx)
        else {
            return false;
        };
        let Some(observed) = self
            .offer_entry_observed_state_cached(new_entry_idx)
            .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(new_entry_idx))
        else {
            return false;
        };
        if !cached_observed_entries.replace_entry_at_slot(old_entry_idx, new_entry_idx, observed) {
            return false;
        }
        let mut replaced_key = cached_key;
        replaced_key.active_entries[slot_idx] = observation_key.active_entries[slot_idx];
        replaced_key.entry_summary_fingerprints[slot_idx] =
            observation_key.entry_summary_fingerprints[slot_idx];
        replaced_key.scope_generations[slot_idx] = observation_key.scope_generations[slot_idx];
        replaced_key.route_change_epochs[slot_idx] = observation_key.route_change_epochs[slot_idx];
        if replaced_key.active_entries != observation_key.active_entries
            || replaced_key.entry_summary_fingerprints != observation_key.entry_summary_fingerprints
            || replaced_key.scope_generations != observation_key.scope_generations
            || replaced_key.route_change_epochs != observation_key.route_change_epochs
        {
            return false;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            replaced_key,
            cached_observed_entries,
        );
        true
    }

    fn cached_entry_slot_move(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
        entry_idx: usize,
    ) -> Option<(usize, usize)> {
        let new_slot_idx = active_entries.slot_for_entry(entry_idx)?;
        let len = active_entries.len as usize;
        let entry = checked_state_index(entry_idx)?;
        let mut old_slot_idx = 0usize;
        while old_slot_idx < len {
            if cached_entries[old_slot_idx] == entry {
                break;
            }
            old_slot_idx += 1;
        }
        if old_slot_idx >= len || old_slot_idx == new_slot_idx {
            return None;
        }
        let mut shifted = cached_entries;
        Self::move_slot_in_array(&mut shifted, len, old_slot_idx, new_slot_idx);
        if shifted[..len] != active_entries.entries[..len] {
            return None;
        }
        Some((old_slot_idx, new_slot_idx))
    }

    fn cached_entry_slot_insert(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
        entry_idx: usize,
    ) -> Option<usize> {
        let insert_slot_idx = active_entries.slot_for_entry(entry_idx)?;
        let len = active_entries.len as usize;
        if len == 0 {
            return None;
        }
        let cached_len = len - 1;
        let entry = checked_state_index(entry_idx)?;
        let mut slot_idx = 0usize;
        while slot_idx < cached_len {
            if cached_entries[slot_idx] == entry {
                return None;
            }
            slot_idx += 1;
        }
        let mut inserted = cached_entries;
        Self::insert_slot_in_array(&mut inserted, cached_len, insert_slot_idx, entry);
        if inserted[..len] != active_entries.entries[..len] {
            return None;
        }
        Some(insert_slot_idx)
    }

    fn cached_entry_slot_remove(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
        entry_idx: usize,
    ) -> Option<usize> {
        let len = active_entries.len as usize;
        if len >= MAX_LANES {
            return None;
        }
        let cached_len = len + 1;
        let entry = u16::try_from(entry_idx).ok()?;
        let mut removed_slot_idx = 0usize;
        while removed_slot_idx < cached_len {
            if cached_entries[removed_slot_idx] == entry {
                break;
            }
            removed_slot_idx += 1;
        }
        if removed_slot_idx >= cached_len {
            return None;
        }
        let mut removed = cached_entries;
        Self::remove_slot_from_array(&mut removed, cached_len, removed_slot_idx, StateIndex::MAX);
        if removed[..len] != active_entries.entries[..len] {
            return None;
        }
        Some(removed_slot_idx)
    }

    fn cached_entry_slot_replace(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
        entry_idx: usize,
    ) -> Option<(usize, usize, usize)> {
        let len = active_entries.len as usize;
        if len == 0 {
            return None;
        }
        let entry = u16::try_from(entry_idx).ok()?;
        let mut replaced_slot_idx = None;
        let mut slot_idx = 0usize;
        while slot_idx < len {
            let cached_entry = cached_entries[slot_idx];
            let active_entry = active_entries.entries[slot_idx];
            if cached_entry != active_entry {
                if replaced_slot_idx.is_some() {
                    return None;
                }
                if cached_entry != entry && active_entry != entry {
                    return None;
                }
                replaced_slot_idx = Some(slot_idx);
            }
            slot_idx += 1;
        }
        let slot_idx = replaced_slot_idx?;
        let old_entry_idx = state_index_to_usize(cached_entries[slot_idx]);
        let new_entry_idx = state_index_to_usize(active_entries.entries[slot_idx]);
        Some((slot_idx, old_entry_idx, new_entry_idx))
    }

    #[inline]
    fn cached_active_entries_len(cached_entries: [StateIndex; MAX_LANES]) -> usize {
        cached_entries
            .iter()
            .position(|entry| entry.is_max())
            .unwrap_or(MAX_LANES)
    }

    #[inline]
    fn cached_active_entries_contains(
        cached_entries: [StateIndex; MAX_LANES],
        entry_idx: usize,
    ) -> bool {
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        let len = Self::cached_active_entries_len(cached_entries);
        let mut slot_idx = 0usize;
        while slot_idx < len {
            if cached_entries[slot_idx] == entry {
                return true;
            }
            slot_idx += 1;
        }
        false
    }

    fn structural_inserted_entry_idx(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
    ) -> Option<usize> {
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_entries);
        if active_len != cached_len + 1 {
            return None;
        }
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut inserted = None;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return None;
            };
            if Self::cached_active_entries_contains(cached_entries, entry_idx) {
                continue;
            }
            if inserted.is_some() {
                return None;
            }
            inserted = Some(entry_idx);
        }
        inserted
    }

    fn structural_removed_entry_idx(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
    ) -> Option<usize> {
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_entries);
        if cached_len != active_len + 1 {
            return None;
        }
        let mut slot_idx = 0usize;
        let mut removed = None;
        while slot_idx < cached_len {
            let entry_idx = state_index_to_usize(cached_entries[slot_idx]);
            if active_entries.slot_for_entry(entry_idx).is_some() {
                slot_idx += 1;
                continue;
            }
            if removed.is_some() {
                return None;
            }
            removed = Some(entry_idx);
            slot_idx += 1;
        }
        removed
    }

    fn structural_replaced_entry_idx(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
    ) -> Option<usize> {
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_entries);
        if active_len != cached_len {
            return None;
        }
        let mut remaining_slots = active_entries.occupancy_mask();
        let mut inserted = None;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return None;
            };
            if Self::cached_active_entries_contains(cached_entries, entry_idx) {
                continue;
            }
            if inserted.is_some() {
                return None;
            }
            inserted = Some(entry_idx);
        }
        inserted
    }

    fn structural_shifted_entry_idx(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
    ) -> Option<usize> {
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_entries);
        if active_len != cached_len {
            return None;
        }
        let mut slot_idx = 0usize;
        let mut shifted = None;
        while slot_idx < active_len {
            let entry_idx = state_index_to_usize(active_entries.entries[slot_idx]);
            if !Self::cached_active_entries_contains(cached_entries, entry_idx) {
                return None;
            }
            if cached_entries[slot_idx] != active_entries.entries[slot_idx] {
                shifted.get_or_insert(entry_idx);
            }
            slot_idx += 1;
        }
        shifted
    }

    fn same_active_entry_set(
        active_entries: ActiveEntrySet,
        cached_entries: [StateIndex; MAX_LANES],
    ) -> bool {
        let active_len = active_entries.len as usize;
        let cached_len = Self::cached_active_entries_len(cached_entries);
        if active_len != cached_len {
            return false;
        }
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return false;
            };
            if !Self::cached_active_entries_contains(cached_entries, entry_idx) {
                return false;
            }
        }
        true
    }

    fn move_slot_in_array<V: Copy>(
        array: &mut [V; MAX_LANES],
        len: usize,
        old_slot_idx: usize,
        new_slot_idx: usize,
    ) {
        if old_slot_idx == new_slot_idx || old_slot_idx >= len || new_slot_idx >= len {
            return;
        }
        let value = array[old_slot_idx];
        if old_slot_idx < new_slot_idx {
            let mut slot_idx = old_slot_idx;
            while slot_idx < new_slot_idx {
                array[slot_idx] = array[slot_idx + 1];
                slot_idx += 1;
            }
        } else {
            let mut slot_idx = old_slot_idx;
            while slot_idx > new_slot_idx {
                array[slot_idx] = array[slot_idx - 1];
                slot_idx -= 1;
            }
        }
        array[new_slot_idx] = value;
    }

    fn insert_slot_in_array<V: Copy>(
        array: &mut [V; MAX_LANES],
        len: usize,
        slot_idx: usize,
        value: V,
    ) {
        if len >= MAX_LANES || slot_idx > len {
            return;
        }
        let mut shift_idx = len;
        while shift_idx > slot_idx {
            array[shift_idx] = array[shift_idx - 1];
            shift_idx -= 1;
        }
        array[slot_idx] = value;
    }

    fn remove_slot_from_array<V: Copy>(
        array: &mut [V; MAX_LANES],
        len: usize,
        slot_idx: usize,
        fill: V,
    ) {
        if len == 0 || slot_idx >= len {
            return;
        }
        let mut shift_idx = slot_idx;
        while shift_idx + 1 < len {
            array[shift_idx] = array[shift_idx + 1];
            shift_idx += 1;
        }
        array[len - 1] = fill;
    }

    fn refresh_cached_frontier_observation_scope_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        scope_id: ScopeId,
    ) {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let (mut cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.active_entries != active_entries.entries
        {
            return;
        }
        let offer_lane_mask =
            self.frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        if cached_key.offer_lane_mask != offer_lane_mask
            || cached_key.binding_nonempty_mask
                != (self.binding_inbox.nonempty_mask & offer_lane_mask)
        {
            return;
        }
        let scope_generation = self.scope_evidence_generation_for_scope(scope_id);
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
                continue;
            };
            if entry_state.active_mask == 0 || entry_state.scope_id != scope_id {
                continue;
            }
            if cached_key.scope_generations[slot_idx] == scope_generation {
                continue;
            }
            if cached_key.entry_summary_fingerprints[slot_idx]
                != entry_state.summary.observation_fingerprint()
            {
                return;
            }
            let lane_idx = entry_state.lane_idx as usize;
            if lane_idx >= MAX_LANES {
                return;
            }
            let route_change_epoch = self.ports[lane_idx]
                .as_ref()
                .map(Port::route_change_epoch)
                .unwrap_or(0);
            if cached_key.route_change_epochs[slot_idx] != route_change_epoch {
                return;
            }
            let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
            else {
                return;
            };
            if !cached_observed_entries.replace_observation(entry_idx, observed) {
                return;
            }
            cached_key.scope_generations[slot_idx] = scope_generation;
            patched = true;
        }
        if !patched {
            return;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            cached_key,
            cached_observed_entries,
        );
    }

    fn refresh_cached_frontier_observation_binding_lane_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        lane_idx: usize,
        previous_nonempty_mask: u8,
    ) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let lane_bit = 1u8 << lane_idx;
        if ((previous_nonempty_mask ^ self.binding_inbox.nonempty_mask) & lane_bit) == 0 {
            return;
        }
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let (mut cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.active_entries != active_entries.entries
        {
            return;
        }
        let offer_lane_mask =
            self.frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        if cached_key.offer_lane_mask != offer_lane_mask || (offer_lane_mask & lane_bit) == 0 {
            return;
        }
        let binding_nonempty_mask = self.binding_inbox.nonempty_mask & offer_lane_mask;
        if ((cached_key.binding_nonempty_mask ^ binding_nonempty_mask) & !lane_bit) != 0 {
            return;
        }
        let mut affected_slot_mask = self.frontier_observation_offer_lane_entry_slot_masks(
            current_parallel_root,
            use_root_observed_entries,
        )[lane_idx];
        if affected_slot_mask == 0 {
            return;
        }
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut affected_slot_mask) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return;
            };
            let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
                return;
            };
            if entry_state.active_mask == 0
                || cached_key.entry_summary_fingerprints[slot_idx]
                    != entry_state.summary.observation_fingerprint()
                || cached_key.scope_generations[slot_idx]
                    != self.scope_evidence_generation_for_scope(entry_state.scope_id)
            {
                return;
            }
            let representative_lane = entry_state.lane_idx as usize;
            if representative_lane >= MAX_LANES {
                return;
            }
            let route_change_epoch = self.ports[representative_lane]
                .as_ref()
                .map(Port::route_change_epoch)
                .unwrap_or(0);
            if cached_key.route_change_epochs[slot_idx] != route_change_epoch {
                return;
            }
            let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
            else {
                return;
            };
            if !cached_observed_entries.replace_observation(entry_idx, observed) {
                return;
            }
        }
        cached_key.binding_nonempty_mask = binding_nonempty_mask;
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            cached_key,
            cached_observed_entries,
        );
    }

    fn refresh_cached_frontier_observation_route_lane_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        lane_idx: usize,
        previous_change_epoch: u32,
    ) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let route_change_epoch = self
            .ports
            .get(lane_idx)
            .and_then(|port| port.as_ref())
            .map(Port::route_change_epoch)
            .unwrap_or(0);
        if route_change_epoch == previous_change_epoch {
            return;
        }
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let (mut cached_key, mut cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.active_entries != active_entries.entries
        {
            return;
        }
        let offer_lane_mask =
            self.frontier_observation_lane_mask(current_parallel_root, use_root_observed_entries);
        if cached_key.offer_lane_mask != offer_lane_mask
            || cached_key.binding_nonempty_mask
                != (self.binding_inbox.nonempty_mask & offer_lane_mask)
        {
            return;
        }
        let mut remaining_entries = active_entries.occupancy_mask();
        let mut patched = false;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
                continue;
            };
            if entry_state.active_mask == 0 || entry_state.lane_idx as usize != lane_idx {
                continue;
            }
            if cached_key.entry_summary_fingerprints[slot_idx]
                != entry_state.summary.observation_fingerprint()
                || cached_key.scope_generations[slot_idx]
                    != self.scope_evidence_generation_for_scope(entry_state.scope_id)
            {
                return;
            }
            if cached_key.route_change_epochs[slot_idx] == route_change_epoch {
                continue;
            }
            let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
            else {
                return;
            };
            if !cached_observed_entries.replace_observation(entry_idx, observed) {
                return;
            }
            cached_key.route_change_epochs[slot_idx] = route_change_epoch;
            patched = true;
        }
        if !patched {
            return;
        }
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            cached_key,
            cached_observed_entries,
        );
    }

    #[inline]
    fn refresh_frontier_observation_cache_for_scope(&mut self, scope_id: ScopeId) {
        let mut active_entries = self.global_active_entries.occupancy_mask();
        let mut roots = [ScopeId::none(); MAX_LANES];
        let mut root_len = 0usize;
        let mut matches_scope = false;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut active_entries) {
            let Some(entry_idx) = self.global_active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
                continue;
            };
            if entry_state.active_mask == 0 || entry_state.scope_id != scope_id {
                continue;
            }
            matches_scope = true;
            if entry_state.parallel_root.is_none() {
                continue;
            }
            let mut seen_root = false;
            let mut idx = 0usize;
            while idx < root_len {
                if roots[idx] == entry_state.parallel_root {
                    seen_root = true;
                    break;
                }
                idx += 1;
            }
            if !seen_root && root_len < MAX_LANES {
                roots[root_len] = entry_state.parallel_root;
                root_len += 1;
            }
        }
        if !matches_scope {
            return;
        }
        self.refresh_cached_frontier_observation_scope_entries(ScopeId::none(), false, scope_id);
        let mut idx = 0usize;
        while idx < root_len {
            self.refresh_cached_frontier_observation_scope_entries(roots[idx], true, scope_id);
            idx += 1;
        }
    }

    #[inline]
    fn refresh_frontier_observation_cache_for_binding_lane(
        &mut self,
        lane_idx: usize,
        previous_nonempty_mask: u8,
    ) {
        if lane_idx >= MAX_LANES {
            return;
        }
        self.refresh_cached_frontier_observation_binding_lane_entries(
            ScopeId::none(),
            false,
            lane_idx,
            previous_nonempty_mask,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.root_frontier_len as usize {
            if self.root_frontier_state[slot_idx].offer_lane_entry_slot_masks[lane_idx] != 0 {
                let root = self.root_frontier_state[slot_idx].root;
                self.refresh_cached_frontier_observation_binding_lane_entries(
                    root,
                    true,
                    lane_idx,
                    previous_nonempty_mask,
                );
            }
            slot_idx += 1;
        }
    }

    #[inline]
    fn refresh_frontier_observation_cache_for_route_lane(
        &mut self,
        lane_idx: usize,
        previous_change_epoch: u32,
    ) {
        if lane_idx >= MAX_LANES {
            return;
        }
        self.refresh_cached_frontier_observation_route_lane_entries(
            ScopeId::none(),
            false,
            lane_idx,
            previous_change_epoch,
        );
        let mut slot_idx = 0usize;
        while slot_idx < self.root_frontier_len as usize {
            let root = self.root_frontier_state[slot_idx].root;
            self.refresh_cached_frontier_observation_route_lane_entries(
                root,
                true,
                lane_idx,
                previous_change_epoch,
            );
            slot_idx += 1;
        }
    }

    #[inline]
    fn refresh_frontier_observation_cache_from_cached_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
    ) -> bool {
        let active_entries = if use_root_observed_entries {
            self.root_frontier_active_entries(current_parallel_root)
        } else {
            self.global_active_entries
        };
        let observation_key =
            self.frontier_observation_key(current_parallel_root, use_root_observed_entries);
        let (cached_key, cached_observed_entries) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        let Some(observed_entries) = self.refresh_frontier_observed_entries_from_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        ) else {
            return false;
        };
        let observed_epoch = self.next_frontier_observation_epoch();
        self.store_frontier_observation(
            current_parallel_root,
            use_root_observed_entries,
            observed_epoch,
            observation_key,
            observed_entries,
        );
        true
    }

    #[inline]
    fn refresh_frontier_observation_cache_for_entry(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        entry_idx: usize,
    ) {
        let (cached_key, _) =
            self.frontier_observation_cache(current_parallel_root, use_root_observed_entries);
        if cached_key == FrontierObservationKey::EMPTY {
            self.refresh_frontier_observation_cache(
                current_parallel_root,
                use_root_observed_entries,
            );
            return;
        }
        if self.refresh_cached_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_frontier_observation_cache_from_cached_entries(
            current_parallel_root,
            use_root_observed_entries,
        ) || self.refresh_replaced_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_removed_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_inserted_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) || self.refresh_shifted_frontier_observation_entry(
            current_parallel_root,
            use_root_observed_entries,
            entry_idx,
        ) {
            return;
        }
        self.refresh_frontier_observation_cache(current_parallel_root, use_root_observed_entries);
    }

    #[inline]
    fn refresh_frontier_observation_caches_for_entry(
        &mut self,
        entry_idx: usize,
        previous_root: ScopeId,
        current_root: ScopeId,
    ) {
        self.refresh_frontier_observation_cache_for_entry(ScopeId::none(), false, entry_idx);
        if !previous_root.is_none() {
            self.refresh_frontier_observation_cache_for_entry(previous_root, true, entry_idx);
        }
        if !current_root.is_none() && current_root != previous_root {
            self.refresh_frontier_observation_cache_for_entry(current_root, true, entry_idx);
        }
    }

    #[inline]
    fn recompute_offer_entry_observed_state_non_consuming(
        &mut self,
        entry_idx: usize,
    ) -> Option<OfferEntryObservedState> {
        let entry_state = self.offer_entry_state.get(entry_idx).copied()?;
        if entry_state.active_mask == 0 {
            return None;
        }
        let (binding_ready, has_ack, has_ready_arm_evidence) =
            self.preview_offer_entry_evidence_non_consuming(entry_state);
        let (observed, _) = self.offer_entry_candidate_from_observation(
            entry_idx,
            entry_state,
            binding_ready,
            has_ack,
            has_ready_arm_evidence,
        );
        if let Some(state) = self.offer_entry_state.get_mut(entry_idx) {
            state.observed = observed;
        }
        Some(observed)
    }

    #[inline]
    fn offer_entry_observed_state_cached(
        &self,
        entry_idx: usize,
    ) -> Option<OfferEntryObservedState> {
        let state = self.offer_entry_state.get(entry_idx)?;
        if state.active_mask == 0 || state.observed.scope_id != state.scope_id {
            return None;
        }
        Some(state.observed)
    }

    fn cached_frontier_changed_entry_slot_mask(
        &self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
    ) -> Option<u8> {
        if cached_key == FrontierObservationKey::EMPTY
            || cached_key.active_entries != observation_key.active_entries
        {
            return None;
        }
        let mut changed_slot_mask = 0u8;
        let mut slot_idx = 0usize;
        while slot_idx < MAX_LANES {
            if observation_key.active_entries[slot_idx].is_max() {
                break;
            }
            if cached_key.entry_summary_fingerprints[slot_idx]
                != observation_key.entry_summary_fingerprints[slot_idx]
                || cached_key.scope_generations[slot_idx]
                    != observation_key.scope_generations[slot_idx]
                || cached_key.route_change_epochs[slot_idx]
                    != observation_key.route_change_epochs[slot_idx]
            {
                changed_slot_mask |= 1u8 << slot_idx;
            }
            slot_idx += 1;
        }
        let mut changed_lane_mask = cached_key.offer_lane_mask ^ observation_key.offer_lane_mask;
        changed_lane_mask |=
            cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask;
        if changed_lane_mask != 0 {
            let slot_masks = self.frontier_observation_offer_lane_entry_slot_masks(
                current_parallel_root,
                use_root_observed_entries,
            );
            let mut remaining_lanes = changed_lane_mask;
            while let Some(lane_idx) = Self::next_lane_in_mask(&mut remaining_lanes) {
                changed_slot_mask |= slot_masks[lane_idx];
            }
        }
        Some(changed_slot_mask)
    }

    fn refresh_frontier_observed_entries_from_cache(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<ObservedEntrySet> {
        let mut changed_slot_mask = self.cached_frontier_changed_entry_slot_mask(
            current_parallel_root,
            use_root_observed_entries,
            observation_key,
            cached_key,
        )?;
        if changed_slot_mask == 0 {
            return Some(cached_observed_entries);
        }
        let mut refreshed = cached_observed_entries;
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut changed_slot_mask) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                return None;
            };
            let Some(observed) = self.recompute_offer_entry_observed_state_non_consuming(entry_idx)
            else {
                return None;
            };
            if !refreshed.replace_observation(entry_idx, observed) {
                return None;
            }
        }
        Some(refreshed)
    }

    fn compose_frontier_observed_entries(
        &mut self,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> ObservedEntrySet {
        let mut composed = ObservedEntrySet::EMPTY;
        let mut remaining_slots = active_entries.occupancy_mask();
        while let Some(_slot_idx) = Self::next_lane_in_mask(&mut remaining_slots) {
            let Some(entry_idx) = active_entries.entry_at(_slot_idx) else {
                continue;
            };
            let Some(entry_state) = self.offer_entry_state.get(entry_idx).copied() else {
                continue;
            };
            if entry_state.active_mask == 0 {
                continue;
            }
            let observed = self
                .cached_offer_entry_observed_state_for_rebuild(
                    entry_idx,
                    entry_state,
                    observation_key,
                    cached_key,
                    cached_observed_entries,
                )
                .or_else(|| self.offer_entry_observed_state_cached(entry_idx))
                .or_else(|| self.recompute_offer_entry_observed_state_non_consuming(entry_idx));
            let Some(observed) = observed else {
                continue;
            };
            let Some((observed_bit, _)) = composed.insert_entry(entry_idx) else {
                continue;
            };
            composed.observe(observed_bit, observed);
        }
        composed
    }

    #[cfg(test)]
    fn patch_frontier_observed_entries_from_cached_structure(
        &mut self,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<ObservedEntrySet> {
        Some(self.compose_frontier_observed_entries(
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        ))
    }

    #[inline]
    fn frontier_observation_entry_reusable(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
        cached_slot_idx: usize,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
    ) -> bool {
        let Some(entry) = checked_state_index(entry_idx) else {
            return false;
        };
        if cached_slot_idx >= MAX_LANES
            || cached_key.active_entries[cached_slot_idx] != entry
            || cached_key.active_entries[cached_slot_idx].is_max()
            || cached_key.entry_summary_fingerprints[cached_slot_idx]
                != entry_state.summary.observation_fingerprint()
            || cached_key.scope_generations[cached_slot_idx]
                != self.scope_evidence_generation_for_scope(entry_state.scope_id)
        {
            return false;
        }
        let changed_binding_mask =
            cached_key.binding_nonempty_mask ^ observation_key.binding_nonempty_mask;
        if (changed_binding_mask & entry_state.offer_lane_mask) != 0 {
            return false;
        }
        if cached_key.route_change_epochs[cached_slot_idx]
            != observation_key.route_change_epochs[cached_slot_idx]
        {
            return false;
        }
        true
    }

    #[inline]
    fn cached_offer_entry_observed_state_for_rebuild(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> Option<OfferEntryObservedState> {
        if cached_key == FrontierObservationKey::EMPTY {
            return None;
        }
        let cached_bit = cached_observed_entries.entry_bit(entry_idx);
        if cached_bit == 0 || (cached_observed_entries.dynamic_controller_mask & cached_bit) != 0 {
            return None;
        }
        let cached_slot_idx = cached_bit.trailing_zeros() as usize;
        if !self.frontier_observation_entry_reusable(
            entry_idx,
            entry_state,
            cached_slot_idx,
            observation_key,
            cached_key,
        ) {
            return None;
        }
        Some(cached_offer_entry_observed_state(
            entry_state.scope_id,
            entry_state.summary,
            cached_observed_entries,
            cached_bit,
        ))
    }

    #[inline]
    fn refresh_frontier_observed_entries(
        &mut self,
        current_parallel_root: ScopeId,
        use_root_observed_entries: bool,
        active_entries: ActiveEntrySet,
        observation_key: FrontierObservationKey,
        cached_key: FrontierObservationKey,
        cached_observed_entries: ObservedEntrySet,
    ) -> ObservedEntrySet {
        if let Some(refreshed) = self.refresh_frontier_observed_entries_from_cache(
            current_parallel_root,
            use_root_observed_entries,
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        ) {
            return refreshed;
        }
        self.compose_frontier_observed_entries(
            active_entries,
            observation_key,
            cached_key,
            cached_observed_entries,
        )
    }

    #[inline]
    fn observed_reentry_entry_idx(
        &self,
        observed_entries: ObservedEntrySet,
        current_idx: usize,
        ready_only: bool,
    ) -> Option<usize> {
        let mut mask = if ready_only {
            observed_entries.ready_mask
        } else {
            observed_entries.occupancy_mask()
        };
        mask &= !observed_entries.entry_bit(current_idx);
        observed_entries.first_entry_idx(mask)
    }

    #[inline]
    fn offer_entry_selection_meta(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<CurrentScopeSelectionMeta> {
        self.offer_entry_state
            .get(entry_idx)
            .filter(|state| state.active_mask != 0 && state.scope_id == scope_id)
            .map(|state| state.selection_meta)
    }

    #[inline]
    fn offer_entry_label_meta(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<ScopeLabelMeta> {
        self.offer_entry_state
            .get(entry_idx)
            .filter(|state| state.active_mask != 0 && state.scope_id == scope_id)
            .map(|state| state.label_meta)
    }

    #[inline]
    fn offer_entry_materialization_meta(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<ScopeArmMaterializationMeta> {
        self.offer_entry_state
            .get(entry_idx)
            .filter(|state| state.active_mask != 0 && state.scope_id == scope_id)
            .map(|state| state.materialization_meta)
    }

    #[inline]
    fn offer_entry_lane_state(
        &self,
        scope_id: ScopeId,
        entry_idx: usize,
    ) -> Option<LaneOfferState> {
        let state = self.offer_entry_state.get(entry_idx)?;
        if state.active_mask == 0 || state.scope_id != scope_id {
            return None;
        }
        let lane_idx = state.lane_idx as usize;
        if lane_idx >= MAX_LANES || (state.active_mask & (1u8 << lane_idx)) == 0 {
            return None;
        }
        let info = self.lane_offer_state[lane_idx];
        (info.scope == scope_id && state_index_to_usize(info.entry) == entry_idx).then_some(info)
    }

    #[inline]
    fn offer_entry_parallel_root(&self, entry_idx: usize) -> Option<ScopeId> {
        self.offer_entry_state
            .get(entry_idx)
            .map(|state| state.parallel_root)
            .filter(|root| !root.is_none())
    }

    fn compute_offer_entry_selection_meta(
        &self,
        scope_id: ScopeId,
        info: LaneOfferState,
        has_offer_lanes: bool,
    ) -> CurrentScopeSelectionMeta {
        let Some(region) = self.cursor.scope_region_by_id(scope_id) else {
            return CurrentScopeSelectionMeta::EMPTY;
        };
        if region.kind != ScopeKind::Route {
            return CurrentScopeSelectionMeta::EMPTY;
        }
        let mut flags = CurrentScopeSelectionMeta::FLAG_ROUTE_ENTRY;
        if has_offer_lanes {
            flags |= CurrentScopeSelectionMeta::FLAG_HAS_OFFER_LANES;
        }
        if info.is_controller() {
            flags |= CurrentScopeSelectionMeta::FLAG_CONTROLLER;
        }
        CurrentScopeSelectionMeta { flags }
    }

    fn compute_scope_arm_materialization_meta(
        &self,
        scope_id: ScopeId,
    ) -> ScopeArmMaterializationMeta {
        let mut meta = ScopeArmMaterializationMeta {
            arm_count: self.cursor.route_scope_arm_count(scope_id).unwrap_or(0),
            ..ScopeArmMaterializationMeta::EMPTY
        };
        let mut arm = 0u8;
        while arm <= 1 {
            let arm_idx = arm as usize;
            if let Some((entry, label)) = self.cursor.controller_arm_entry_by_arm(scope_id, arm) {
                meta.controller_arm_entry[arm_idx] = entry;
                meta.controller_arm_label[arm_idx] = label;
                let target_cursor = self.cursor.with_index(state_index_to_usize(entry));
                if let Some(recv_meta) = target_cursor.try_recv_meta() {
                    meta.controller_recv_mask |= 1u8 << arm_idx;
                    if recv_meta.peer != ROLE {
                        meta.controller_cross_role_recv_mask |= 1u8 << arm_idx;
                    }
                    meta.record_binding_demux_lane(arm, recv_meta.lane);
                }
            }
            if let Some(entry) = self.cursor.route_scope_arm_recv_index(scope_id, arm)
                && let Some(entry) = checked_state_index(entry)
            {
                meta.recv_entry[arm_idx] = entry;
                if let Some(recv_meta) = self
                    .cursor
                    .with_index(state_index_to_usize(entry))
                    .try_recv_meta()
                {
                    meta.record_binding_demux_lane(arm, recv_meta.lane);
                }
            }
            if let Some(PassiveArmNavigation::WithinArm { entry }) = self
                .cursor
                .follow_passive_observer_arm_for_scope(scope_id, arm)
            {
                meta.passive_arm_entry[arm_idx] = entry;
            }
            if let Some(scope) = self.cursor.passive_arm_scope_by_arm(scope_id, arm) {
                meta.passive_arm_scope[arm_idx] = scope;
            }
            if arm == 1 {
                break;
            }
            arm += 1;
        }
        let mut dispatch_idx = 0usize;
        while let Some(dispatch) = self
            .cursor
            .route_scope_first_recv_dispatch_entry(scope_id, dispatch_idx)
        {
            let (_label, dispatch_arm, target) = dispatch;
            meta.first_recv_dispatch[dispatch_idx] = dispatch;
            let target_cursor = self.cursor.with_index(state_index_to_usize(target));
            if let Some(recv_meta) = target_cursor.try_recv_meta() {
                meta.record_binding_demux_lane(dispatch_arm, recv_meta.lane);
            }
            dispatch_idx += 1;
        }
        meta.first_recv_len = dispatch_idx as u8;
        meta
    }

    fn next_active_frontier_entry(
        &self,
        active_entries: ActiveEntrySet,
        remaining_mask: &mut u8,
    ) -> Option<usize> {
        while *remaining_mask != 0 {
            let slot_idx = remaining_mask.trailing_zeros() as usize;
            *remaining_mask &= !(1u8 << slot_idx);
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(state) = self.offer_entry_state.get(entry_idx).copied() else {
                continue;
            };
            if state.active_mask != 0 && state.lane_idx != u8::MAX {
                return Some(entry_idx);
            }
        }
        None
    }

    #[inline]
    fn offer_entry_frontier(&self, entry_state: OfferEntryState) -> FrontierKind {
        entry_state.frontier
    }

    #[inline]
    fn preview_offer_entry_evidence_non_consuming(
        &mut self,
        entry_state: OfferEntryState,
    ) -> (bool, bool, bool) {
        let binding_ready = self
            .binding_inbox
            .has_buffered_for_lane_mask(entry_state.offer_lane_mask);
        let mut has_ack = self.peek_scope_ack(entry_state.scope_id).is_some();
        let lane_idx = entry_state.lane_idx as usize;
        let pending_ack_mask = if lane_idx < MAX_LANES {
            self.pending_scope_ack_lane_mask(
                lane_idx,
                entry_state.scope_id,
                entry_state.offer_lane_mask,
            )
        } else {
            0
        };
        if !has_ack {
            has_ack = pending_ack_mask != 0;
        }
        let has_ready_arm_evidence = self.scope_has_ready_arm_evidence(entry_state.scope_id);
        (binding_ready, has_ack, has_ready_arm_evidence)
    }

    #[inline]
    fn offer_entry_candidate_from_observation(
        &self,
        entry_idx: usize,
        entry_state: OfferEntryState,
        binding_ready: bool,
        has_ack: bool,
        has_ready_arm_evidence: bool,
    ) -> (OfferEntryObservedState, FrontierCandidate) {
        let loop_meta = if entry_state.lane_idx as usize >= MAX_LANES {
            entry_state.label_meta.loop_meta()
        } else {
            self.lane_offer_state[entry_state.lane_idx as usize].loop_meta
        };
        let ack_is_progress = Self::ack_is_progress_evidence(loop_meta, has_ack);
        let observed = offer_entry_observed_state(
            entry_state.scope_id,
            entry_state.summary,
            has_ready_arm_evidence,
            ack_is_progress,
            binding_ready,
        );
        let candidate = offer_entry_frontier_candidate(
            entry_idx,
            entry_state.parallel_root,
            self.offer_entry_frontier(entry_state),
            observed,
        );
        (observed, candidate)
    }

    fn scan_offer_entry_candidate_non_consuming(
        &mut self,
        entry_idx: usize,
    ) -> Option<FrontierCandidate> {
        let entry_state = self.offer_entry_state.get(entry_idx).copied()?;
        if entry_state.active_mask == 0 {
            return None;
        }
        let (binding_ready, has_ack, has_ready_arm_evidence) =
            self.preview_offer_entry_evidence_non_consuming(entry_state);
        let (_observed, candidate) = self.offer_entry_candidate_from_observation(
            entry_idx,
            entry_state,
            binding_ready,
            has_ack,
            has_ready_arm_evidence,
        );
        Some(candidate)
    }

    fn next_frontier_observation_epoch(&mut self) -> u32 {
        let next = self.frontier_observation_epoch.wrapping_add(1);
        if next == 0 {
            self.frontier_observation_epoch = 1;
            self.global_frontier_observed_epoch = 0;
            self.global_frontier_observed_key = FrontierObservationKey::EMPTY;
            self.global_frontier_observed = ObservedEntrySet::EMPTY;
            let len = self.root_frontier_len as usize;
            let mut idx = 0usize;
            while idx < len {
                self.root_frontier_state[idx].observed_epoch = 0;
                self.root_frontier_state[idx].observed_key = FrontierObservationKey::EMPTY;
                self.root_frontier_state[idx].observed_entries = ObservedEntrySet::EMPTY;
                idx += 1;
            }
            1
        } else {
            self.frontier_observation_epoch = next;
            next
        }
    }

    #[inline]
    fn global_frontier_observed_entries(&self) -> ObservedEntrySet {
        self.global_frontier_observed
    }

    fn root_frontier_progress_sibling_exists(
        &self,
        root: ScopeId,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        loop_controller_without_evidence: bool,
    ) -> bool {
        self.observed_frontier_progress_sibling_exists(
            self.root_frontier_observed_entries(root),
            current_entry_idx,
            current_frontier,
            loop_controller_without_evidence,
        )
    }

    fn global_frontier_progress_sibling_exists(
        &self,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        loop_controller_without_evidence: bool,
    ) -> bool {
        self.observed_frontier_progress_sibling_exists(
            self.global_frontier_observed_entries(),
            current_entry_idx,
            current_frontier,
            loop_controller_without_evidence,
        )
    }

    #[inline]
    fn observed_frontier_progress_sibling_exists(
        &self,
        observed_entries: ObservedEntrySet,
        current_entry_idx: usize,
        current_frontier: FrontierKind,
        loop_controller_without_evidence: bool,
    ) -> bool {
        let mut sibling_mask = observed_entries.progress_mask;
        sibling_mask &= !observed_entries.entry_bit(current_entry_idx);
        if !loop_controller_without_evidence {
            sibling_mask &= observed_entries.frontier_mask(current_frontier);
        }
        sibling_mask != 0
    }

    fn remove_root_frontier_slot(&mut self, slot_idx: usize) {
        let len = self.root_frontier_len as usize;
        if slot_idx >= len {
            return;
        }
        let removed_root = self.root_frontier_state[slot_idx].root;
        if let Some(ordinal) = Self::scope_ordinal_index(removed_root) {
            self.root_frontier_slot_by_ordinal[ordinal] = u8::MAX;
        }
        let last = len - 1;
        let mut idx = slot_idx;
        while idx < last {
            let moved = self.root_frontier_state[idx + 1];
            self.root_frontier_state[idx] = moved;
            if let Some(ordinal) = Self::scope_ordinal_index(moved.root) {
                self.root_frontier_slot_by_ordinal[ordinal] = idx as u8;
            }
            idx += 1;
        }
        self.root_frontier_state[last] = RootFrontierState::EMPTY;
        self.root_frontier_len = last as u8;
    }

    #[inline]
    fn root_frontier_observed_entries(&self, root: ScopeId) -> ObservedEntrySet {
        self.root_frontier_slot(root)
            .map(|slot_idx| self.root_frontier_state[slot_idx].observed_entries)
            .unwrap_or(ObservedEntrySet::EMPTY)
    }

    fn attach_offer_entry_to_root_frontier(
        &mut self,
        entry_idx: usize,
        root: ScopeId,
        lane_idx: u8,
    ) {
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        self.root_frontier_state[slot_idx]
            .active_entries
            .insert_entry(entry_idx, lane_idx);
    }

    fn detach_offer_entry_from_root_frontier(&mut self, entry_idx: usize, root: ScopeId) {
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        self.root_frontier_state[slot_idx]
            .active_entries
            .remove_entry(entry_idx);
    }

    fn offer_lane_mask_for_active_entries(&self, active_entries: ActiveEntrySet) -> u8 {
        let mut offer_lane_mask = 0u8;
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(state) = self.offer_entry_state.get(entry_idx).copied() else {
                continue;
            };
            if state.active_mask == 0 {
                continue;
            }
            offer_lane_mask |= state.offer_lane_mask;
        }
        offer_lane_mask
    }

    fn offer_lane_entry_slot_masks_for_active_entries(
        &self,
        active_entries: ActiveEntrySet,
    ) -> [u8; MAX_LANES] {
        let mut slot_masks = [0u8; MAX_LANES];
        let mut remaining_entries = active_entries.occupancy_mask();
        while let Some(slot_idx) = Self::next_lane_in_mask(&mut remaining_entries) {
            let Some(entry_idx) = active_entries.entry_at(slot_idx) else {
                continue;
            };
            let Some(state) = self.offer_entry_state.get(entry_idx).copied() else {
                continue;
            };
            if state.active_mask == 0 {
                continue;
            }
            let mut lane_mask = state.offer_lane_mask;
            while let Some(lane_idx) = Self::next_lane_in_mask(&mut lane_mask) {
                slot_masks[lane_idx] |= 1u8 << slot_idx;
            }
        }
        slot_masks
    }

    fn recompute_global_offer_lane_mask(&mut self) {
        self.global_offer_lane_mask =
            self.offer_lane_mask_for_active_entries(self.global_active_entries);
    }

    fn recompute_global_offer_lane_entry_slot_masks(&mut self) {
        #[cfg(feature = "std")]
        {
            *self.global_offer_lane_entry_slot_masks =
                self.offer_lane_entry_slot_masks_for_active_entries(self.global_active_entries);
        }
        #[cfg(not(feature = "std"))]
        {
            self.global_offer_lane_entry_slot_masks =
                self.offer_lane_entry_slot_masks_for_active_entries(self.global_active_entries);
        }
    }

    fn recompute_root_frontier_offer_lane_mask(&mut self, root: ScopeId) {
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        let active_entries = self.root_frontier_state[slot_idx].active_entries;
        self.root_frontier_state[slot_idx].offer_lane_mask =
            self.offer_lane_mask_for_active_entries(active_entries);
    }

    fn recompute_root_frontier_offer_lane_entry_slot_masks(&mut self, root: ScopeId) {
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        let active_entries = self.root_frontier_state[slot_idx].active_entries;
        self.root_frontier_state[slot_idx].offer_lane_entry_slot_masks =
            self.offer_lane_entry_slot_masks_for_active_entries(active_entries);
    }

    fn detach_lane_from_root_frontier(&mut self, lane_idx: usize, info: LaneOfferState) {
        let root = info.parallel_root.canonical();
        if root.is_none() {
            return;
        }
        let Some(slot_idx) = self.root_frontier_slot(root) else {
            return;
        };
        let bit = 1u8 << lane_idx;
        let slot = &mut self.root_frontier_state[slot_idx];
        slot.active_mask &= !bit;
        slot.controller_mask &= !bit;
        slot.dynamic_controller_mask &= !bit;
        if slot.active_mask == 0 {
            self.remove_root_frontier_slot(slot_idx);
        }
    }

    fn attach_lane_to_root_frontier(&mut self, lane_idx: usize, info: LaneOfferState) {
        let root = info.parallel_root.canonical();
        if root.is_none() {
            return;
        }
        let slot_idx = if let Some(slot_idx) = self.root_frontier_slot(root) {
            slot_idx
        } else {
            let slot_idx = self.root_frontier_len as usize;
            if slot_idx >= MAX_LANES {
                return;
            }
            self.root_frontier_state[slot_idx] = RootFrontierState {
                root,
                ..RootFrontierState::EMPTY
            };
            if let Some(ordinal) = Self::scope_ordinal_index(root) {
                self.root_frontier_slot_by_ordinal[ordinal] = slot_idx as u8;
            }
            self.root_frontier_len += 1;
            slot_idx
        };
        let bit = 1u8 << lane_idx;
        let slot = &mut self.root_frontier_state[slot_idx];
        slot.active_mask |= bit;
        if info.is_controller() {
            slot.controller_mask |= bit;
        }
        if info.is_dynamic() {
            slot.dynamic_controller_mask |= bit;
        }
    }

    fn compute_offer_entry_static_summary(
        &self,
        active_mask: u8,
        entry_idx: usize,
    ) -> OfferEntryStaticSummary {
        let mut summary = OfferEntryStaticSummary::EMPTY;
        let mut lane_mask = active_mask;
        while lane_mask != 0 {
            let lane_idx = lane_mask.trailing_zeros() as usize;
            lane_mask &= !(1u8 << lane_idx);
            let info = self.lane_offer_state[lane_idx];
            if info.scope.is_none() || state_index_to_usize(info.entry) != entry_idx {
                continue;
            }
            summary.observe_lane(info);
        }
        summary
    }

    fn refresh_offer_entry_state(&mut self, entry_idx: usize) {
        let Some(state) = self.offer_entry_state.get(entry_idx).copied() else {
            return;
        };
        let previous_root = state.parallel_root;
        if state.active_mask == 0 {
            self.offer_entry_state[entry_idx] = OfferEntryState::EMPTY;
            self.recompute_global_offer_lane_mask();
            self.recompute_global_offer_lane_entry_slot_masks();
            self.recompute_root_frontier_offer_lane_mask(previous_root);
            self.recompute_root_frontier_offer_lane_entry_slot_masks(previous_root);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        }
        self.detach_offer_entry_from_root_frontier(entry_idx, state.parallel_root);
        self.global_active_entries.remove_entry(entry_idx);
        let lane_idx = state.active_mask.trailing_zeros() as usize;
        let info = self.lane_offer_state[lane_idx];
        if info.scope.is_none() {
            self.offer_entry_state[entry_idx] = OfferEntryState::EMPTY;
            self.recompute_global_offer_lane_mask();
            self.recompute_global_offer_lane_entry_slot_masks();
            self.recompute_root_frontier_offer_lane_mask(previous_root);
            self.recompute_root_frontier_offer_lane_entry_slot_masks(previous_root);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                previous_root,
                ScopeId::none(),
            );
            return;
        }
        let (offer_lanes, offer_lanes_len) = self.offer_lanes_for_scope(info.scope);
        let mut offer_lane_mask = 0u8;
        let mut offer_lane_idx = 0usize;
        while offer_lane_idx < offer_lanes_len {
            let offer_lane = offer_lanes[offer_lane_idx] as usize;
            if offer_lane < MAX_LANES {
                offer_lane_mask |= 1u8 << offer_lane;
            }
            offer_lane_idx += 1;
        }
        let selection_meta =
            self.compute_offer_entry_selection_meta(info.scope, info, offer_lanes_len != 0);
        let materialization_meta = self.compute_scope_arm_materialization_meta(info.scope);
        let summary = self.compute_offer_entry_static_summary(state.active_mask, entry_idx);
        let state = &mut self.offer_entry_state[entry_idx];
        state.lane_idx = lane_idx as u8;
        state.parallel_root = info.parallel_root;
        state.frontier = info.frontier;
        state.scope_id = info.scope;
        state.offer_lane_mask = offer_lane_mask;
        state.offer_lanes = offer_lanes;
        state.offer_lanes_len = offer_lanes_len as u8;
        state.selection_meta = selection_meta;
        state.label_meta = info.label_meta;
        state.materialization_meta = materialization_meta;
        state.summary = summary;
        self.global_active_entries
            .insert_entry(entry_idx, lane_idx as u8);
        self.attach_offer_entry_to_root_frontier(entry_idx, info.parallel_root, lane_idx as u8);
        self.recompute_global_offer_lane_mask();
        self.recompute_global_offer_lane_entry_slot_masks();
        self.recompute_root_frontier_offer_lane_mask(previous_root);
        self.recompute_root_frontier_offer_lane_entry_slot_masks(previous_root);
        self.recompute_root_frontier_offer_lane_mask(info.parallel_root);
        self.recompute_root_frontier_offer_lane_entry_slot_masks(info.parallel_root);
        let observed = self
            .recompute_offer_entry_observed_state_non_consuming(entry_idx)
            .unwrap_or(OfferEntryObservedState::EMPTY);
        self.offer_entry_state[entry_idx].observed = observed;
        self.refresh_frontier_observation_caches_for_entry(
            entry_idx,
            previous_root,
            info.parallel_root,
        );
    }

    fn detach_lane_from_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let entry_idx = state_index_to_usize(info.entry);
        if info.entry.is_max() || entry_idx >= crate::global::typestate::MAX_STATES {
            return;
        }
        let bit = 1u8 << lane_idx;
        let state = self.offer_entry_state[entry_idx];
        let active_mask = state.active_mask & !bit;
        if active_mask == 0 {
            self.detach_offer_entry_from_root_frontier(entry_idx, state.parallel_root);
            self.global_active_entries.remove_entry(entry_idx);
            self.offer_entry_state[entry_idx] = OfferEntryState::EMPTY;
            self.recompute_global_offer_lane_mask();
            self.recompute_global_offer_lane_entry_slot_masks();
            self.recompute_root_frontier_offer_lane_mask(state.parallel_root);
            self.recompute_root_frontier_offer_lane_entry_slot_masks(state.parallel_root);
            self.refresh_frontier_observation_caches_for_entry(
                entry_idx,
                state.parallel_root,
                ScopeId::none(),
            );
            return;
        }
        self.offer_entry_state[entry_idx].active_mask = active_mask;
        self.refresh_offer_entry_state(entry_idx);
    }

    fn attach_lane_to_offer_entry(&mut self, lane_idx: usize, info: LaneOfferState) {
        let entry_idx = state_index_to_usize(info.entry);
        if info.entry.is_max() || entry_idx >= crate::global::typestate::MAX_STATES {
            return;
        }
        let bit = 1u8 << lane_idx;
        let was_inactive = {
            let state = &mut self.offer_entry_state[entry_idx];
            let was_inactive = state.active_mask == 0;
            state.active_mask |= bit;
            was_inactive
        };
        if was_inactive {
            self.global_active_entries
                .insert_entry(entry_idx, lane_idx as u8);
            self.attach_offer_entry_to_root_frontier(entry_idx, info.parallel_root, lane_idx as u8);
        }
        self.refresh_offer_entry_state(entry_idx);
    }

    #[inline]
    fn clear_lane_offer_state(&mut self, lane_idx: usize) {
        let bit = 1u8 << lane_idx;
        let old = self.lane_offer_state[lane_idx];
        self.detach_lane_from_offer_entry(lane_idx, old);
        self.detach_lane_from_root_frontier(lane_idx, old);
        self.lane_offer_state[lane_idx] = LaneOfferState::EMPTY;
        self.active_offer_mask &= !bit;
        self.lane_offer_linger_mask &= !bit;
    }

    fn sync_lane_offer_state(&mut self) {
        let refresh_mask = self.offer_refresh_mask();
        let mut stale_mask = self.active_offer_mask & !refresh_mask;
        while stale_mask != 0 {
            let lane_idx = stale_mask.trailing_zeros() as usize;
            stale_mask &= !(1u8 << lane_idx);
            self.clear_lane_offer_state(lane_idx);
        }
        let mut lane_mask = refresh_mask;
        while lane_mask != 0 {
            let lane_idx = lane_mask.trailing_zeros() as usize;
            lane_mask &= !(1u8 << lane_idx);
            self.refresh_lane_offer_state(lane_idx);
        }
    }

    fn refresh_lane_offer_state(&mut self, lane_idx: usize) {
        if lane_idx >= MAX_LANES {
            return;
        }
        let bit = 1u8 << lane_idx;
        let old = self.lane_offer_state[lane_idx];
        self.detach_lane_from_offer_entry(lane_idx, old);
        self.detach_lane_from_root_frontier(lane_idx, old);
        self.lane_offer_state[lane_idx] = LaneOfferState::EMPTY;
        if let Some(info) = self.compute_lane_offer_state(lane_idx) {
            self.lane_offer_state[lane_idx] = info;
            self.active_offer_mask |= bit;
            if self.is_linger_route(info.scope) {
                self.lane_offer_linger_mask |= bit;
            } else {
                self.lane_offer_linger_mask &= !bit;
            }
            self.attach_lane_to_root_frontier(lane_idx, info);
            self.attach_lane_to_offer_entry(lane_idx, info);
        } else {
            self.active_offer_mask &= !bit;
            self.lane_offer_linger_mask &= !bit;
        }
    }

    fn compute_lane_offer_state(&self, lane_idx: usize) -> Option<LaneOfferState> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let (mut entry_idx, mut scope_id, mut lane_cursor) =
            if let Some(idx) = self.cursor.index_for_lane_step(lane_idx) {
                let lane_cursor = self.cursor.with_index(idx);
                let scope_id = lane_cursor.node_scope_id();
                (idx, scope_id, lane_cursor)
            } else {
                let (scope_id, entry) = self.active_linger_offer_for_lane(lane_idx)?;
                let entry_idx = state_index_to_usize(entry);
                let lane_cursor = self.cursor.with_index(entry_idx);
                (entry_idx, scope_id, lane_cursor)
            };
        let mut region = lane_cursor.scope_region_by_id(scope_id)?;
        if region.kind != ScopeKind::Route {
            return None;
        }
        let mut entry = lane_cursor
            .route_scope_offer_entry(region.scope_id)
            .unwrap_or(StateIndex::MAX);
        if !entry.is_max() && state_index_to_usize(entry) != entry_idx {
            let canonical_entry = state_index_to_usize(entry);
            if canonical_entry >= region.start && canonical_entry < region.end {
                let selected_arm = self.route_arm_for(lane_idx as u8, scope_id);
                if region.linger || selected_arm.is_none() {
                    // Keep offer-state participation only while this scope is unresolved.
                    // Once a non-linger route arm has been selected, re-entering offer_entry
                    // would replay a settled decision.
                    entry_idx = canonical_entry;
                    lane_cursor = self.cursor.with_index(entry_idx);
                } else if let Some((linger_scope, linger_entry)) =
                    self.active_linger_offer_for_lane(lane_idx)
                {
                    scope_id = linger_scope;
                    entry_idx = state_index_to_usize(linger_entry);
                    lane_cursor = self.cursor.with_index(entry_idx);
                    region = lane_cursor.scope_region_by_id(scope_id)?;
                    if region.kind != ScopeKind::Route {
                        return None;
                    }
                    entry = lane_cursor
                        .route_scope_offer_entry(region.scope_id)
                        .unwrap_or(StateIndex::MAX);
                    if !entry.is_max() && state_index_to_usize(entry) != entry_idx {
                        return None;
                    }
                } else {
                    return None;
                }
            } else {
                if let Some((linger_scope, linger_entry)) =
                    self.active_linger_offer_for_lane(lane_idx)
                {
                    scope_id = linger_scope;
                    entry_idx = state_index_to_usize(linger_entry);
                    lane_cursor = self.cursor.with_index(entry_idx);
                    region = lane_cursor.scope_region_by_id(scope_id)?;
                    if region.kind != ScopeKind::Route {
                        return None;
                    }
                    entry = lane_cursor
                        .route_scope_offer_entry(region.scope_id)
                        .unwrap_or(StateIndex::MAX);
                    if !entry.is_max() && state_index_to_usize(entry) != entry_idx {
                        return None;
                    }
                } else {
                    return None;
                }
            }
        }
        let entry_idx = if entry.is_max() {
            entry_idx
        } else {
            state_index_to_usize(entry)
        };
        let is_controller = lane_cursor.is_route_controller(region.scope_id);
        let is_dynamic = lane_cursor
            .route_scope_controller_policy(region.scope_id)
            .map(|(policy, _, _)| policy.is_dynamic())
            .unwrap_or(false);
        let static_facts =
            Self::frontier_static_facts(&lane_cursor, region.scope_id, is_controller, is_dynamic);
        let label_meta =
            Self::scope_label_meta(&lane_cursor, region.scope_id, static_facts.loop_meta);
        let mut flags = 0u8;
        if is_controller {
            flags |= LaneOfferState::FLAG_CONTROLLER;
        }
        if is_dynamic {
            flags |= LaneOfferState::FLAG_DYNAMIC;
        }
        let parallel_root =
            Self::parallel_scope_root(&lane_cursor, region.scope_id).unwrap_or(ScopeId::none());
        Some(LaneOfferState {
            scope: region.scope_id,
            entry: StateIndex::from_usize(entry_idx),
            parallel_root,
            frontier: static_facts.frontier,
            loop_meta: static_facts.loop_meta,
            label_meta,
            static_ready: static_facts.ready,
            flags,
        })
    }

    fn active_linger_offer_for_lane(&self, lane_idx: usize) -> Option<(ScopeId, StateIndex)> {
        if lane_idx >= MAX_LANES {
            return None;
        }
        let len = self.lane_route_arm_lens[lane_idx] as usize;
        let mut idx = len;
        while idx > 0 {
            idx -= 1;
            let slot = self.lane_route_arms[lane_idx][idx];
            if slot.scope.is_none() || slot.arm != 0 {
                continue;
            }
            if !self.is_linger_route(slot.scope) {
                continue;
            }
            if let Some(region) = self.cursor.scope_region_by_id(slot.scope) {
                if region.kind == ScopeKind::Route {
                    return Some((slot.scope, StateIndex::from_usize(region.start)));
                }
            }
        }
        None
    }

    fn set_lane_cursor_to_eff_index(&mut self, lane_idx: usize, eff_index: EffIndex) {
        self.cursor
            .set_lane_cursor_to_eff_index(lane_idx, eff_index);
        self.refresh_lane_offer_state(lane_idx);
    }

    /// Advance the cursor for a specific lane by one step.
    #[inline]
    fn advance_lane_cursor(&mut self, lane_idx: usize, eff_index: EffIndex) {
        self.cursor.advance_lane_to_eff_index(lane_idx, eff_index);
        self.refresh_lane_offer_state(lane_idx);
    }

    #[inline]
    fn align_cursor_to_lane_progress(&mut self, preferred_lane_idx: usize) -> bool {
        if let Some(idx) = self.cursor.index_for_lane_step(preferred_lane_idx) {
            self.set_cursor(self.cursor.with_index(idx));
            return true;
        }
        let mut lane_idx = 0usize;
        while lane_idx < MAX_LANES {
            if let Some(idx) = self.cursor.index_for_lane_step(lane_idx) {
                self.set_cursor(self.cursor.with_index(idx));
                return true;
            }
            lane_idx += 1;
        }
        false
    }

    fn advance_phase_skipping_inactive(&mut self) {
        self.cursor.advance_phase_without_sync();
        while self.phase_guard_mismatch() {
            self.cursor.advance_phase_without_sync();
        }
        self.cursor.sync_idx_to_phase_start();
        self.sync_lane_offer_state();
    }

    fn has_ready_frontier_candidate(&mut self) -> bool {
        if self.active_offer_mask == 0 {
            return false;
        }
        let scope_id = self.current_offer_scope_id();
        if scope_id.is_none() {
            return false;
        }
        let cursor_parallel = Self::parallel_scope_root(&self.cursor, scope_id);
        let mut has_ready = false;
        self.for_each_active_offer_candidate(cursor_parallel, |candidate| {
            has_ready |= candidate.ready;
            ControlFlow::<()>::Continue(())
        });
        has_ready
    }

    #[inline]
    fn maybe_advance_phase(&mut self) {
        if self.cursor.is_phase_complete() && !self.has_active_linger_route() {
            if self.has_ready_frontier_candidate() {
                return;
            }
            self.advance_phase_skipping_inactive();
        }
    }

    fn phase_guard_mismatch(&self) -> bool {
        let Some(phase) = self.cursor.current_phase() else {
            return false;
        };
        let guard = phase.route_guard;
        if guard.is_empty() {
            return false;
        }
        let Some(selected) = self.selected_arm_for_scope(guard.scope) else {
            return false;
        };
        selected != guard.arm
    }

    fn has_active_linger_route(&self) -> bool {
        let phase_mask = self
            .cursor
            .current_phase()
            .map(|phase| phase.lane_mask)
            .unwrap_or(0);
        ((self.lane_linger_mask | self.lane_offer_linger_mask) & phase_mask) != 0
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B> Drop
    for CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    fn drop(&mut self) {
        // Drop all active ports and guards
        for port in self.ports.iter_mut() {
            if let Some(p) = port.take() {
                drop(p);
            }
        }
        for guard in self.guards.iter_mut() {
            if let Some(g) = guard.take() {
                drop(g);
            }
        }
    }
}

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
    pub(crate) fn mint_control_token<K>(
        &self,
        dest_role: u8,
        shot: CapShot,
        lane: Lane,
    ) -> SendResult<GenericCapToken<K>>
    where
        K: ResourceKind + crate::control::cap::mint::SessionScopedKind,
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsCanonical,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        cluster
            .canonical_session_token::<K, Mint>(
                self.rendezvous_id(),
                self.sid,
                lane,
                dest_role,
                shot,
                self.mint,
            )
            .ok_or(SendError::PhaseInvariant)
    }

    pub(crate) fn mint_control_token_with_handle<K>(
        &self,
        dest_role: u8,
        shot: CapShot,
        lane: Lane,
        handle: K::Handle,
    ) -> SendResult<GenericCapToken<K>>
    where
        K: ResourceKind,
        <Mint as MintConfigMarker>::Policy: crate::control::cap::mint::AllowsCanonical,
    {
        let cluster = self.control.cluster().ok_or(SendError::PhaseInvariant)?;
        cluster
            .canonical_token_with_handle::<K, Mint>(
                self.rendezvous_id(),
                self.sid,
                lane,
                dest_role,
                shot,
                handle,
                self.mint,
            )
            .ok_or(SendError::PhaseInvariant)
    }

    fn splice_handle_from_operands(operands: SpliceOperands) -> SpliceHandle {
        let mut flags = 0u16;
        if operands.seq_tx != 0 || operands.seq_rx != 0 {
            flags |= splice_flags::FENCES_PRESENT;
        }
        SpliceHandle {
            src_rv: operands.src_rv.raw(),
            dst_rv: operands.dst_rv.raw(),
            src_lane: operands.src_lane.raw() as u16,
            dst_lane: operands.dst_lane.raw() as u16,
            old_gen: operands.old_gen.raw(),
            new_gen: operands.new_gen.raw(),
            seq_tx: operands.seq_tx,
            seq_rx: operands.seq_rx,
            flags,
        }
    }
}
pub trait CanonicalTokenProvider<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, B>
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    fn into_token(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        meta: &SendMeta,
    ) -> SendResult<
        Option<CapFlowToken<<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>>,
    >;
}

impl<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, B>
    CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B> for NoControl
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[inline(always)]
    fn into_token(
        _endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        _meta: &SendMeta,
    ) -> SendResult<
        Option<CapFlowToken<<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>>,
    > {
        Ok(None)
    }
}

impl<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, K, B>
    CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B> for ExternalControl<K>
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    Mint::Policy: crate::control::cap::mint::AllowsCanonical,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind:
        ResourceKind + ControlMint,
    K: ResourceKind,
    B: BindingSlot,
{
    /// External control: behavior depends on `K::AUTO_MINT_EXTERNAL`.
    ///
    /// - When `AUTO_MINT_EXTERNAL = true` (e.g., SpliceIntentKind):
    ///   Auto-mint a token with proper handle (sid/lane/scope) populated by the resolver.
    ///
    /// - When `AUTO_MINT_EXTERNAL = false` (default, e.g., LoadBeginKind):
    ///   Caller provides the token/payload directly; no auto-minting.
    #[inline(always)]
    fn into_token(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        meta: &SendMeta,
    ) -> SendResult<
        Option<CapFlowToken<<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>>,
    > {
        if K::AUTO_MINT_EXTERNAL {
            // Auto-mint for external splice kinds
            endpoint
                .canonical_control_token::<
                    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
                >(meta)
                .map(Some)
        } else {
            // Caller provides the payload directly
            Ok(None)
        }
    }
}

impl<'r, const ROLE: u8, T, U, C, E, Mint, const MAX_RV: usize, M, K, B>
    CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B> for CanonicalControl<K>
where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    Mint::Policy: crate::control::cap::mint::AllowsCanonical,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind:
        ResourceKind + ControlMint,
    K: ResourceKind,
    B: BindingSlot,
{
    #[inline(always)]
    fn into_token(
        endpoint: &CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        meta: &SendMeta,
    ) -> SendResult<
        Option<CapFlowToken<<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>>,
    > {
        endpoint
            .canonical_control_token::<
                <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
            >(meta)
            .map(Some)
    }
}
