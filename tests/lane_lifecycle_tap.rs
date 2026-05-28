#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::{
    g::{self, Msg, Role},
    integration::program::{RoleProgram, project},
    integration::{
        SessionKitStorage,
        ids::{Lane, SessionId},
        runtime::{Config, TapEvent},
    },
};
use runtime_support::{RING_EVENTS, with_fixture};
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<
    'static,
    TestTransport,
    hibana::integration::runtime::DefaultLabelUniverse,
    hibana::integration::runtime::CounterClock,
    2,
>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

const LANE_ACQUIRE_ID: u16 = 0x0210;
const LANE_RELEASE_ID: u16 = 0x0211;

fn controller_program() -> RoleProgram<0> {
    let program = g::send::<Role<0>, Role<1>, Msg<1, ()>, 0>();
    project(&program)
}

fn decode_sid_lane(packed: u32) -> (u32, u16) {
    let sid = packed >> 16;
    let lane = (packed & 0xFFFF) as u16;
    (sid, lane)
}

#[test]
fn lane_lifecycle_emits_acquire_and_release_taps() {
    with_fixture(|_clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let tap_ptr = tap_buf as *mut [TapEvent; runtime_support::RING_EVENTS];
        let slab_ptr = slab as *mut [u8];
        let (expected_rv, expected_sid, expected_lane) = with_resident_tls_ref(
            &SESSION_SLOT,
            |cluster| {
                let tap_buf = unsafe { &mut *tap_ptr };
                let slab = unsafe { &mut *slab_ptr };
                let rv_id = cluster
                    .add_rendezvous_from_config(
                        Config::<hibana::integration::runtime::DefaultLabelUniverse, _>::from_resources((tap_buf, slab), hibana::integration::runtime::CounterClock::new()),
                        transport.clone(),
                    )
                    .expect("register rendezvous");

                let sid = SessionId::new(7);
                let lane = Lane::new(0);
                let controller_program = controller_program();
                let endpoint = cluster
                    .rendezvous(rv_id)
                    .session(sid)
                    .role(&controller_program)
                    .enter(None)
                    .expect("attach cursor");
                core::hint::black_box(&endpoint);

                (rv_id.raw() as u32, sid.raw(), lane.raw() as u16)
            },
        );

        let events = unsafe { &*tap_ptr };
        let mut acquire_count = 0usize;
        let mut release_count = 0usize;
        let mut has_expected_acquire = false;
        let mut has_expected_release = false;
        let mut idx = 0usize;
        while idx < RING_EVENTS {
            let event = events[idx];
            if event.id == LANE_ACQUIRE_ID {
                acquire_count += 1;
                let (event_sid, event_lane) = decode_sid_lane(event.arg1);
                has_expected_acquire |= event.arg0 == expected_rv
                    && event_sid == expected_sid
                    && event_lane == expected_lane;
            } else if event.id == LANE_RELEASE_ID {
                release_count += 1;
                let (event_sid, event_lane) = decode_sid_lane(event.arg1);
                has_expected_release |= event.arg0 == expected_rv
                    && event_sid == expected_sid
                    && event_lane == expected_lane;
            }
            idx += 1;
        }

        assert!(has_expected_acquire, "expected lane acquire event");
        assert!(has_expected_release, "expected lane release event");
        assert_eq!(acquire_count, 1, "expected exactly one acquire event");
        assert_eq!(release_count, 1, "expected exactly one release event");
    });
}
