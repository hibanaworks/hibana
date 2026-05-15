//! Static configuration describing the runtime envelope Hibana operates in.
//!
//! The crate expects callers to provide everything it needs at start-up: fixed
//! buffers for rendezvous bookkeeping, observation rings, and wire payloads. By
//! storing slices rather than owning allocations we uphold the `no_alloc`
//! contract required by the crate design.

use core::{cell::Cell, fmt, marker::PhantomData, ops::Range};

use crate::observe::core::TapEvent;
use crate::runtime::consts::{DefaultLabelUniverse, LANE_DOMAIN_SIZE, LabelUniverse, RING_EVENTS};

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
    pub(crate) lane_range: Range<u16>,
    pub(crate) endpoint_slots: usize,
    universe_marker: PhantomData<U>,
    pub(crate) clock: C,
    pub(crate) liveness_policy: LivenessPolicy,
    pub(crate) operational_deadline: OperationalDeadline,
}

impl<'a, U: LabelUniverse, C: Clock> fmt::Debug for Config<'a, U, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("lane_range", &self.lane_range)
            .field("endpoint_slots", &self.endpoint_slots)
            .field("universe", &core::any::type_name::<U>())
            .field("clock", &core::any::type_name::<C>())
            .field("liveness_policy", &self.liveness_policy)
            .field("operational_deadline", &self.operational_deadline)
            .finish()
    }
}

impl<'a, U: LabelUniverse, C: Clock> Config<'a, U, C> {
    /// Construct a configuration from the complete runtime envelope.
    ///
    /// The canonical public attach path always realises control traffic on lane
    /// `0`, so the configured window must include that reserved control lane.
    /// `operational_deadline_ticks` configures an internal wait-site fuse; it is
    /// not a public timeout API and cannot select a protocol branch.
    pub fn new(
        tap_buf: &'a mut [TapEvent; RING_EVENTS],
        slab: &'a mut [u8],
        lane_range: Range<u16>,
        endpoint_slots: usize,
        clock: C,
        operational_deadline_ticks: Option<u32>,
    ) -> Self {
        Self::validate_lane_range(&lane_range);
        Self::validate_endpoint_slots(endpoint_slots);
        Self {
            tap_buf,
            slab,
            lane_range,
            endpoint_slots,
            universe_marker: PhantomData,
            clock,
            liveness_policy: LivenessPolicy::default(),
            operational_deadline: OperationalDeadline::from_optional_ticks(
                operational_deadline_ticks,
            ),
        }
    }

    fn validate_lane_range(range: &Range<u16>) {
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
    }

    fn validate_endpoint_slots(endpoint_slots: usize) {
        assert!(
            endpoint_slots > 0,
            "endpoint slot capacity must materialize at least one projected role"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn new_rejects_windows_without_control_lane_zero() {
        let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let panic = catch_unwind(AssertUnwindSafe(|| {
            let _: Config<'_, DefaultLabelUniverse, _> = Config::new(
                &mut tap_buf,
                &mut slab,
                32..35,
                1,
                CounterClock::new(),
                None,
            );
        }));
        assert!(
            panic.is_err(),
            "public SessionKit config must reject lane windows that exclude reserved control lane 0"
        );
    }

    #[test]
    fn new_accepts_zero_based_high_lane_windows() {
        let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let config: Config<'_, DefaultLabelUniverse, _> =
            Config::new(&mut tap_buf, &mut slab, 0..35, 1, CounterClock::new(), None);
        assert_eq!(
            config.lane_range,
            0..35,
            "public SessionKit config must keep accepting zero-based high-lane windows"
        );
    }

    #[test]
    fn new_accepts_full_wire_lane_domain() {
        let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let config: Config<'_, DefaultLabelUniverse, _> = Config::new(
            &mut tap_buf,
            &mut slab,
            0..LANE_DOMAIN_SIZE,
            1,
            CounterClock::new(),
            None,
        );

        assert!(
            config.lane_range.contains(&u16::from(u8::MAX)),
            "public SessionKit config must be able to include lane 255"
        );
    }

    #[test]
    fn new_rejects_out_of_wire_domain_window() {
        let mut tap_buf = [TapEvent::zero(); RING_EVENTS];
        let mut slab = [0u8; 256];
        let panic = catch_unwind(AssertUnwindSafe(|| {
            let _: Config<'_, DefaultLabelUniverse, _> = Config::new(
                &mut tap_buf,
                &mut slab,
                0..(LANE_DOMAIN_SIZE + 1),
                1,
                CounterClock::new(),
                None,
            );
        }));

        assert!(
            panic.is_err(),
            "public SessionKit config must reject lanes outside the u8 wire domain"
        );
    }
}
