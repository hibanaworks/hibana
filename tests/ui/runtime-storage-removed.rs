use hibana::runtime::{Config, CounterClock, RuntimeStorage, TapEvent, RING_EVENTS};

fn main() {
    let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
    let mut slab = [0u8; 64];
    let storage = RuntimeStorage::from_buffers(&mut tap_buf, &mut slab);
    let _: Config<'_, CounterClock> = storage.into();
}
