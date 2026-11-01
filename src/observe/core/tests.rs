use super::{TAP_EVENTS, TapRing};
use crate::observe::event::{TapEvent, TapRecord};

#[test]
fn producer_checkpoints_are_independent_of_reader_consumption() {
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    let mut first = ring.port();
    assert_eq!(first.checkpoint(), Some(0));
    ring.push(TapEvent::new(0, 7, 8, 9, 10));
    assert_eq!(first.checkpoint(), Some(1));
    assert_eq!(first.checkpoint(), Some(1));
    let mut second = ring.port();
    assert_eq!(first.next().unwrap().ts(), 0);
    assert_eq!(first.checkpoint(), Some(1));
    assert_eq!(second.next().unwrap().ts(), 0);
    assert!(first.next().is_none());
    ring.push(TapEvent::new(0, 7, 8, 11, 12));
    assert_eq!(first.checkpoint(), Some(2));
    assert_eq!(second.checkpoint(), Some(2));
    assert_eq!(first.next().unwrap().ts(), 1);
    assert_eq!(second.next().unwrap().ts(), 1);
}

#[test]
fn checkpoints_reveal_the_produced_prefix_even_after_ring_overwrite() {
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    let mut reader = ring.port();
    for ordinal in 0..TAP_EVENTS + 1 {
        ring.push(TapEvent::new(0, 7, 8, ordinal as u32, 0));
    }
    assert_eq!(reader.checkpoint(), Some((TAP_EVENTS + 1) as u32));
    assert_eq!(reader.next().unwrap().ts(), 1);
    assert_eq!(reader.count(), TAP_EVENTS - 1);
}

#[test]
fn an_exhausted_checkpoint_never_returns_a_small_ordinal_after_counter_wrap() {
    let mut storage = [TapRecord::zero(); TAP_EVENTS];
    let ring = TapRing::from_storage(&mut storage);
    let reader = ring.port();
    ring.ring.head.set(u32::MAX as usize - 1);
    assert_eq!(reader.checkpoint(), Some(u32::MAX - 1));
    ring.push(TapEvent::new(0, 7, 8, 9, 10));
    assert_eq!(reader.checkpoint(), None);
    ring.ring.head.set(usize::MAX);
    assert_eq!(reader.checkpoint(), None);
    ring.push(TapEvent::new(0, 7, 8, 9, 10));
    assert_eq!(ring.ring.head.get(), 0);
    assert_eq!(reader.checkpoint(), None);
}

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
