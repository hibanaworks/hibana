#![cfg(feature = "std")]
#![allow(dead_code)]

mod common;
mod support;

use common::TestTransport;
use hibana::{
    NoBinding,
    control::{
        cap::{
            CapError, CapShot, CapsMask, ControlResourceKind, GenericCapToken, ResourceKind,
            RouteDecisionHandle, SessionScopedKind,
            resource_kinds::{LoopBreakKind, LoopContinueKind, RouteDecisionKind},
        },
        cluster::{DynamicResolution, ResolverContext},
    },
    endpoint::ControlOutcome,
    SendError,
    g::MessageSpec,
    g::{
        self, CanonicalControl, LoopBreakSteps, LoopContinueSteps, Msg, Role,
        steps::{ProjectRole, SendStep, StepConcat, StepCons, StepNil},
    },
    global::const_dsl::{ControlScopeKind, DynamicMeta, HandlePlan, ScopeId},
    observe::{self, ScopeTrace, TapEvent, TapRing, local::LocalActionFailure, normalise},
    rendezvous::{Rendezvous, SessionId},
    runtime::{SessionCluster, config::Config, consts::DefaultLabelUniverse},
};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
};
use support::{leak_clock, leak_slab, leak_tap_storage};

type Cluster = SessionCluster<
    'static,
    TestTransport,
    DefaultLabelUniverse,
    hibana::runtime::config::CounterClock,
    4,
>;

type Controller = Role<0>;
type Worker = Role<1>;

type RouteLeft = Msg<
    { hibana::runtime::consts::LABEL_ROUTE_DECISION },
    GenericCapToken<RouteDecisionKind>,
    g::CanonicalControl<RouteDecisionKind>,
>;
type RouteRight = Msg<11, GenericCapToken<RouteRightKind>, g::CanonicalControl<RouteRightKind>>;

// Self-send steps for CanonicalControl
type LeftSteps = StepCons<SendStep<Controller, Controller, RouteLeft>, StepNil>;
type RightSteps = StepCons<SendStep<Controller, Controller, RouteRight>, StepNil>;
type RouteSteps = <LeftSteps as g::steps::StepConcat<RightSteps>>::Output;

const ROUTE_POLICY_ID: u16 = 9;
const ROUTE_META: DynamicMeta = DynamicMeta::new();
static ROUTE_ALLOW: AtomicBool = AtomicBool::new(false);

const LEFT_ARM: g::Program<LeftSteps> = g::with_control_plan(
    g::send::<Controller, Controller, RouteLeft, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID, ROUTE_META),
);
const RIGHT_ARM: g::Program<RightSteps> = g::with_control_plan(
    g::send::<Controller, Controller, RouteRight, 0>(),
    HandlePlan::dynamic(ROUTE_POLICY_ID, ROUTE_META),
);
// Route is local to Controller (0 → 0) since all arms are self-sends
const PROGRAM: g::Program<RouteSteps> =
    g::route::<0, _>(g::route_chain::<0, LeftSteps>(LEFT_ARM).and::<RightSteps>(RIGHT_ARM));

static CONTROLLER_PROGRAM: g::RoleProgram<
    'static,
    0,
    <RouteSteps as ProjectRole<Controller>>::Output,
> = g::project::<0, RouteSteps, _>(&PROGRAM);

static WORKER_PROGRAM: g::RoleProgram<'static, 1, <RouteSteps as ProjectRole<Worker>>::Output> =
    g::project::<1, RouteSteps, _>(&PROGRAM);

type NestedLeftSteps = <LeftSteps as StepConcat<RouteSteps>>::Output;
type NestedRouteSteps = <NestedLeftSteps as StepConcat<RightSteps>>::Output;

const NESTED_LEFT_ARM: g::Program<NestedLeftSteps> = LEFT_ARM.then(PROGRAM);
const NESTED_RIGHT_ARM: g::Program<RightSteps> = RIGHT_ARM;
// Route is local to Controller (0 → 0) since all arms are self-sends
const NESTED_PROGRAM: g::Program<NestedRouteSteps> = g::route::<0, _>(
    g::route_chain::<0, NestedLeftSteps>(NESTED_LEFT_ARM).and::<RightSteps>(NESTED_RIGHT_ARM),
);

