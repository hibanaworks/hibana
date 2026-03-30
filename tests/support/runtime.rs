use hibana::substrate::{mgmt::tap::TapEvent, runtime::CounterClock};
use std::boxed::Box;

pub(crate) const RING_EVENTS: usize = 2048;

pub(crate) fn leak_tap_storage() -> &'static mut [TapEvent; RING_EVENTS] {
    let storage: Box<[TapEvent]> = vec![TapEvent::default(); RING_EVENTS].into_boxed_slice();
    let storage: Box<[TapEvent; RING_EVENTS]> = storage.try_into().expect("ring events length");
    Box::leak(storage)
}

pub(crate) fn leak_slab(size: usize) -> &'static mut [u8] {
    Box::leak(vec![0u8; size].into_boxed_slice())
}

pub(crate) fn leak_clock() -> &'static CounterClock {
    Box::leak(Box::new(CounterClock::new()))
}
