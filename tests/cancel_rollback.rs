#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{cell::UnsafeCell, mem::MaybeUninit};

use common::TestTransport;
use hibana::{
    g::advanced::steps::{SendStep, SeqSteps, StepCons, StepNil},
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
use runtime_support::with_fixture;
use tls_ref_support::with_tls_ref;

const LABEL_CANCEL: u8 = 60;
const LABEL_CHECKPOINT: u8 = 61;
const LABEL_ROLLBACK: u8 = 63;
type TestKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;
type CancelProtocolSteps = StepCons<
    SendStep<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>,
        0,
    >,
    StepNil,
>;
type CheckpointProtocolSteps = SeqSteps<
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
            Msg<{ LABEL_ROLLBACK }, GenericCapToken<RollbackKind>, CanonicalControl<RollbackKind>>,
            0,
        >,
        StepNil,
    >,
>;
type BootstrapProtocolSteps = StepCons<SendStep<Role<0>, Role<1>, Msg<1, u32>, 0>, StepNil>;
std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

const CANCEL_PROTOCOL: g::Program<CancelProtocolSteps> = g::send::<
    Role<0>,
    Role<0>,
    Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>,
    0,
>();

static CONTROLLER_CANCEL_PROGRAM: RoleProgram<'static, 0> = project(&CANCEL_PROTOCOL);

const CHECKPOINT_PROTOCOL: g::Program<CheckpointProtocolSteps> = g::seq(
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

static CONTROLLER_CHECKPOINT_PROGRAM: RoleProgram<'static, 0> = project(&CHECKPOINT_PROTOCOL);
const BOOTSTRAP_PROTOCOL: g::Program<BootstrapProtocolSteps> =
    g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();

static CONTROLLER_BOOTSTRAP_PROGRAM: RoleProgram<'static, 0> = project(&BOOTSTRAP_PROTOCOL);

fn run_cancel_local_action_test(
    cluster: &'static TestKit,
    tap_storage: &'static mut [hibana::substrate::mgmt::tap::TapEvent;
                     runtime_support::RING_EVENTS],
    slab: &'static mut [u8],
) {
    let config = Config::new(tap_storage, slab);
    let transport = TestTransport::default();
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(7);

    let _bootstrap = cluster
        .enter(rv_id, sid, &CONTROLLER_BOOTSTRAP_PROGRAM, NoBinding)
        .expect("bootstrap attach");

    let mut controller = cluster
        .enter(rv_id, sid, &CONTROLLER_CANCEL_PROGRAM, NoBinding)
        .expect("attach controller");
    let outcome = futures::executor::block_on(
        controller
            .flow::<Msg<
                { LABEL_CANCEL },
                GenericCapToken<CancelKind>,
                CanonicalControl<CancelKind>,
            >>()
            .expect("cancel flow")
            .send(()),
    )
    .expect("cancel action");
    assert!(outcome.is_canonical());
}

fn run_checkpoint_rollback_local_action_test(
    cluster: &'static TestKit,
    tap_storage: &'static mut [hibana::substrate::mgmt::tap::TapEvent;
                     runtime_support::RING_EVENTS],
    slab: &'static mut [u8],
) {
    let config = Config::new(tap_storage, slab);
    let transport = TestTransport::default();
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(9);

    let _bootstrap = cluster
        .enter(rv_id, sid, &CONTROLLER_BOOTSTRAP_PROGRAM, NoBinding)
        .expect("bootstrap attach");

    let mut controller = cluster
        .enter(rv_id, sid, &CONTROLLER_CHECKPOINT_PROGRAM, NoBinding)
        .expect("attach controller");
    let checkpoint_outcome = futures::executor::block_on(
        controller
            .flow::<Msg<
                { LABEL_CHECKPOINT },
                GenericCapToken<CheckpointKind>,
                CanonicalControl<CheckpointKind>,
            >>()
            .expect("checkpoint flow")
            .send(()),
    )
    .expect("checkpoint action");
    assert!(checkpoint_outcome.is_canonical());
    let rollback_outcome =
        futures::executor::block_on(
            controller
                .flow::<Msg<
                    { LABEL_ROLLBACK },
                    GenericCapToken<RollbackKind>,
                    CanonicalControl<RollbackKind>,
                >>()
                .expect("rollback flow")
                .send(()),
        )
        .expect("rollback action");
    assert!(rollback_outcome.is_canonical());
}

/// Test cancel as a local action (self-send) via unified flow().send() API.
/// CanonicalControl self-send means Controller makes the local decision.
/// This test verifies that typestate advances correctly through flow().send().
#[test]
fn cancel_local_action_advances_typestate() {
    with_fixture(|clock, tap_storage, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| run_cancel_local_action_test(cluster, tap_storage, slab),
        );
    });
}

/// Test checkpoint/rollback as local actions (self-send) via unified flow().send() API.
/// CanonicalControl self-send means Controller makes local checkpoint/rollback decisions.
/// This test verifies typestate advances through the sequence.
#[test]
fn checkpoint_rollback_local_actions_advance_typestate() {
    with_fixture(|clock, tap_storage, slab| {
        with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| run_checkpoint_rollback_local_action_test(cluster, tap_storage, slab),
        );
    });
}