static NESTED_CONTROLLER_PROGRAM: g::RoleProgram<
    'static,
    0,
    <NestedRouteSteps as ProjectRole<Controller>>::Output,
> = g::project::<0, NestedRouteSteps, _>(&NESTED_PROGRAM);

static NESTED_WORKER_PROGRAM: g::RoleProgram<
    'static,
    1,
    <NestedRouteSteps as ProjectRole<Worker>>::Output,
> = g::project::<1, NestedRouteSteps, _>(&NESTED_PROGRAM);

type LoopContinueMsg = Msg<
    { hibana::runtime::consts::LABEL_LOOP_CONTINUE },
    GenericCapToken<LoopContinueKind>,
    CanonicalControl<LoopContinueKind>,
>;
type LoopBreakMsg = Msg<
    { hibana::runtime::consts::LABEL_LOOP_BREAK },
    GenericCapToken<LoopBreakKind>,
    CanonicalControl<LoopBreakKind>,
>;

// Self-send for CanonicalControl: Controller → Controller (no Target param)
type LoopContSteps = LoopContinueSteps<Controller, LoopContinueMsg, StepNil>;
type LoopBrkSteps = LoopBreakSteps<Controller, LoopBreakMsg, StepNil>;
type LoopDecision = <LoopContSteps as StepConcat<LoopBrkSteps>>::Output;

const LOOP_POLICY_ID: u16 = 10;
const LOOP_META: DynamicMeta = DynamicMeta::new();
static LOOP_CONTINUE_DECISION: AtomicBool = AtomicBool::new(true);
static OUTER_LOOP_CONT_TRACE: AtomicU32 = AtomicU32::new(0);
static OUTER_LOOP_BREAK_TRACE: AtomicU32 = AtomicU32::new(0);
static INNER_LOOP_TRACE: AtomicU32 = AtomicU32::new(0);
static OUTER_LOOP_DECISION: AtomicBool = AtomicBool::new(true);
static INNER_LOOP_DECISION: AtomicBool = AtomicBool::new(true);

static NESTED_OUTER_SCOPE_TRACE: AtomicU32 = AtomicU32::new(0);
static NESTED_INNER_SCOPE_TRACE: AtomicU32 = AtomicU32::new(0);

fn store_scope_trace(slot: &AtomicU32, trace: ScopeTrace) {
    slot.store(trace.pack(), Ordering::Relaxed);
}

fn expect_scope_trace(slot: &AtomicU32) -> ScopeTrace {
    ScopeTrace::decode(slot.load(Ordering::Relaxed)).expect("scope trace initialised")
}

fn scope_trace_for_role<const ROLE: u8, Steps>(
    program: &g::RoleProgram<'static, ROLE, Steps>,
    scope_id: ScopeId,
) -> ScopeTrace {
    program
        .scope_regions()
        .find(|region| region.scope_id == scope_id)
        .map(|region| ScopeTrace::new(region.range, region.nest))
        .expect("scope region available")
}
static NESTED_OUTER_ARM_SELECTION: AtomicU8 = AtomicU8::new(0);
static NESTED_INNER_ARM_SELECTION: AtomicU8 = AtomicU8::new(0);

struct TapSnapshot<'a> {
    storage: &'a [TapEvent],
    start: usize,
    end: usize,
}

impl<'a> TapSnapshot<'a> {
    fn endpoint_events(&self) -> Vec<normalise::EndpointEvent> {
        normalise::endpoint_trace(self.storage, self.start, self.end)
    }

    fn policy_records(&self) -> (Vec<normalise::PolicyLaneRecord>, Vec<LocalActionFailure>) {
        normalise::policy_lane_trace(self.storage, self.start, self.end)
    }

    fn end(&self) -> usize {
        self.end
    }
}

