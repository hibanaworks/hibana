//! Static configuration describing the runtime envelope Hibana operates in.
//!
//! The crate expects callers to provide everything it needs at start-up: fixed
//! buffers for rendezvous bookkeeping, observation rings, and wire payloads. By
//! storing slices rather than owning allocations we uphold the `no_alloc`
//! contract required by the crate design.

use core::{cell::Cell, fmt, ops::Range};

use crate::observe::TapEvent;
use crate::runtime::consts::{DefaultLabelUniverse, LANES_MAX, LabelUniverse, RING_EVENTS};

/// Clock source used to timestamp tap events.
pub trait Clock {
    fn now32(&self) -> u32;
}

/// Monotonic counter clock suitable for `no_std` deployments.
///
/// By default, this clock provides saturating monotonic behavior: it increments
/// on each call and saturates at `u32::MAX` without wrapping. This ensures
/// tap timestamps remain non-decreasing in `no_std` environments.
///
/// Host environments may inject wrap-aware monotonic clocks via the Clock trait
/// for environments that handle wrap-around appropriately. The observe module
/// provides timestamp checkers for development/testing (see `observe::install_ts_checker`
/// when `cfg(test)` or future `cfg(feature="trace")`).
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
    tap_buf: &'a mut [TapEvent; RING_EVENTS],
    slab: &'a mut [u8],
    lane_range: Range<u8>,
    universe: U,
    clock: C,
    global_tap: bool,
}

impl<'a, U: LabelUniverse, C: Clock> fmt::Debug for Config<'a, U, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("lane_range", &self.lane_range)
            .field("universe", &core::any::type_name::<U>())
            .field("clock", &core::any::type_name::<C>())
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
            global_tap: false,
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
    /// Callers should stay within `[0, LANES_MAX)` to match the rendezvous
    /// bookkeeping layout.
    pub fn with_lane_range(mut self, range: Range<u8>) -> Self {
        assert!(
            range.start <= range.end && range.end <= LANES_MAX,
            "lane range {:?} must be within [0, {})",
            range,
            LANES_MAX
        );
        self.lane_range = range;
        self
    }

    /// Access the lane range currently configured.
    pub fn lane_range(&self) -> Range<u8> {
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
            global_tap: self.global_tap,
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
            global_tap: self.global_tap,
        }
    }

    /// Access the configured clock.
    pub fn clock(&self) -> &C {
        &self.clock
    }

    pub(crate) fn into_parts(self) -> ConfigParts<'a, U, C> {
        ConfigParts {
            tap_buf: self.tap_buf,
            slab: self.slab,
            lane_range: self.lane_range,
            universe: self.universe,
            clock: self.clock,
            global_tap: self.global_tap,
        }
    }

    /// Enable global tap registration (requires the backing buffers to be `'static`).
    pub fn enable_global_tap(mut self) -> Self {
        self.global_tap = true;
        self
    }
}

pub(crate) struct ConfigParts<'a, U: LabelUniverse, C: Clock> {
    pub(crate) tap_buf: &'a mut [TapEvent; RING_EVENTS],
    pub(crate) slab: &'a mut [u8],
    pub(crate) lane_range: Range<u8>,
    pub(crate) universe: U,
    pub(crate) clock: C,
    pub(crate) global_tap: bool,
}
