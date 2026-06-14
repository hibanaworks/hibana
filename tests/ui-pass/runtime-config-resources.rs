use hibana::runtime::{Config, CounterClock, RING_EVENTS, TapEvent};

fn main() {
    let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
    let mut slab = [0u8; 64];
    let _ = Config::from_resources((&mut tap_buf, &mut slab[..]), CounterClock::zero());

    let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
    let mut slab = [0u8; 64];
    let _ = Config::from_resources((&mut tap_buf, &mut slab), CounterClock::zero());
}
