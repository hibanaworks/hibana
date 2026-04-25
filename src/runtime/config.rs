//! Static configuration describing the runtime envelope Hibana operates in.
//!
//! The crate expects callers to provide everything it needs at start-up: fixed
//! buffers for rendezvous bookkeeping, observation rings, and wire payloads. By
//! storing slices rather than owning allocations we uphold the `no_alloc`
//! contract required by the crate design.

use core::{cell::Cell, fmt, ops::Range};

use crate::observe::core::TapEvent;
use crate::runtime::consts::{
    DefaultLabelUniverse, LANE_DOMAIN_SIZE, LANES_MAX, LabelUniverse, RING_EVENTS,
};

/// Clock source used to timestamp tap events.
pub trait Clock {
    fn now32(&self) -> u32;
}

/// Offer-time liveness policy for dynamic route resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LivenessPolicy {
    pub(crate) max_defer_per_offer: u8,
    pub(crate) max_no_evidence_defer: u8,
    pub(crate) force_poll_on_exhaustion: bool,
    pub(crate) max_forced_poll_attempts: u8,
    pub(crate) exhaust_reason: u16,
}

impl Default for LivenessPolicy {
    fn default() -> Self {
        Self {
            max_defer_per_offer: 8,
            max_no_evidence_defer: 1,
            force_poll_on_exhaustion: true,
            max_forced_poll_attempts: 1,
            exhaust_reason: crate::policy_runtime::ENGINE_LIVENESS_EXHAUSTED,
        }
    }
}

/// Monotonic counter clock suitable for `no_std` deployments.
///
/// By default, this clock provides saturating monotonic behavior: it increments
/// on each call and saturates at `u32::MAX` without wrapping. This ensures
/// tap timestamps remain non-decreasing in `no_std` environments.
///
/// Host environments may inject wrap-aware monotonic clocks via the Clock trait
/// for environments that handle wrap-around appropriately.
pub struct CounterClock {
    counter: Cell<u32>,
}

impl CounterClock {
    pub const fn new() -> Self {
        Self {
            counter: Cell::new(0),
        }
    }
}

impl Default for CounterClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for CounterClock {
    /// Returns a monotonically non-decreasing tick counter.
    ///
    /// The counter increments on each call and saturates at `u32::MAX`
    /// (no wrapping). This default behavior ensures tap timestamps remain
    /// non-decreasing without wrap-around in `no_std` deployments.
    fn now32(&self) -> u32 {
        let current = self.counter.get();
        let next = current.saturating_add(1);
        self.counter.set(next);
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
pub struct Config<'a, U: LabelUniverse = DefaultLabelUniverse, C: Clock = CounterClock> {
    pub(crate) tap_buf: &'a mut [TapEvent; RING_EVENTS],
    pub(crate) slab: &'a mut [u8],
    pub(crate) lane_range: Range<u16>,
    universe: U,
    pub(crate) clock: C,
    pub(crate) liveness_policy: LivenessPolicy,
}

impl<'a, U: LabelUniverse, C: Clock> fmt::Debug for Config<'a, U, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("lane_range", &self.lane_range)
            .field("universe", &core::any::type_name::<U>())
            .field("clock", &core::any::type_name::<C>())
            .field("liveness_policy", &self.liveness_policy)
            .finish()
    }
}

impl<'a> Config<'a, DefaultLabelUniverse, CounterClock> {
    /// Construct a default configuration using the canonical label universe.
    pub fn new(tap_buf: &'a mut [TapEvent; RING_EVENTS], slab: &'a mut [u8]) -> Self {
        Self {
            tap_buf,
            slab,
            lane_range: 0..LANES_MAX,
            universe: DefaultLabelUniverse,
            clock: CounterClock::new(),
            liveness_policy: LivenessPolicy::default(),
        }
    }
}

impl<'a, U: LabelUniverse, C: Clock> Config<'a, U, C> {
    /// Returns the tap storage for rendezvous instantiation.
    pub fn tap_storage(&mut self) -> &mut [TapEvent; RING_EVENTS] {
        self.tap_buf
    }

    /// Returns the backing slab used for DMA/SHM views.
    pub fn slab(&mut self) -> &mut [u8] {
        self.slab
    }

    /// Configures the subset of lanes allowed for rendezvous operations.
    ///
    /// The canonical public attach path always realises control traffic on lane
    /// `0`, so the configured window must include that reserved control lane.
    pub fn with_lane_range(mut self, range: Range<u16>) -> Self {
        assert!(
            range.start <= range.end,
            "lane range {:?} must satisfy start <= end",
            range
        );
        assert!(
            range.start == 0 && range.end > 0,
            "lane range {:?} must include reserved control lane 0 for SessionKit attach",
            range
        );
        assert!(
            range.end <= LANE_DOMAIN_SIZE,
            "lane range {:?} must stay inside the u8 wire lane domain",
            range
        );
        self.lane_range = range;
        self
    }

    /// Access the lane range currently configured.
    pub fn lane_range(&self) -> Range<u16> {
        self.lane_range.clone()
    }

    /// Swap the label universe while reusing owned buffers.
    pub fn with_universe<V: LabelUniverse>(self, universe: V) -> Config<'a, V, C> {
        Config {
            tap_buf: self.tap_buf,
            slab: self.slab,
            lane_range: self.lane_range,
            universe,
            clock: self.clock,
            liveness_policy: self.liveness_policy,
        }
    }

    /// Borrow the active label universe.
    pub fn universe(&self) -> &U {
        &self.universe
    }

    /// Replace the clock source with a custom implementation.
    pub fn with_clock<D: Clock>(self, clock: D) -> Config<'a, U, D> {
        Config {
            tap_buf: self.tap_buf,
            slab: self.slab,
            lane_range: self.lane_range,
            universe: self.universe,
            clock,
            liveness_policy: self.liveness_policy,
        }
    }

    /// Access the configured clock.
    pub fn clock(&self) -> &C {
        &self.clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn with_lane_range_rejects_windows_without_control_lane_zero() {
        let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let panic = catch_unwind(AssertUnwindSafe(|| {
            let _ = Config::new(&mut tap_buf, &mut slab).with_lane_range(32..35);
        }));
        assert!(
            panic.is_err(),
            "public SessionKit config must reject lane windows that exclude reserved control lane 0"
        );
    }

    #[test]
    fn with_lane_range_accepts_zero_based_high_lane_windows() {
        let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let config = Config::new(&mut tap_buf, &mut slab).with_lane_range(0..35);
        assert_eq!(
            config.lane_range(),
            0..35,
            "public SessionKit config must keep accepting zero-based high-lane windows"
        );
    }

    #[test]
    fn with_lane_range_accepts_full_wire_lane_domain() {
        let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let config = Config::new(&mut tap_buf, &mut slab).with_lane_range(0..LANE_DOMAIN_SIZE);

        assert!(
            config.lane_range().contains(&u16::from(u8::MAX)),
            "public SessionKit config must be able to include lane 255"
        );
    }

    #[test]
    fn with_lane_range_rejects_out_of_wire_domain_window() {
        let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let panic = catch_unwind(AssertUnwindSafe(|| {
            let _ = Config::new(&mut tap_buf, &mut slab).with_lane_range(0..(LANE_DOMAIN_SIZE + 1));
        }));

        assert!(
            panic.is_err(),
            "public SessionKit config must reject lanes outside the u8 wire domain"
        );
    }
}
