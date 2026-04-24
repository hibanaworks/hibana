//! Affine lane guard used by cursor endpoints.
//!
//! The guard is the detached RAII witness for a leased lane. `LaneLease`
//! creates it only after converting the borrow-bound rendezvous lease into a
//! `Port`, so the endpoint keeps only the shorter borrow lifetime.

use core::cell::Cell;
use core::marker::PhantomData;

use crate::runtime::{config::Clock, consts::LabelUniverse};
use crate::{
    control::cap::mint::EpochTbl, control::types::Lane, rendezvous::core::Rendezvous,
    transport::Transport,
};

/// Lease-backed lane guard.
///
/// Dropping the guard releases the lane via the underlying rendezvous.
/// The raw pointer erases the longer rendezvous lifetime; the shorter endpoint
/// borrow is carried by `active_leases`.
pub(crate) struct LaneGuard<'lease, T: Transport, U: LabelUniverse, C: Clock> {
    rendezvous: *const (),
    lane: Lane,
    active_leases: &'lease Cell<u32>,
    _marker: PhantomData<fn() -> (T, U, C)>,
}

impl<'lease, T, U, C> LaneGuard<'lease, T, U, C>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
{
    pub(crate) fn new_detached(
        rendezvous: *const (),
        lane: Lane,
        active_leases: &'lease Cell<u32>,
    ) -> Self {
        Self {
            rendezvous,
            lane,
            active_leases,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn detach_rendezvous(&mut self) {
        self.rendezvous = core::ptr::null();
    }
}

impl<'lease, T, U, C> Drop for LaneGuard<'lease, T, U, C>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
{
    fn drop(&mut self) {
        let lane = self.lane;
        if !self.rendezvous.is_null() {
            unsafe {
                let rv = &*self
                    .rendezvous
                    .cast::<Rendezvous<'static, 'static, T, U, C, EpochTbl>>();
                if let Some(sid) = rv.release_lane(lane) {
                    rv.emit_lane_release(sid, lane);
                }
            }
        }

        let current = self.active_leases.get();
        debug_assert!(current > 0, "lane_release underflow");
        self.active_leases.set(current.saturating_sub(1));
    }
}
