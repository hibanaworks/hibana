#![cfg(feature = "std")]

mod common;
mod support;

use common::TestTransport;
use hibana::{
    NoBinding,
    g::{self, StepNil},
    observe::normalise::{self, LaneEvent},
    rendezvous::{Lane, Rendezvous, SessionId as RendezvousSessionId},
    runtime::{SessionCluster, config::Config, consts::DefaultLabelUniverse},
};
use support::{leak_clock, leak_slab, leak_tap_storage};

type Cluster = SessionCluster<
    'static,
    TestTransport,
    DefaultLabelUniverse,
    hibana::runtime::config::CounterClock,
    2,
>;

const PROGRAM: g::Program<StepNil> = g::Program::empty();

static CONTROLLER_PROGRAM: g::RoleProgram<'static, 0, StepNil> =
    g::project::<0, StepNil, _>(&PROGRAM);

#[test]
fn lease_observe_tracks_lane_lifecycle() {
    // Prepare cluster and rendezvous with test transport.
    let cluster: &'static Cluster = Box::leak(Box::new(SessionCluster::new(leak_clock())));
    let transport = TestTransport::default();

    let rendezvous: Rendezvous<
        '_,
        '_,
        TestTransport,
        DefaultLabelUniverse,
        hibana::runtime::config::CounterClock,
    > = Rendezvous::from_config(
        Config::new(leak_tap_storage(), leak_slab(1024)),
        transport.clone(),
    );

    let rv_id = cluster
        .add_rendezvous(rendezvous)
        .expect("register rendezvous");

    let sid = RendezvousSessionId::new(7);
    // Lane 0 is always active (primary lane)
    let lane = Lane::new(0);

    // Capture the initial tap head for later diffing.
    let tap = cluster
        .get_local(&rv_id)
        .expect("rendezvous registered")
        .tap();
    let start_head = tap.head();

    {
        let endpoint = cluster
            .attach_cursor::<0, _, _, _>(rv_id, sid, &CONTROLLER_PROGRAM, NoBinding)
            .expect("attach cursor");
        drop(endpoint);
    }

    // Touch the rendezvous again to ensure interior mutability paths are quiesced.
    let _ = cluster
        .get_local(&rv_id)
        .expect("rendezvous registered")
        .tap();

    let end_head = tap.head();
    let storage = tap.as_slice();
    let events = normalise::lane_trace(storage, start_head, end_head);

    let expected_acquire = LaneEvent::Acquire {
        rv: rv_id.raw() as u32,
        sid: sid.raw(),
        lane: lane.raw() as u16,
    };
    let expected_release = LaneEvent::Release {
        rv: rv_id.raw() as u32,
        sid: sid.raw(),
        lane: lane.raw() as u16,
    };

    assert!(
        events.contains(&expected_acquire),
        "expected lane acquire event, got {:?}",
        events
    );
    assert!(
        events.contains(&expected_release),
        "expected lane release event, got {:?}",
        events
    );

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, LaneEvent::Acquire { .. }))
            .count(),
        1,
        "expected exactly one acquire event"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, LaneEvent::Release { .. }))
            .count(),
        1,
        "expected exactly one release event"
    );
}
