use super::{TAP_EVENTS, TapRing};
use crate::observe::event::{TapEvent, TapRecord};

#[test]
fn lagging_reader_reconstructs_the_exact_resident_window() {
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    let mut reader = ring.port();

    let written = 70u32;
    for ordinal in 0..written {
        ring.push(TapEvent::new(u32::MAX, 7, 8, ordinal, !ordinal));
    }

    for ordinal in written - TAP_EVENTS as u32..written {
        let event = reader.next().expect("resident tap record");
        assert_eq!(event.ts(), ordinal);
        assert_eq!(event.id(), 7);
        assert_eq!(event.causal_key(), 8);
        assert_eq!(event.arg0(), ordinal);
        assert_eq!(event.arg1(), !ordinal);
    }
    assert!(reader.next().is_none());
}

#[test]
fn head_wrap_preserves_the_exact_resident_window() {
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    ring.ring.head.set(usize::MAX - 1);

    for ordinal in 0..TAP_EVENTS as u32 + 2 {
        ring.push(TapEvent::new(0, 7, 8, ordinal, !ordinal));
    }

    let events = ring.port().collect::<std::vec::Vec<_>>();
    assert_eq!(events.len(), TAP_EVENTS);
    for (event, ordinal) in events.iter().zip(2..TAP_EVENTS as u32 + 2) {
        assert_eq!(event.ts(), u32::MAX);
        assert_eq!(event.arg0(), ordinal);
        assert_eq!(event.arg1(), !ordinal);
    }
}
