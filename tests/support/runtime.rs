use core::cell::UnsafeCell;
use hibana::runtime::{CounterClock, TapEvent};

pub(crate) const RING_EVENTS: usize = 128;
const TEST_SLAB_CAPACITY: usize = 1_048_576;

std::thread_local! {
    static TEST_CLOCK: CounterClock = const { CounterClock::zero() };
    static TEST_TAP: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
    static TEST_SLAB: UnsafeCell<[u8; TEST_SLAB_CAPACITY]> =
        const { UnsafeCell::new([0u8; TEST_SLAB_CAPACITY]) };
}

pub(crate) fn with_runtime_workspace<R>(
    f: impl FnOnce(&'static CounterClock, &'static mut [TapEvent; RING_EVENTS], &'static mut [u8]) -> R,
) -> R {
    TEST_CLOCK.with(|clock| {
        TEST_TAP.with(|tap| {
            TEST_SLAB.with(|slab| unsafe {
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
