#![cfg(feature = "std")]

mod common;
mod support;

use common::TestTransport;
use hibana::{
    NoBinding,
    control::cap::{
        GenericCapToken,
        resource_kinds::{CancelKind, CheckpointKind, RollbackKind},
    },
    endpoint::ControlOutcome,
    g::{
        self, Msg, Role,
        steps::{ProjectRole, SendStep, StepCons, StepNil},
    },
    rendezvous::{Rendezvous, SessionId},
    runtime::{
        SessionCluster,
        config::{Config, CounterClock},
        consts::DefaultLabelUniverse,
    },
};
use support::{leak_slab, leak_tap_storage};

type Cluster = SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>;

type Controller = Role<0>;
type Worker = Role<1>;

// CanonicalControl requires self-send (From == To)
type CancelMsg = Msg<
    { hibana::runtime::consts::LABEL_CANCEL },
    GenericCapToken<CancelKind>,
    hibana::g::CanonicalControl<CancelKind>,
>;
type CheckpointMsg = Msg<
    { hibana::runtime::consts::LABEL_CHECKPOINT },
    GenericCapToken<CheckpointKind>,
    hibana::g::CanonicalControl<CheckpointKind>,
>;
type RollbackMsg = Msg<
    { hibana::runtime::consts::LABEL_ROLLBACK },
    GenericCapToken<RollbackKind>,
    hibana::g::CanonicalControl<RollbackKind>,
>;
type BootstrapMsg = Msg<1, u32>;

// Self-send steps for CanonicalControl
type CancelSteps = StepCons<SendStep<Controller, Controller, CancelMsg, 0>, StepNil>;
type CancelProgram = g::Program<CancelSteps>;

const CANCEL_PROTOCOL: CancelProgram = g::send::<Controller, Controller, CancelMsg, 0>();

type ControllerCancelLocal = <CancelSteps as ProjectRole<Controller>>::Output;
#[allow(dead_code)]
type WorkerCancelLocal = <CancelSteps as ProjectRole<Worker>>::Output;

static CONTROLLER_CANCEL_PROGRAM: g::RoleProgram<'static, 0, ControllerCancelLocal> =
    g::project::<0, CancelSteps, _>(&CANCEL_PROTOCOL);

// Self-send steps for CanonicalControl
type CheckpointSteps = StepCons<
    SendStep<Controller, Controller, CheckpointMsg, 0>,
    StepCons<SendStep<Controller, Controller, RollbackMsg, 0>, StepNil>,
>;
type CheckpointProgram = g::Program<CheckpointSteps>;

const CHECKPOINT_PROTOCOL: CheckpointProgram = g::seq(
    g::send::<Controller, Controller, CheckpointMsg, 0>(),
    g::send::<Controller, Controller, RollbackMsg, 0>(),
);

type ControllerCheckpointLocal = <CheckpointSteps as ProjectRole<Controller>>::Output;

static CONTROLLER_CHECKPOINT_PROGRAM: g::RoleProgram<'static, 0, ControllerCheckpointLocal> =
    g::project::<0, CheckpointSteps, _>(&CHECKPOINT_PROTOCOL);
type BootstrapSteps = StepCons<SendStep<Controller, Worker, BootstrapMsg, 0>, StepNil>;
type ControllerBootstrapLocal = <BootstrapSteps as ProjectRole<Controller>>::Output;

const BOOTSTRAP_PROTOCOL: g::Program<BootstrapSteps> =
    g::send::<Controller, Worker, BootstrapMsg, 0>();

static CONTROLLER_BOOTSTRAP_PROGRAM: g::RoleProgram<'static, 0, ControllerBootstrapLocal> =
    g::project::<0, BootstrapSteps, _>(&BOOTSTRAP_PROTOCOL);