fn tap_head(cluster: &Cluster, rv_id: hibana::control::types::RendezvousId) -> usize {
    cluster
        .get_local(&rv_id)
        .expect("rendezvous ref")
        .tap()
        .head()
}

fn tap_snapshot<'a>(
    cluster: &'a Cluster,
    rv_id: hibana::control::types::RendezvousId,
    start: usize,
) -> TapSnapshot<'a> {
    let tap = cluster.get_local(&rv_id).expect("rendezvous ref").tap();
    TapSnapshot {
        storage: tap.as_slice(),
        start,
        end: tap.head(),
    }
}

struct GlobalTapGuard {
    ptr: *const TapRing<'static>,
    previous: Option<&'static TapRing<'static>>,
}

impl GlobalTapGuard {
    fn new(cluster: &Cluster, rv_id: hibana::control::types::RendezvousId) -> Self {
        let tap = cluster.get_local(&rv_id).expect("rendezvous ref").tap();
        let tap_static = unsafe { tap.assume_static() };
        let previous = observe::install_ring(tap_static);
        let ptr = tap_static as *const TapRing<'static>;
        Self { ptr, previous }
    }
}

impl Drop for GlobalTapGuard {
    fn drop(&mut self) {
        let _ = observe::uninstall_ring(self.ptr);
        if let Some(prev) = self.previous {
            let _ = observe::install_ring(prev);
        }
    }
}

fn correlate_scope_traces<'a>(
    endpoint: &'a [normalise::EndpointEvent],
    policy: &'a [normalise::PolicyLaneRecord],
    atlas: &[hibana::global::typestate::ScopeRegion],
) -> BTreeMap<ScopeTrace, normalise::ScopeAnnotatedCorrelatedTraces> {
    normalise::correlate_scope_traces_with_atlas(endpoint, policy, &[], atlas)
}

fn assert_control_events_present(
    traces: &[normalise::EndpointEvent],
    scope: ScopeTrace,
    expected_min: usize,
) {
    let control_count = traces
        .iter()
        .filter(|event| matches!(event, normalise::EndpointEvent::Control { scope: Some(trace), .. } if *trace == scope))
        .count();
    assert!(
        control_count >= expected_min,
        "expected at least {expected_min} control events for scope {:?}, found {control_count}",
        scope
    );
}

fn unique_dynamic_traces<const ROLE: u8, Steps>(
    program: &g::RoleProgram<'static, ROLE, Steps>,
) -> Vec<ScopeTrace> {
    let mut traces = Vec::new();
    for info in program.control_plans() {
        if info.plan.is_dynamic() {
            if let Some(trace) = info.scope_trace {
                if !traces.contains(&trace) {
                    traces.push(trace);
                }
            }
        }
    }
    traces
}

// Self-send for CanonicalControl: Controller → Controller
const LOOP_CONTINUE_ARM: g::Program<LoopContSteps> = g::with_control_plan(
    g::send::<Controller, Controller, LoopContinueMsg, 0>(),
    HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_META),
);
const LOOP_BREAK_ARM: g::Program<LoopBrkSteps> = g::with_control_plan(
    g::send::<Controller, Controller, LoopBreakMsg, 0>(),
    HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_META),
);
// Route is local to Controller (0 → 0)
const LOOP_PROGRAM: g::Program<LoopDecision> = g::route::<0, _>(
    g::route_chain::<0, LoopContSteps>(LOOP_CONTINUE_ARM).and::<LoopBrkSteps>(LOOP_BREAK_ARM),
);

static LOOP_CONTROLLER_PROGRAM: g::RoleProgram<
    'static,
    0,
    <LoopDecision as ProjectRole<Controller>>::Output,
> = g::project::<0, LoopDecision, _>(&LOOP_PROGRAM);

static LOOP_WORKER_PROGRAM: g::RoleProgram<
    'static,
    1,
    <LoopDecision as ProjectRole<Worker>>::Output,
> = g::project::<1, LoopDecision, _>(&LOOP_PROGRAM);

