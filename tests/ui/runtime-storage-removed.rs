use hibana::runtime::{Clock, Config, CounterClock, RING_EVENTS, RuntimeStorage, TapEvent};

fn main() {
    let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
    let mut slab = [0u8; 64];
    let clock = CounterClock::zero();
    let storage = RuntimeStorage::from_buffers(&mut tap_buf, &mut slab);
    let _: Config<'_> = storage.into();
    let _: Config<'_> = Config::from_resources((&mut tap_buf, &mut slab), clock);
    fn _needs_clock<C: Clock>(_clock: C) {}
}
