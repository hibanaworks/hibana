#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{cell::UnsafeCell, mem::MaybeUninit};

use common::TestTransport;
use hibana::{
    g::advanced::{CanonicalControl, RoleProgram, project},
    g::{self, Msg, Role},
    substrate::cap::{
        CapShot, ControlResourceKind, GenericCapToken, ResourceKind,
        advanced::{
            CAP_HANDLE_LEN, CapError, CapsMask, ControlHandling, ControlMint, ControlScopeKind,
            ScopeId, SessionScopedKind,
        },
    },
    substrate::{
        Lane, SessionId, SessionKit,
        binding::NoBinding,
        runtime::{Config, CounterClock, DefaultLabelUniverse},
    },
};
use runtime_support::with_fixture;
use tls_ref_support::with_tls_ref;

const LABEL_CANCEL: u8 = 60;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CancelKind;

fn encode_session_scoped_handle(sid: u32, lane: u16) -> [u8; CAP_HANDLE_LEN] {
    let mut buf = [0u8; CAP_HANDLE_LEN];
    buf[0..4].copy_from_slice(&sid.to_le_bytes());
    buf[4..6].copy_from_slice(&lane.to_le_bytes());
    buf
}

fn decode_session_scoped_handle(data: [u8; CAP_HANDLE_LEN]) -> (u32, u16) {
    let sid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let lane = u16::from_le_bytes([data[4], data[5]]);
    (sid, lane)
}

impl ResourceKind for CancelKind {
    type Handle = (u32, u16);
    const TAG: u8 = 0x45;
    const NAME: &'static str = "Cancel";
    const AUTO_MINT_EXTERNAL: bool = false;

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        encode_session_scoped_handle(handle.0, handle.1)
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok(decode_session_scoped_handle(data))
    }

    fn zeroize(_handle: &mut Self::Handle) {}

    fn caps_mask(_handle: &Self::Handle) -> CapsMask {
        CapsMask::empty()
    }

    fn scope_id(_handle: &Self::Handle) -> Option<ScopeId> {
        None
    }
}

impl SessionScopedKind for CancelKind {
    fn handle_for_session(sid: SessionId, lane: Lane) -> Self::Handle {
        (sid.raw(), lane.raw() as u16)
    }

    fn shot() -> CapShot {
        CapShot::One
    }
}

impl ControlMint for CancelKind {
    fn mint_handle(sid: SessionId, lane: Lane, _scope: ScopeId) -> Self::Handle {
        (sid.raw(), lane.raw() as u16)
    }
}

impl ControlResourceKind for CancelKind {
    const LABEL: u8 = LABEL_CANCEL;
    const SCOPE: ControlScopeKind = ControlScopeKind::Cancel;
    const TAP_ID: u16 = 0;
    const SHOT: CapShot = CapShot::One;
    const HANDLING: ControlHandling = ControlHandling::Canonical;
}

type TestKit = SessionKit<'static, TestTransport, DefaultLabelUniverse, CounterClock, 2>;
std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
    };
}

fn run_cancel_local_action_test(
    cluster: &'static TestKit,
    tap_storage: &'static mut [hibana::substrate::tap::TapEvent; runtime_support::RING_EVENTS],
    slab: &'static mut [u8],
) {
    let cancel_protocol = g::send::<
        Role<0>,
        Role<0>,
        Msg<{ LABEL_CANCEL }, GenericCapToken<CancelKind>, CanonicalControl<CancelKind>>,
        0,
    >();
    let controller_cancel_program: RoleProgram<0> = project(&cancel_protocol);
    let bootstrap_protocol = g::send::<Role<0>, Role<1>, Msg<1, u32>, 0>();
    let controller_bootstrap_program: RoleProgram<0> = project(&bootstrap_protocol);
    let config = Config::new(tap_storage, slab);
    let transport = TestTransport::default();
    let rv_id = cluster
        .add_rendezvous_from_config(config, transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(7);

    let _bootstrap = cluster
        .enter(rv_id, sid, &controller_bootstrap_program, NoBinding)
        .expect("bootstrap attach");

    let mut controller = cluster
        .enter(rv_id, sid, &controller_cancel_program, NoBinding)
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
