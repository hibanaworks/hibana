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
    runtime::{SessionKitStorage, ids::SessionId, tap},
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn controller_program() -> RoleProgram<0> {
    let program = g::send::<0, 1, Msg<1, ()>>();
    project(&program)
}

fn decode_rv_lane(packed: u32) -> (u32, u16) {
    let rv = packed >> 16;
    let lane = (packed & 0xFFFF) as u16;
    (rv, lane)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LaneEvent {
    ts: u32,
    id: u16,
    rv: u32,
    sid: u32,
    lane: u16,
}

fn collect_lane_events(mut port: impl Iterator<Item = tap::TapEvent>) -> Vec<LaneEvent> {
    let mut events = Vec::new();
    for event in &mut port {
        if event.id() == tap::LANE_ACQUIRE || event.id() == tap::LANE_RELEASE {
            let (rv, lane) = decode_rv_lane(event.arg1());
            events.push(LaneEvent {
                ts: event.ts(),
                id: event.id(),
                rv,
                sid: event.arg0(),
                lane,
            });
        }
    }
    events
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
                    .rendezvous(slab, transport.clone())
                    .expect("register rendezvous");
                let mut tap = rv.tap();

                let sid = SessionId::new(7);
                let controller_program = controller_program();
                {
                    let endpoint = rv.enter(sid, &controller_program).expect("attach cursor");
                    core::hint::black_box(&endpoint);
                }

                let mut acquire_count = 0usize;
                let mut release_count = 0usize;
                let mut has_expected_acquire = false;
                let mut has_expected_release = false;
                for event in &mut tap {
                    if event.id() == tap::LANE_ACQUIRE {
                        acquire_count += 1;
                        let (event_rv, event_lane) = decode_rv_lane(event.arg1());
                        has_expected_acquire |=
                            event.arg0() == sid.raw() && event_rv == 1u32 && event_lane == 0;
                    } else if event.id() == tap::LANE_RELEASE {
                        release_count += 1;
                        let (event_rv, event_lane) = decode_rv_lane(event.arg1());
                        has_expected_release |=
                            event.arg0() == sid.raw() && event_rv == 1u32 && event_lane == 0;
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

#[test]
fn lane_lifecycle_keeps_full_session_id_in_evidence() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let slab_ptr = slab as *mut [u8];
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let slab = unsafe { &mut *slab_ptr };
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let controller_program = controller_program();

            for sid_raw in [0x0001_0007u32, 0x0002_0007u32] {
                let endpoint = rv
                    .enter(SessionId::new(sid_raw), &controller_program)
                    .expect("attach cursor");
                drop(endpoint);
            }

            collect_lane_events(rv.tap())
        });

        assert_eq!(
            events,
            vec![
                LaneEvent {
                    ts: 0,
                    id: tap::LANE_ACQUIRE,
                    rv: 1,
                    sid: 0x0001_0007,
                    lane: 0,
                },
                LaneEvent {
                    ts: 1,
                    id: tap::LANE_RELEASE,
                    rv: 1,
                    sid: 0x0001_0007,
                    lane: 0,
                },
                LaneEvent {
                    ts: 2,
                    id: tap::LANE_ACQUIRE,
                    rv: 1,
                    sid: 0x0002_0007,
                    lane: 0,
                },
                LaneEvent {
                    ts: 3,
                    id: tap::LANE_RELEASE,
                    rv: 1,
                    sid: 0x0002_0007,
                    lane: 0,
                },
            ],
            "lane lifecycle evidence must retain all SessionId bits"
        );
    });
}

#[test]
fn shared_sid_lane_emits_one_association_pair() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let slab_ptr = slab as *mut [u8];
        let (mid_events, final_events) = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let slab = unsafe { &mut *slab_ptr };
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let mut tap_port = rv.tap();

            let sid = SessionId::new(11);
            let controller_program = controller_program();
            let endpoint_a = rv
                .enter(sid, &controller_program)
                .expect("attach first cursor");
            let endpoint_b = rv
                .enter(sid, &controller_program)
                .expect("attach second cursor");

            drop(endpoint_a);
            let mid_events = collect_lane_events(tap_port.by_ref());
            drop(endpoint_b);
            let final_events = collect_lane_events(tap_port);
            (mid_events, final_events)
        });

        assert_eq!(
            mid_events,
            vec![LaneEvent {
                ts: 0,
                id: tap::LANE_ACQUIRE,
                rv: 1,
                sid: 11,
                lane: 0,
            }],
            "first drop must not release a shared session/lane association"
        );
        assert_eq!(
            final_events,
            vec![LaneEvent {
                ts: 1,
                id: tap::LANE_RELEASE,
                rv: 1,
                sid: 11,
                lane: 0,
            }],
            "last drop must release the shared session/lane association exactly once"
        );
    });
}

#[test]
fn new_tap_port_reads_all_runtime_events_before_wrap() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let slab_ptr = slab as *mut [u8];
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let slab = unsafe { &mut *slab_ptr };
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let controller_program = controller_program();

            for sid_raw in 100..110 {
                let endpoint = rv
                    .enter(SessionId::new(sid_raw), &controller_program)
                    .expect("attach cursor");
                drop(endpoint);
            }

            collect_lane_events(rv.tap())
        });

        assert_eq!(
            events.len(),
            20,
            "new tap port must retain pre-wrap lane events"
        );
        assert_eq!(events.first().map(|event| event.ts), Some(0));
        assert_eq!(events.last().map(|event| event.ts), Some(19));
    });
}

#[test]
fn new_tap_port_reads_latest_thirty_two_runtime_events_after_wrap() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        let slab_ptr = slab as *mut [u8];
        let events = with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            let slab = unsafe { &mut *slab_ptr };
            let rv = cluster
                .rendezvous(slab, transport.clone())
                .expect("register rendezvous");
            let controller_program = controller_program();

            let base_sid = 0x0001_0200u32;
            for sid_raw in base_sid..(base_sid + 35) {
                let endpoint = rv
                    .enter(SessionId::new(sid_raw), &controller_program)
                    .expect("attach cursor");
                drop(endpoint);
            }

            collect_lane_events(rv.tap())
        });

        assert_eq!(
            events.len(),
            32,
            "new tap port must expose only the retained 32-event window"
        );
        assert_eq!(events.first().map(|event| event.ts), Some(38));
        assert_eq!(events.last().map(|event| event.ts), Some(69));
        assert_eq!(events.first().map(|event| event.sid), Some(0x0001_0213));
        assert_eq!(events.last().map(|event| event.sid), Some(0x0001_0222));
        assert!(
            events.iter().all(|event| event.rv == 1 && event.lane == 0),
            "retained lifecycle evidence must preserve rendezvous and lane"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.id == tap::LANE_ACQUIRE)
                .count(),
            16
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.id == tap::LANE_RELEASE)
                .count(),
            16
        );
    });
}
