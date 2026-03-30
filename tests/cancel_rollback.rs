#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::{
    g::advanced::steps::{ProjectRole, SendStep, SeqSteps, StepCons, StepNil},
    g::advanced::{CanonicalControl, RoleProgram, project},
    g::{self, Msg, Role},
    substrate::cap::{
        GenericCapToken,
        advanced::{CancelKind, CheckpointKind, RollbackKind},
    },
    substrate::{
        SessionId, SessionKit,
        binding::NoBinding,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
    },
};
use runtime_support::{leak_slab, leak_tap_storage};

const LABEL_CANCEL: u8 = 60;
const LABEL_CHECKPOINT: u8 = 61;
const LABEL_ROLLBACK: u8 = 63;

const CANCEL_PROTOCOL: g::Program<
    StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>,
            0,
        >,
        StepNil,
    >,
> = g::send::<
    Role<0>,
    Role<0>,
    Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>,
    0,
>();

static CONTROLLER_CANCEL_PROGRAM: RoleProgram<
    'static,
    0,
    <StepCons<
        SendStep<
            Role<0>,
            Role<0>,
            Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>,
            0,
        >,
        StepNil,
    > as ProjectRole<Role<0>>>::Output,
> = project(&CANCEL_PROTOCOL);

const CHECKPOINT_PROTOCOL: g::Program<
    SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_CHECKPOINT },
                    GenericCapToken<CheckpointKind>,
                    CanonicalControl<CheckpointKind>,
                >,
                0,
            >,
            StepNil,
        >,
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROLLBACK },
                    GenericCapToken<RollbackKind>,
                    CanonicalControl<RollbackKind>,
                >,
                0,
            >,
            StepNil,
        >,
    >,
> = g::seq(
    g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { LABEL_CHECKPOINT },
            GenericCapToken<CheckpointKind>,
            CanonicalControl<CheckpointKind>,
        >,
        0,
    >(),
    g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_ROLLBACK }, GenericCapToken<RollbackKind>, CanonicalControl<RollbackKind>>,
        0,
    >(),
);

static CONTROLLER_CHECKPOINT_PROGRAM: RoleProgram<
    'static,
    0,
    <SeqSteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_CHECKPOINT },
                    GenericCapToken<CheckpointKind>,
                    CanonicalControl<CheckpointKind>,
                >,
                0,
            >,
            StepNil,
        >,
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_ROLLBACK },
                    GenericCapToken<RollbackKind>,
                    CanonicalControl<RollbackKind>,
                >,
                0,
            >,
            StepNil,
        >,
    > as ProjectRole<Role<0>>>::Output,
> = project(&CHECKPOINT_PROTOCOL);
const BOOTSTRAP_PROTOCOL: g::Program<
    StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil>,
> = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();

static CONTROLLER_BOOTSTRAP_PROGRAM: RoleProgram<
    'static,
    0,
    <StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil> as ProjectRole<Role<0>>>::Output,
> = project(&BOOTSTRAP_PROTOCOL);

/// Test cancel as a local action (self-send) via unified flow().send() API.
/// CanonicalControl self-send means Controller makes the local decision.
/// This test verifies that typestate advances correctly through flow().send().
#[tokio::test]
async fn cancel_local_action_advances_typestate() {
    let tap_storage = leak_tap_storage();
    let slab = leak_slab(2048);
    let config = Config::new(tap_storage, slab);
    let transport = TestTransport::default();
    let cluster: &mut SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4> =
        Box::leak(Box::new(SessionKit::new(runtime_support::leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(7);

    {
        let bootstrap = cluster
            .enter::<0, _, _, _>(rv_id, sid, &CONTROLLER_BOOTSTRAP_PROGRAM, NoBinding)
            .expect("bootstrap attach");
        drop(bootstrap);
    }

    // Self-send: only Controller participates in CancelMsg
    let controller = cluster
        .enter::<0, _, _, _>(rv_id, sid, &CONTROLLER_CANCEL_PROGRAM, NoBinding)
        .expect("attach controller");

    // Unified API: flow().send(()) for CanonicalControl (auto-minted token)
    let (controller, outcome) = controller
        .flow::<Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>>()
        .expect("flow for cancel")
        .send(())
        .await
        .expect("cancel action");

    // CanonicalControl returns Canonical outcome with registered token
    assert!(outcome.is_canonical());
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
    let cluster: &mut SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4> =
        Box::leak(Box::new(SessionKit::new(runtime_support::leak_clock())));
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(9);

    {
        let bootstrap = cluster
            .enter::<0, _, _, _>(rv_id, sid, &CONTROLLER_BOOTSTRAP_PROGRAM, NoBinding)
            .expect("bootstrap attach");
        drop(bootstrap);
    }

    // Self-send: only Controller participates in CheckpointMsg/RollbackMsg
    let controller = cluster
        .enter::<0, _, _, _>(rv_id, sid, &CONTROLLER_CHECKPOINT_PROGRAM, NoBinding)
        .expect("attach controller");

    // Unified API: flow().send(()) for CanonicalControl
    let (controller, checkpoint_outcome) =
        controller
            .flow::<Msg<
                { LABEL_CHECKPOINT },
                GenericCapToken<CheckpointKind>,
                CanonicalControl<CheckpointKind>,
            >>()
            .expect("flow for checkpoint")
            .send(())
            .await
            .expect("checkpoint action");
    assert!(checkpoint_outcome.is_canonical());

    let (controller, rollback_outcome) = controller
        .flow::<
            Msg<{ LABEL_ROLLBACK }, GenericCapToken<RollbackKind>, CanonicalControl<RollbackKind>>,
        >()
        .expect("flow for rollback")
        .send(())
        .await
        .expect("rollback action");

    assert!(rollback_outcome.is_canonical());
    drop(controller);
}