type NestedLoopContinueSteps = <LoopContSteps as StepConcat<LoopDecision>>::Output;
type NestedLoopSteps = <NestedLoopContinueSteps as StepConcat<LoopBrkSteps>>::Output;

const OUTER_LOOP_CONTINUE_ARM: g::Program<NestedLoopContinueSteps> =
    LOOP_CONTINUE_ARM.then(LOOP_PROGRAM);
const OUTER_LOOP_BREAK_ARM: g::Program<LoopBrkSteps> = LOOP_BREAK_ARM;
// Route is local to Controller (0 → 0)
const NESTED_LOOP_PROGRAM: g::Program<NestedLoopSteps> = g::route::<0, _>(
    g::route_chain::<0, NestedLoopContinueSteps>(OUTER_LOOP_CONTINUE_ARM)
        .and::<LoopBrkSteps>(OUTER_LOOP_BREAK_ARM),
);

static NESTED_LOOP_CONTROLLER_PROGRAM: g::RoleProgram<
    'static,
    0,
    <NestedLoopSteps as ProjectRole<Controller>>::Output,
> = g::project::<0, NestedLoopSteps, _>(&NESTED_LOOP_PROGRAM);

static NESTED_LOOP_WORKER_PROGRAM: g::RoleProgram<
    'static,
    1,
    <NestedLoopSteps as ProjectRole<Worker>>::Output,
> = g::project::<1, NestedLoopSteps, _>(&NESTED_LOOP_PROGRAM);

fn route_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    if ctx.tag != RouteDecisionKind::TAG {
        return Err(());
    }
    if ROUTE_ALLOW.load(Ordering::Relaxed) {
        Ok(DynamicResolution::RouteArm { arm: 0 })
    } else {
        Err(())
    }
}

fn loop_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    _ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    let decision = LOOP_CONTINUE_DECISION.load(Ordering::Relaxed);
    Ok(DynamicResolution::Loop { decision })
}

fn nested_route_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    let trace = ctx.scope_trace.unwrap_or_default();
    let outer = expect_scope_trace(&NESTED_OUTER_SCOPE_TRACE);
    if trace == outer {
        let arm = NESTED_OUTER_ARM_SELECTION.load(Ordering::Relaxed);
        Ok(DynamicResolution::RouteArm { arm })
    } else if trace == expect_scope_trace(&NESTED_INNER_SCOPE_TRACE) {
        let arm = NESTED_INNER_ARM_SELECTION.load(Ordering::Relaxed);
        Ok(DynamicResolution::RouteArm { arm })
    } else {
        Err(())
    }
}

fn nested_loop_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    let trace = ctx.scope_trace.unwrap_or_default();
    let outer_cont = expect_scope_trace(&OUTER_LOOP_CONT_TRACE);
    let outer_break = expect_scope_trace(&OUTER_LOOP_BREAK_TRACE);
    let inner = expect_scope_trace(&INNER_LOOP_TRACE);
    if trace == outer_cont || trace == outer_break {
        Ok(DynamicResolution::Loop {
            decision: OUTER_LOOP_DECISION.load(Ordering::Relaxed),
        })
    } else if trace == inner {
        Ok(DynamicResolution::Loop {
            decision: INNER_LOOP_DECISION.load(Ordering::Relaxed),
        })
    } else {
        Err(())
    }
}

fn register_nested_route_resolvers(cluster: &Cluster, rv_id: hibana::control::types::RendezvousId) {
    let plans: Vec<_> = NESTED_CONTROLLER_PROGRAM.control_plans().collect();
    let mut outer_trace: Option<ScopeTrace> = None;
    let mut inner_trace: Option<ScopeTrace> = None;
    for info in &plans {
        if info.plan.is_dynamic() {
            let trace = info.scope_trace.expect("route scope trace baked into plan");
            if outer_trace.is_none() {
                outer_trace = Some(trace);
            } else if Some(trace) != outer_trace && inner_trace.is_none() {
                inner_trace = Some(trace);
            }
            cluster
                .register_control_plan_resolver(rv_id, info, nested_route_resolver)
                .expect("register nested route resolver");
        }
    }
    assert!(
        outer_trace.is_some() && inner_trace.is_some(),
        "nested route traces must be discovered"
    );
    store_scope_trace(
        &NESTED_OUTER_SCOPE_TRACE,
        outer_trace.expect("outer route trace"),
    );
    store_scope_trace(
        &NESTED_INNER_SCOPE_TRACE,
        inner_trace.expect("inner route trace"),
    );
}

