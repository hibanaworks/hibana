#![cfg(feature = "std")]

use hibana::control::cap::EndpointResource;
use hibana::observe::normalise::CapEventStage;
use hibana::observe::{self, normalise};

#[test]
fn cap_events_basic() {
    let sid = 42;
    let events = vec![
        observe::RawEvent::new(1, observe::cap_mint::<EndpointResource>(), sid, 0),
        observe::RawEvent::new(2, observe::cap_claim::<EndpointResource>(), sid, 0),
        observe::RawEvent::new(3, observe::cap_exhaust::<EndpointResource>(), sid, 0),
    ];

    let caps = normalise::cap_events(&events);
    assert_eq!(caps.len(), 3);

    let mint = &caps[0];
    assert!(mint.is_mint());
    assert_eq!(mint.kind, "EndpointResource");
    assert_eq!(mint.sid, sid);

    assert_eq!(caps[1].stage, CapEventStage::Claim);
    assert_eq!(caps[2].stage, CapEventStage::Exhaust);
}
