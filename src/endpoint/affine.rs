//! Affine lane guard used by cursor endpoints.
//!
//! The lane guard owns the rendezvous lease outright and ensures lane release
//! happens through the typed lease pipeline.

use core::cell::Cell;

use crate::runtime::{config::Clock, consts::LabelUniverse};
use crate::{
    control::lease::core::{FullSpec, RendezvousLease},
    control::types::Lane,
    rendezvous::core::Rendezvous,
    transport::Transport,
};

/// Lease-backed lane guard.
///
/// Dropping the guard releases the lane via the underlying rendezvous lease.
/// Always uses the default EpochTbl since LaneGuard is only used for creating new endpoints.
pub(crate) struct LaneGuard<'cfg, T: Transport, U: LabelUniverse, C: Clock> {
    pub(crate) lease:
        Option<RendezvousLease<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl, FullSpec>>,
    rendezvous: *const Rendezvous<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl>,
    lane: Lane,
    active_leases: &'cfg Cell<u32>,
    release_on_drop: bool,
}

impl<'cfg, T, U, C> LaneGuard<'cfg, T, U, C>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
{
    pub(crate) fn new(
        lease: RendezvousLease<'cfg, 'cfg, T, U, C, crate::control::cap::mint::EpochTbl, FullSpec>,
        lane: Lane,
        active_leases: &'cfg Cell<u32>,
        release_on_drop: bool,
    ) -> Self {
        Self {
            lease: Some(lease),
            rendezvous: core::ptr::null(),
            lane,
            active_leases,
            release_on_drop,
        }
    }

    pub(crate) fn detach_lease(&mut self) {
        if let Some(mut lease) = self.lease.take() {
            let mut ptr: *const Rendezvous<
                'cfg,
                'cfg,
                T,
                U,
                C,
                crate::control::cap::mint::EpochTbl,
            > = core::ptr::null();
            lease.with_rendezvous(|rv| {
                ptr = core::ptr::from_mut(rv).cast_const();
            });
            self.rendezvous = ptr;
            drop(lease);
        }
    }
}

impl<'cfg, T, U, C> Drop for LaneGuard<'cfg, T, U, C>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
{
    fn drop(&mut self) {
        if !self.release_on_drop {
            return;
        }

        let lane = self.lane;
        if let Some(mut lease) = self.lease.take() {
            lease.release_lane_with_tap(lane);
        } else if !self.rendezvous.is_null() {
            unsafe {
                let rv = &*self.rendezvous;
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