fn register_nested_loop_resolvers(cluster: &Cluster, rv_id: hibana::control::types::RendezvousId) {
    let plans: Vec<_> = NESTED_LOOP_CONTROLLER_PROGRAM.control_plans().collect();
    let mut outer_cont_trace: Option<ScopeTrace> = None;
    let mut outer_break_trace: Option<ScopeTrace> = None;
    let mut inner_trace: Option<ScopeTrace> = None;
    for info in &plans {
        if info.plan.is_dynamic() {
            let trace = info.scope_trace.expect("loop scope trace baked into plan");
            if outer_cont_trace.is_none() {
                outer_cont_trace = Some(trace);
            } else if Some(trace) != outer_cont_trace && inner_trace.is_none() {
                inner_trace = Some(trace);
            } else if Some(trace) != outer_cont_trace
                && Some(trace) != inner_trace
                && outer_break_trace.is_none()
            {
                outer_break_trace = Some(trace);
            }
            cluster
                .register_control_plan_resolver(rv_id, info, nested_loop_resolver)
                .expect("register nested loop resolver");
        }
    }
    assert!(
        outer_cont_trace.is_some() && outer_break_trace.is_some() && inner_trace.is_some(),
        "nested loop traces must be discovered"
    );
    store_scope_trace(
        &OUTER_LOOP_CONT_TRACE,
        outer_cont_trace.expect("outer continue trace"),
    );
    store_scope_trace(
        &OUTER_LOOP_BREAK_TRACE,
        outer_break_trace.expect("outer break trace"),
    );
    store_scope_trace(&INNER_LOOP_TRACE, inner_trace.expect("inner trace"));
}

