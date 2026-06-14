//! Runtime configuration describing the runtime envelope Hibana operates in.
//!
//! The crate expects callers to provide everything it needs at start-up: fixed
//! buffers for rendezvous bookkeeping, observation rings, and wire payloads. By
//! storing slices rather than owning allocations we uphold the `no_alloc`
//! contract required by the crate design.

use core::{cell::Cell, fmt, ops::Range};

use crate::observe::core::TapEvent;
use crate::runtime_core::consts::RING_EVENTS;

/// Clock source used to timestamp tap events.
pub trait Clock {
    fn now32(&self) -> u32;
}

/// Monotonic counter clock suitable for `no_std` deployments.
///
/// This clock increments on each call, stops at `u32::MAX`, and never wraps.
/// This keeps tap timestamps non-decreasing in `no_std` environments.
///
/// Host environments may inject wrap-aware monotonic clocks via the Clock trait
/// for environments that handle wrap-around appropriately.
pub struct CounterClock {
    counter: Cell<u32>,
}

impl CounterClock {
    pub const fn zero() -> Self {
        Self {
            counter: Cell::new(0),
        }
    }
}

impl Clock for CounterClock {
    /// Returns a monotonically non-decreasing tick counter.
    ///
    /// The counter increments on each call, stops at `u32::MAX`, and never
    /// wraps.
    fn now32(&self) -> u32 {
        let current = self.counter.get();
        if current != u32::MAX {
            self.counter.set(current + 1);
        }
        current
    }
}

impl<C: Clock + ?Sized> Clock for &C {
    fn now32(&self) -> u32 {
        (**self).now32()
    }
}

impl fmt::Debug for CounterClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CounterClock").finish()
    }
}

/// Borrowed resources required by the runtime.
pub struct Config<'a, C: Clock = CounterClock> {
    pub(crate) tap_buf: &'a mut [TapEvent; RING_EVENTS],
    pub(crate) slab: &'a mut [u8],
    pub(crate) clock: C,
}

impl<'a, C: Clock> fmt::Debug for Config<'a, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("initial_lane_range", &Self::initial_lane_range())
            .field("clock", &core::any::type_name::<C>())
            .finish()
    }
}

impl<'a, C: Clock> Config<'a, C> {
    /// Borrow the runtime resources used by attach.
    ///
    /// Runtime sizing that follows from the projected program is derived by the
    /// attach path. Callers provide only the storage/clock envelope; they do not
    /// choose lane windows, endpoint slot counts, or hidden wait fuses.
    pub fn from_resources(
        resources: (&'a mut [TapEvent; RING_EVENTS], &'a mut [u8]),
        clock: C,
    ) -> Self {
        Self {
            tap_buf: resources.0,
            slab: resources.1,
            clock,
        }
    }

    /// Zero-width lane domain materialized before a projected role descriptor exists.
    ///
    /// Lane legality and lane storage sizing are owned by projection metadata.
    /// Public config therefore starts with no materialized lane slots; endpoint
    /// attach expands the rendezvous to the role descriptor's lane span.
    pub(crate) fn initial_lane_range() -> Range<u16> {
        0..0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_defer_lane_domain_until_projected_descriptor() {
        let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let _: Config<'_, _> =
            Config::from_resources((&mut tap_buf, &mut slab), CounterClock::zero());
        let lane_range = Config::<CounterClock>::initial_lane_range();
        assert_eq!(lane_range, 0..0);
    }
}
