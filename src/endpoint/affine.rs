//! Affine lane guard used by cursor endpoints.
//!
//! The guard is the detached RAII witness for a leased lane. `LaneLease`
//! creates it only after converting the borrow-bound rendezvous lease into a
//! `Port`, so the endpoint keeps only the shorter borrow lifetime.

use core::cell::Cell;
use core::marker::PhantomData;

use crate::{
    rendezvous::core::{LaneRelease, Rendezvous},
    session::types::Lane,
    transport::Transport,
};

/// Lease-backed lane guard.
///
/// Dropping the guard releases the lane via the underlying rendezvous.
/// The raw pointer erases the longer rendezvous lifetime; the shorter endpoint
/// borrow is carried by `active_leases`.
pub(crate) struct LaneGuard<'lease, T: Transport> {
    rendezvous: *const (),
    lane: Lane,
    active_leases: &'lease Cell<u32>,
    _marker: PhantomData<fn() -> T>,
}

impl<'lease, T> LaneGuard<'lease, T>
where
    T: Transport,
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
}

impl<'lease, T> Drop for LaneGuard<'lease, T>
where
    T: Transport,
{
    fn drop(&mut self) {
        let lane = self.lane;
        if !self.rendezvous.is_null() {
            /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */
            unsafe {
                let rv = &*self.rendezvous.cast::<Rendezvous<'static, 'static, T>>();
                match rv.release_lane(lane) {
                    LaneRelease::Released(sid) => rv.emit_lane_release(sid, lane),
                    LaneRelease::StillHeld => {}
                }
            }
        }

        let current = self.active_leases.get();
        if current == 0 {
            crate::invariant();
        }
        self.active_leases.set(current - 1);
    }
}