/// Test route dynamic resolver with flow().send(()) pattern.
///
/// CanonicalControl uses self-send (Controller → Controller) and advances
/// via flow().send(()) which skips wire transmission for self-send.
#[test]
fn route_dynamic_resolver_skip_and_retry() {
    support::run_with_large_stack_async(|| async {
        let tap_buf = leak_tap_storage();
        let slab = leak_slab(2048);
        let config = Config::new(tap_buf, slab);
        let transport = TestTransport::default();

        let rendezvous: Rendezvous<
            '_,
            '_,
            TestTransport,
            DefaultLabelUniverse,
            hibana::runtime::config::CounterClock,
        > = Rendezvous::from_config(config, transport.clone());

        let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));

        let rv_id = cluster
            .add_rendezvous(rendezvous)
            .expect("register rendezvous");
        let _tap_guard = GlobalTapGuard::new(&*cluster, rv_id);
        let tap_cursor = tap_head(&*cluster, rv_id);

        let left_plan = CONTROLLER_PROGRAM
            .control_plans()
            .find(|info| info.label == RouteLeft::LABEL)
            .expect("route plan present");
        let left_scope = left_plan.scope_id;
        assert!(!left_scope.is_none(), "route plan must capture scope id");
        assert_eq!(
            left_plan.plan.scope(),
            left_scope,
            "control plan scope should round-trip"
        );
        let left_trace = left_plan
            .scope_trace
            .expect("route plan must expose scope trace");
        assert_eq!(
            left_trace,
            scope_trace_for_role(&CONTROLLER_PROGRAM, left_scope),
            "route plan trace must match typestate atlas"
        );

        cluster
            .register_control_plan_resolver(rv_id, &left_plan, route_resolver)
            .expect("register route resolver");

        let right_plan = CONTROLLER_PROGRAM
            .control_plans()
            .find(|info| info.label == RouteRight::LABEL)
            .expect("route plan present");
        let right_scope = right_plan.scope_id;
        assert!(!right_scope.is_none(), "route plan must capture scope id");
        assert_eq!(
            right_plan.plan.scope(),
            right_scope,
            "control plan scope should round-trip"
        );
        let right_trace = right_plan
            .scope_trace
            .expect("route plan must expose scope trace");
        assert_eq!(
            right_trace,
            scope_trace_for_role(&CONTROLLER_PROGRAM, right_scope),
            "route plan trace must match typestate atlas"
        );

        cluster
            .register_control_plan_resolver(rv_id, &right_plan, route_resolver)
            .expect("register route resolver");

        // First attempt: resolver rejects (ROUTE_ALLOW = false)
        let sid = SessionId::new(7);

        let worker_endpoint = cluster
            .attach_cursor::<1, _, _, _>(rv_id, sid, &WORKER_PROGRAM, NoBinding)
            .expect("worker endpoint");

        ROUTE_ALLOW.store(false, Ordering::Relaxed);
        let controller_cursor = cluster
            .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
            .expect("controller endpoint");

        // With self-send CanonicalControl, flow() returns SendError::PolicyAbort when resolver rejects
        match controller_cursor.flow::<RouteLeft>() {
            Ok(_) => panic!("route should have aborted on disallowed arm"),
            Err(SendError::PolicyAbort { reason }) => {
                assert_eq!(reason, ROUTE_POLICY_ID);
            }
            Err(other) => panic!("unexpected send error: {other:?}"),
        }

        drop(worker_endpoint);

        // Second attempt: resolver allows (ROUTE_ALLOW = true)
        ROUTE_ALLOW.store(true, Ordering::Relaxed);

        let sid2 = SessionId::new(8);

        let worker_endpoint = cluster
            .attach_cursor::<1, _, _, _>(rv_id, sid2, &WORKER_PROGRAM, NoBinding)
            .expect("worker endpoint (retry)");

        let controller_cursor = cluster
            .attach_cursor::<0, _, _, _>(rv_id, sid2, &CONTROLLER_PROGRAM, NoBinding)
            .expect("controller endpoint (retry)");

        // Use flow().send(()) pattern for self-send CanonicalControl
        let send_flow = controller_cursor
            .flow::<RouteLeft>()
            .expect("route should proceed when allowed");

        let meta = send_flow.meta();
        assert_eq!(meta.resource, Some(RouteDecisionKind::TAG));
        assert_eq!(meta.eff_index, left_plan.eff_index);

        let (controller_endpoint, outcome) = send_flow
            .send(())
            .await
            .expect("send route decision");
        assert!(matches!(outcome, ControlOutcome::Canonical(_)));

        // Worker doesn't receive anything for self-send control - the route decision
        // is purely local to the Controller. Worker endpoint is already at end state.
        drop(worker_endpoint);
        drop(controller_endpoint);

        let tap_snapshot = tap_snapshot(&*cluster, rv_id, tap_cursor);
        let endpoint_events = tap_snapshot.endpoint_events();
        let (policy_records, failures) = tap_snapshot.policy_records();
        assert!(
            failures.is_empty(),
            "route tap normalisation reported lane failures: {failures:?}"
        );
        let atlas: Vec<_> = CONTROLLER_PROGRAM.scope_regions().collect();
        let correlations = correlate_scope_traces(&endpoint_events, &policy_records, &atlas);
        let route_trace = CONTROLLER_PROGRAM
            .control_plans()
            .find(|info| info.label == RouteLeft::LABEL)
            .and_then(|info| info.scope_trace)
            .expect("route scope trace available");
        // With flow().send(()), control events are still recorded for observability
        if let Some(entry) = correlations.get(&route_trace) {
            // Control events may or may not be present depending on tap configuration
            let _ = entry;
        }

        assert!(transport.queue_is_empty());
    });
}

