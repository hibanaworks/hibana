#![cfg(feature = "std")]

mod common;
#[path = "support/runtime.rs"]
mod runtime_support;

use common::TestTransport;
use hibana::{
    g::advanced::steps::StepNil,
    g::advanced::{RoleProgram, project},
    g::{self},
    substrate::{
        Lane, SessionCluster, SessionId,
        binding::NoBinding,
        mgmt::session::tap::TapEvent,
        runtime::{Config, DefaultLabelUniverse},
    },
};
use runtime_support::{RING_EVENTS, leak_clock, leak_slab, leak_tap_storage};

const PROGRAM: g::Program<StepNil> = StepNil::PROGRAM;

static CONTROLLER_PROGRAM: RoleProgram<'static, 0, StepNil> = project(&PROGRAM);

const LANE_ACQUIRE_ID: u16 = 0x0210;
const LANE_RELEASE_ID: u16 = 0x0211;

fn decode_sid_lane(packed: u32) -> (u32, u16) {
    let sid = packed >> 16;
    let lane = (packed & 0xFFFF) as u16;
    (sid, lane)
}

#[test]
fn lease_observe_tracks_lane_lifecycle() {
    // Prepare cluster and rendezvous with test transport.
    let cluster: &'static SessionCluster<
        'static,
        TestTransport,
        DefaultLabelUniverse,
        hibana::substrate::runtime::CounterClock,
        2,
    > = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let transport = TestTransport::default();
    let tap_buf = leak_tap_storage();
    let tap_buf_ptr: *const [TapEvent; RING_EVENTS] = tap_buf;
    let rv_id = cluster
        .add_rendezvous_from_config(Config::new(tap_buf, leak_slab(1024)), transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(7);
    // Lane 0 is always active (primary lane)
    let lane = Lane::new(0);

    {
        let endpoint = cluster
            .enter::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
            .expect("attach cursor");
        drop(endpoint);
    }

    let storage = unsafe { &*tap_buf_ptr };
    let events: Vec<TapEvent> = storage
        .iter()
        .copied()
        .filter(|event| event.id != 0)
        .collect();
    let expected_rv = rv_id.raw() as u32;
    let expected_sid = sid.raw();
    let expected_lane = lane.raw() as u16;
    let acquire_count = events
        .iter()
        .filter(|event| event.id == LANE_ACQUIRE_ID)
        .count();
    let release_count = events
        .iter()
        .filter(|event| event.id == LANE_RELEASE_ID)
        .count();
    let has_expected_acquire = events.iter().any(|event| {
        if event.id != LANE_ACQUIRE_ID {
            return false;
        }
        let (event_sid, event_lane) = decode_sid_lane(event.arg1);
        event.arg0 == expected_rv && event_sid == expected_sid && event_lane == expected_lane
    });
    let has_expected_release = events.iter().any(|event| {
        if event.id != LANE_RELEASE_ID {
            return false;
        }
        let (event_sid, event_lane) = decode_sid_lane(event.arg1);
        event.arg0 == expected_rv && event_sid == expected_sid && event_lane == expected_lane
    });

    assert!(
        has_expected_acquire,
        "expected lane acquire event, got {:?}",
        events
    );
    assert!(
        has_expected_release,
        "expected lane release event, got {:?}",
        events
    );
    assert_eq!(acquire_count, 1, "expected exactly one acquire event");
    assert_eq!(release_count, 1, "expected exactly one release event");
}
