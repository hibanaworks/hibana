use core::cell::UnsafeCell;
use hibana::substrate::{tap::TapEvent, runtime::CounterClock};

pub(crate) const RING_EVENTS: usize = 128;
const FIXTURE_SLAB_CAPACITY: usize = 262_144;

std::thread_local! {
    static FIXTURE_CLOCK: CounterClock = const { CounterClock::new() };
    static FIXTURE_TAP: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    static FIXTURE_SLAB: UnsafeCell<[u8; FIXTURE_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; FIXTURE_SLAB_CAPACITY]) };
}

pub(crate) fn with_fixture<R>(
    f: impl FnOnce(&'static CounterClock, &'static mut [TapEvent; RING_EVENTS], &'static mut [u8]) -> R,
) -> R {
    FIXTURE_CLOCK.with(|clock| {
        FIXTURE_TAP.with(|tap| {
            FIXTURE_SLAB.with(|slab| unsafe {
                let tap = &mut *tap.get();
                let slab = &mut *slab.get();
                tap.fill(TapEvent::zero());
                slab.fill(0);
                f(
                    &*(clock as *const CounterClock),
                    &mut *(tap as *mut [TapEvent; RING_EVENTS]),
                    &mut *(slab.as_mut_slice() as *mut [u8]),
                )
            })
        })
    })
}
