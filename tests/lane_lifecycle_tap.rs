#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::{
    g::{self, Msg},
    runtime::program::{RoleProgram, project},
    runtime::{Config, SessionKitStorage, ids::SessionId},
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport, 2>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

const LANE_ACQUIRE_ID: u16 = 0x0210;
const LANE_RELEASE_ID: u16 = 0x0211;

fn controller_program() -> RoleProgram<0> {
    let program = g::send::<0, 1, Msg<1, ()>>();
    project(&program)
}

fn decode_sid_lane(packed: u32) -> (u32, u16) {
    let sid = packed >> 16;
    let lane = (packed & 0xFFFF) as u16;
    (sid, lane)
}

#[test]
fn lane_lifecycle_emits_acquire_and_release_taps() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let slab_ptr = slab as *mut [u8];
        let (acquire_count, release_count, has_expected_acquire, has_expected_release) =
            with_resident_tls_ref(&SESSION_SLOT, |cluster| {
                let slab = unsafe { &mut *slab_ptr };
                let rv = cluster
                    .rendezvous(Config::from_resources(slab), transport.clone())
                    .expect("register rendezvous");
                let mut tap = rv.tap();

                let sid = SessionId::new(7);
                let controller_program = controller_program();
                {
                    let endpoint = rv
                        .session(sid)
                        .role(&controller_program)
                        .enter()
                        .expect("attach cursor");
                    core::hint::black_box(&endpoint);
                }

                let mut acquire_count = 0usize;
                let mut release_count = 0usize;
                let mut has_expected_acquire = false;
                let mut has_expected_release = false;
                for event in &mut tap {
                    if event.id() == LANE_ACQUIRE_ID {
                        acquire_count += 1;
                        let (event_sid, event_lane) = decode_sid_lane(event.arg1());
                        has_expected_acquire |=
                            event.arg0() == 1u32 && event_sid == sid.raw() && event_lane == 0;
                    } else if event.id() == LANE_RELEASE_ID {
                        release_count += 1;
                        let (event_sid, event_lane) = decode_sid_lane(event.arg1());
                        has_expected_release |=
                            event.arg0() == 1u32 && event_sid == sid.raw() && event_lane == 0;
                    }
                }
                (
                    acquire_count,
                    release_count,
                    has_expected_acquire,
                    has_expected_release,
                )
            });

        assert!(has_expected_acquire, "expected lane acquire event");
        assert!(has_expected_release, "expected lane release event");
        assert_eq!(acquire_count, 1, "expected exactly one acquire event");
        assert_eq!(release_count, 1, "expected exactly one release event");
    });
}
