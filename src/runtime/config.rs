//! Static configuration describing the runtime envelope Hibana operates in.
//!
//! The crate expects callers to provide everything it needs at start-up: fixed
//! buffers for rendezvous bookkeeping, observation rings, and wire payloads. By
//! storing slices rather than owning allocations we uphold the `no_alloc`
//! contract required by the crate design.

use core::{cell::Cell, fmt, marker::PhantomData, ops::Range};

use crate::observe::core::TapEvent;
use crate::runtime::consts::{DefaultLabelUniverse, LabelUniverse, RING_EVENTS};

/// Clock source used to timestamp tap events.
pub trait Clock {
    fn now32(&self) -> u32;
}

/// Offer-time progress accounting for dynamic route resolution.
///
/// This is intentionally not a public knob. Offer progression is
/// evidence-driven: the endpoint either observes route evidence, remains
/// pending, or faults for a real protocol/transport cause. Hidden defer budgets
/// and synthetic poll retries must not become route authority.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OfferProgressPolicy;

/// Runtime-owned fuse for operational waits.
///
/// This is not a protocol branch and is intentionally not exposed on endpoint
/// methods. Expiry is terminal evidence: the integration poisons the current
/// session generation instead of selecting an alternate route. Protocol-visible
/// time must be represented in the choreography as a timer/clock role plus an
/// explicit route point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OperationalDeadline {
    ticks: u32,
}

impl OperationalDeadline {
    pub(crate) const DISABLED: Self = Self { ticks: u32::MAX };

    #[inline]
    pub(crate) const fn from_ticks(ticks: u32) -> Self {
        let ticks = if ticks == 0 { 1 } else { ticks };
        Self { ticks }
    }

    #[inline]
    pub(crate) const fn from_optional_ticks(ticks: Option<u32>) -> Self {
        match ticks {
            Some(ticks) => Self::from_ticks(ticks),
            None => Self::DISABLED,
        }
    }

    #[inline]
    pub(crate) const fn ticks(self) -> u32 {
        self.ticks
    }

    #[inline]
    pub(crate) const fn is_disabled(self) -> bool {
        self.ticks == u32::MAX
    }
}

impl Default for OperationalDeadline {
    fn default() -> Self {
        Self::DISABLED
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
    universe_marker: PhantomData<U>,
    pub(crate) clock: C,
    pub(crate) offer_progress_policy: OfferProgressPolicy,
}

impl<'a, U: LabelUniverse, C: Clock> fmt::Debug for Config<'a, U, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("initial_lane_range", &Self::initial_lane_range())
            .field("universe", &core::any::type_name::<U>())
            .field("clock", &core::any::type_name::<C>())
            .field("offer_progress_policy", &self.offer_progress_policy)
            .finish()
    }
}

impl<'a, U: LabelUniverse, C: Clock> Config<'a, U, C> {
    /// Borrow the runtime resources used by attach.
    ///
    /// Runtime sizing that follows from the projected program is derived by the
    /// attach path. Callers provide only the storage/clock envelope; they do not
    /// choose lane windows, endpoint slot counts, or operational deadline fuses.
    /// Wait-site fuses belong to the transport/substrate owner and are read from
    /// the transport when a rendezvous is materialized.
    pub fn from_resources(
        tap_buf: &'a mut [TapEvent; RING_EVENTS],
        slab: &'a mut [u8],
        clock: C,
    ) -> Self {
        Self {
            tap_buf,
            slab,
            universe_marker: PhantomData,
            clock,
            offer_progress_policy: OfferProgressPolicy,
        }
    }

    /// Empty lane domain materialized before a projected role descriptor exists.
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
        let _: Config<'_, DefaultLabelUniverse, _> =
            Config::from_resources(&mut tap_buf, &mut slab, CounterClock::new());
        let lane_range = Config::<DefaultLabelUniverse, CounterClock>::initial_lane_range();
        assert_eq!(lane_range, 0..0);
    }
}