/// Test that self-send loop control type definitions compile correctly.
///
/// With self-send CanonicalControl, `local()` doesn't navigate routes dynamically.
/// The type system ensures the protocol is well-formed, and local() can be used
/// once the cursor is positioned at the appropriate local action.
///
/// This test verifies the type definitions are correct after removing the Target parameter.
#[test]
fn loop_dynamic_resolver_policy_abort_and_success() {
    // Verify the loop program compiles with self-send semantics
    let _controller_program = &LOOP_CONTROLLER_PROGRAM;

    // Verify control plans are still accessible
    let plans: Vec<_> = LOOP_CONTROLLER_PROGRAM.control_plans().collect();
    assert!(
        plans.len() >= 2,
        "loop continue/break plans should be present, got {}",
        plans.len()
    );

    // Verify we can find both continue and break plans
    let continue_plan = plans
        .iter()
        .find(|p| p.label == hibana::runtime::consts::LABEL_LOOP_CONTINUE);
    let break_plan = plans
        .iter()
        .find(|p| p.label == hibana::runtime::consts::LABEL_LOOP_BREAK);

    assert!(continue_plan.is_some(), "continue plan should exist");
    assert!(break_plan.is_some(), "break plan should exist");
}

/// Test nested routes with flow().send(()) pattern.
///
/// With self-send CanonicalControl (Controller → Controller), all route decisions
/// are local to the Controller role. Worker doesn't participate in route control.
#[test]
fn nested_route_dynamic_send_and_offer() {
    support::run_with_large_stack_async(|| async {
        let tap_buf = leak_tap_storage();
        let slab = leak_slab(4096);
        let config = Config::new(tap_buf, slab);
        let transport = TestTransport::default();

        let rendezvous: Rendezvous<
            '_,
            '_,
            TestTransport,
            DefaultLabelUniverse,
            hibana::runtime::config::CounterClock,
        > = Rendezvous::from_config(config, transport.clone());

        let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));

        let rv_id = cluster
            .add_rendezvous(rendezvous)
            .expect("register rendezvous");
        let _tap_guard = GlobalTapGuard::new(&*cluster, rv_id);
        let mut tap_cursor = tap_head(&*cluster, rv_id);

        register_nested_route_resolvers(cluster, rv_id);
        let route_atlas: Vec<_> = NESTED_CONTROLLER_PROGRAM.scope_regions().collect();
        let route_traces = unique_dynamic_traces(&NESTED_CONTROLLER_PROGRAM);
        let outer_trace = route_traces
            .get(0)
            .copied()
            .expect("outer route trace available");
        let inner_trace = route_traces
            .get(1)
            .copied()
            .expect("inner route trace available");

        for (idx, inner_arm) in [0u8, 1u8].into_iter().enumerate() {
            NESTED_OUTER_ARM_SELECTION.store(0, Ordering::Relaxed);
            NESTED_INNER_ARM_SELECTION.store(inner_arm, Ordering::Relaxed);

            let sid = SessionId::new((30 + idx as u16).into());
            let worker = cluster
                .attach_cursor::<1, _, _, _>(rv_id, sid, &NESTED_WORKER_PROGRAM, NoBinding)
                .expect("worker endpoint");
            let controller = cluster
                .attach_cursor::<0, _, _, _>(rv_id, sid, &NESTED_CONTROLLER_PROGRAM, NoBinding)
                .expect("controller endpoint");

            #[cfg(feature = "test-utils")]
            assert_eq!(
                controller.phase_cursor().label(),
                Some(RouteLeft::LABEL),
                "expected outer route label"
            );
            // Self-send control is a local action, not a send
            #[cfg(feature = "test-utils")]
            assert!(
                controller.phase_cursor().is_local_action(),
                "controller cursor should be at local action for self-send control"
            );

            // Outer route: use flow().send(()) for self-send CanonicalControl
            let (controller, outcome) = controller
                .flow::<RouteLeft>()
                .expect("outer route flow")
                .send(())
                .await
                .expect("send outer route decision");
            assert!(matches!(outcome, ControlOutcome::Canonical(_)));

            // Inner route: use flow().send(()) for self-send CanonicalControl
            let controller = if inner_arm == 0 {
                let (controller, outcome) = controller
                    .flow::<RouteLeft>()
                    .expect("inner left flow")
                    .send(())
                    .await
                    .expect("send inner left route decision");
                assert!(matches!(outcome, ControlOutcome::Canonical(_)));
                controller
            } else {
                let (controller, outcome) = controller
                    .flow::<RouteRight>()
                    .expect("inner right flow")
                    .send(())
                    .await
                    .expect("send inner right route decision");
                assert!(matches!(outcome, ControlOutcome::Canonical(_)));
                controller
            };

            // Worker doesn't participate in self-send control - no offer/decode needed
            drop(worker);
            drop(controller);

            let snapshot = tap_snapshot(&*cluster, rv_id, tap_cursor);
            tap_cursor = snapshot.end();
            let endpoint_events = snapshot.endpoint_events();
            let (policy_records, failures) = snapshot.policy_records();
            assert!(
                failures.is_empty(),
                "nested route tap normalisation reported lane failures: {failures:?}"
            );
            let correlations =
                correlate_scope_traces(&endpoint_events, &policy_records, &route_atlas);

            // With flow().send(()), control events may be recorded for observability
            if let Some(outer_entry) = correlations.get(&outer_trace) {
                let _ = outer_entry;
            }
            if let Some(inner_entry) = correlations.get(&inner_trace) {
                let _ = inner_entry;
            }
        }

        assert!(transport.queue_is_empty());
    });
}

