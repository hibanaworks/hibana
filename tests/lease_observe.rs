#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{cell::UnsafeCell, mem::MaybeUninit};

use common::TestTransport;
use hibana::{
    g::{self, Msg, Role},
    substrate::program::{RoleProgram, project},
    substrate::{
        SessionKit,
        binding::NoBinding,
        ids::{Lane, SessionId},
        runtime::{Config, DefaultLabelUniverse},
        tap::TapEvent,
    },
};
use runtime_support::{RING_EVENTS, with_fixture};
use tls_ref_support::with_tls_ref;

type TestKit = SessionKit<
    'static,
    TestTransport,
    DefaultLabelUniverse,
    hibana::substrate::runtime::CounterClock,
    2,
>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<MaybeUninit<TestKit>> = const {
        UnsafeCell::new(MaybeUninit::uninit())
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
fn lease_observe_tracks_lane_lifecycle() {
    with_fixture(|clock, tap_buf, slab| {
        let transport = TestTransport::default();
        let tap_ptr = tap_buf as *mut [TapEvent; runtime_support::RING_EVENTS];
        let slab_ptr = slab as *mut [u8];
        let (expected_rv, expected_sid, expected_lane) = with_tls_ref(
            &SESSION_SLOT,
            |ptr| unsafe {
                ptr.write(SessionKit::new(clock));
            },
            |cluster| {
                let tap_buf = unsafe { &mut *tap_ptr };
                let slab = unsafe { &mut *slab_ptr };
                let rv_id = cluster
                    .add_rendezvous_from_config(Config::new(tap_buf, slab), transport.clone())
                    .expect("register rendezvous");

                let sid = SessionId::new(7);
                let lane = Lane::new(0);
                let controller_program = controller_program();
                let endpoint = cluster
                    .enter(rv_id, sid, &controller_program, NoBinding)
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