/// Test cancel as a local action (self-send) via unified flow().send() API.
/// CanonicalControl self-send means Controller makes the local decision.
/// This test verifies that typestate advances correctly through flow().send().
#[tokio::test]
async fn cancel_local_action_advances_typestate() {
    let tap_storage = leak_tap_storage();
    let slab = leak_slab(2048);
    let config = Config::new(tap_storage, slab);
    let transport = TestTransport::default();
    let rendezvous = Rendezvous::from_config(config, transport.clone());
    let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(support::leak_clock())));
    let rv_id = cluster
        .add_rendezvous(rendezvous)
        .expect("register rendezvous");

    let sid = SessionId::new(7);

    {
        let bootstrap = cluster
            .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_BOOTSTRAP_PROGRAM, NoBinding)
            .expect("bootstrap attach");
        drop(bootstrap);
    }

    // Self-send: only Controller participates in CancelMsg
    let controller = cluster
        .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_CANCEL_PROGRAM, NoBinding)
        .expect("attach controller");

    #[cfg(feature = "test-utils")]
    let initial_idx = controller.phase_cursor().index();

    // Unified API: flow().send(()) for CanonicalControl (auto-minted token)
    let (controller, outcome) = controller
        .flow::<CancelMsg>()
        .expect("flow for cancel")
        .send(())
        .await
        .expect("cancel action");

    #[cfg(feature = "test-utils")]
    {
        let final_idx = controller.phase_cursor().index();
        assert!(
            final_idx > initial_idx,
            "cursor should advance after local action"
        );
    }

    // CanonicalControl returns Canonical outcome with registered token
    assert!(matches!(outcome, ControlOutcome::Canonical(_)));

    drop(controller);
}

/// Test checkpoint/rollback as local actions (self-send) via unified flow().send() API.
/// CanonicalControl self-send means Controller makes local checkpoint/rollback decisions.
/// This test verifies typestate advances through the sequence.
#[tokio::test]
async fn checkpoint_rollback_local_actions_advance_typestate() {
    let tap_storage = leak_tap_storage();
    let slab = leak_slab(2048);
    let config = Config::new(tap_storage, slab);
    let transport = TestTransport::default();
    let rendezvous = Rendezvous::from_config(config, transport.clone());
    let cluster: &mut Cluster = Box::leak(Box::new(SessionCluster::new(support::leak_clock())));
    let rv_id = cluster
        .add_rendezvous(rendezvous)
        .expect("register rendezvous");

    let sid = SessionId::new(9);

    {
        let bootstrap = cluster
            .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_BOOTSTRAP_PROGRAM, NoBinding)
            .expect("bootstrap attach");
        drop(bootstrap);
    }

    // Self-send: only Controller participates in CheckpointMsg/RollbackMsg
    let controller = cluster
        .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_CHECKPOINT_PROGRAM, NoBinding)
        .expect("attach controller");

    #[cfg(feature = "test-utils")]
    let mut last_idx = controller.phase_cursor().index();

    // Unified API: flow().send(()) for CanonicalControl
    let (controller, checkpoint_outcome) = controller
        .flow::<CheckpointMsg>()
        .expect("flow for checkpoint")
        .send(())
        .await
        .expect("checkpoint action");

    #[cfg(feature = "test-utils")]
    {
        let mid_idx = controller.phase_cursor().index();
        assert!(mid_idx > last_idx, "cursor should advance after checkpoint");
        last_idx = mid_idx;
    }
    assert!(matches!(checkpoint_outcome, ControlOutcome::Canonical(_)));

    let (controller, rollback_outcome) = controller
        .flow::<RollbackMsg>()
        .expect("flow for rollback")
        .send(())
        .await
        .expect("rollback action");

    #[cfg(feature = "test-utils")]
    {
        let final_idx = controller.phase_cursor().index();
        assert!(final_idx > last_idx, "cursor should advance after rollback");
    }
    assert!(matches!(rollback_outcome, ControlOutcome::Canonical(_)));

    drop(controller);
}
