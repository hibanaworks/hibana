use hibana::runtime::{Clock, CounterClock, RING_EVENTS, RuntimeStorage};

fn main() {
    let mut slab = [0u8; 64];
    let clock = CounterClock::zero();
    let _ = RING_EVENTS;
    let _ = RuntimeStorage::from_buffers(&mut slab);
    fn _needs_clock<C: Clock>(_clock: C) {}
    _needs_clock(clock);
}
