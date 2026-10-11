//! Affine lane guard used by cursor endpoints.
//!
//! The guard is the detached RAII witness for a leased lane. `LaneLease`
//! creates it only after converting the borrow-bound rendezvous lease into a
//! `Port`, so the endpoint keeps only the shorter borrow lifetime.

use core::cell::Cell;
use core::marker::PhantomData;

use crate::{
    rendezvous::core::{LaneRelease, Rendezvous},
    session::types::{Lane, SessionId},
    transport::Transport,
};

/// Lease-backed lane guard.
///
/// Dropping the guard releases the lane via the underlying rendezvous.
/// The raw pointer erases the longer rendezvous lifetime; the shorter endpoint
/// borrow is carried by `active_leases`.
pub(crate) struct LaneGuard<'lease, T: Transport> {
    rendezvous: *const (),
    sid: SessionId,
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
        sid: SessionId,
        lane: Lane,
        active_leases: &'lease Cell<u32>,
    ) -> Self {
        Self {
            rendezvous,
            sid,
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
        let sid = self.sid;
        let lane = self.lane;
        if !self.rendezvous.is_null() {
            /* SAFETY: `LaneGuard` stores the rendezvous pointer that issued the
            lane lease. Drop only needs a shared rendezvous borrow to release
            the counted lane handle and emit the release evidence. */
            unsafe {
                let rv = &*self.rendezvous.cast::<Rendezvous<'static, 'static, T>>();
                match rv.release_lane(sid, lane) {
                    LaneRelease::Released => rv.emit_lane_release(sid, lane),
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