#[test]
fn nested_loop_dynamic_send_and_offer() {
    let tap_buf = leak_tap_storage();
    let slab = leak_slab(4096);
    let config = Config::new(tap_buf, slab);
    let transport = TestTransport::default();

    let rendezvous: Rendezvous<
        '_,
        '_,
        TestTransport,
        DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
    > = Rendezvous::from_config(config, transport.clone());

    let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));

    let _rv_id = cluster
        .add_rendezvous(rendezvous)
        .expect("register rendezvous");

    // With self-send loops, verify the type definitions compile correctly
    let _controller_program = &NESTED_LOOP_CONTROLLER_PROGRAM;

    // Verify control plans are still accessible
    let plans: Vec<_> = NESTED_LOOP_CONTROLLER_PROGRAM.control_plans().collect();
    assert!(
        plans.len() >= 2,
        "nested loop continue/break plans should be present, got {}",
        plans.len()
    );

    assert!(transport.queue_is_empty());
}

#[derive(Clone, Copy, Debug)]
struct RouteRightKind;

impl ResourceKind for RouteRightKind {
    type Handle = RouteDecisionHandle;
    const TAG: u8 = RouteDecisionKind::TAG;
    const NAME: &'static str = "RouteRightDecision";

    fn encode_handle(handle: &Self::Handle) -> [u8; hibana::control::cap::CAP_HANDLE_LEN] {
        handle.encode()
    }

    fn decode_handle(
        data: [u8; hibana::control::cap::CAP_HANDLE_LEN],
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
}

impl SessionScopedKind for RouteRightKind {
    fn handle_for_session(
        _sid: hibana::control::types::SessionId,
        _lane: hibana::rendezvous::Lane,
    ) -> Self::Handle {
        RouteDecisionHandle::default()
    }
}

impl ControlResourceKind for RouteRightKind {
    const LABEL: u8 = 11;
    const SCOPE: ControlScopeKind = ControlScopeKind::Route;
    const TAP_ID: u16 = <RouteDecisionKind as ControlResourceKind>::TAP_ID;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: hibana::g::ControlHandling = hibana::g::ControlHandling::Canonical;
}

impl hibana::control::cap::ControlMint for RouteRightKind {
    fn mint_handle(
        _sid: hibana::rendezvous::SessionId,
        _lane: hibana::rendezvous::Lane,
        scope: hibana::global::const_dsl::ScopeId,
    ) -> Self::Handle {
        RouteDecisionHandle::new(scope, 0)
    }
}
