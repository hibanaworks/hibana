//! Association table and unique identifiers.
//!
//! This module provides the `UniqueId` newtype that guarantees `NoCrossLaneAliasing`.
//! The uniqueness is enforced by the rendezvous layer, which ensures that each lane
//! has a unique identifier that cannot be duplicated.

use crate::control::types::{LaneId, NoCrossLaneAliasing};
use core::marker::PhantomData;

/// Unique identifier that guarantees no cross-lane aliasing.
///
/// This newtype is only constructible by the rendezvous layer, which ensures uniqueness.
/// By encoding the invariant in the type, we can statically prevent cross-lane aliasing.
#[repr(transparent)]
pub struct UniqueId<Inv: NoCrossLaneAliasing> {
    lane: LaneId,
    _p: PhantomData<Inv>,
}

impl<Inv: NoCrossLaneAliasing> UniqueId<Inv> {
    /// Create a new unique identifier.
    ///
    /// # Safety
    ///
    /// The caller must ensure that this lane ID is truly unique and will not be
    /// aliased with any other `UniqueId` for the same invariant type `Inv`.
    ///
    /// This is enforced by the rendezvous layer's allocation strategy.
    #[cfg(test)]
    pub(crate) unsafe fn new(lane: LaneId) -> Self {
        Self {
            lane,
            _p: PhantomData,
        }
    }

    /// Get the lane identifier.
    pub fn lane(&self) -> LaneId {
        self.lane
    }
}

impl<Inv: NoCrossLaneAliasing> Clone for UniqueId<Inv> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Inv: NoCrossLaneAliasing> Copy for UniqueId<Inv> {}

impl<Inv: NoCrossLaneAliasing> core::fmt::Debug for UniqueId<Inv> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UniqueId")
            .field("lane", &self.lane)
            .finish()
    }
}

impl<Inv: NoCrossLaneAliasing> PartialEq for UniqueId<Inv> {
    fn eq(&self, other: &Self) -> bool {
        self.lane == other.lane
    }
}

impl<Inv: NoCrossLaneAliasing> Eq for UniqueId<Inv> {}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestInv;
    impl NoCrossLaneAliasing for TestInv {}

    #[test]
    fn test_unique_id() {
        let id1: UniqueId<TestInv> = unsafe { UniqueId::new(LaneId::new(1)) };
        let id2: UniqueId<TestInv> = unsafe { UniqueId::new(LaneId::new(2)) };

        assert_ne!(id1, id2);
        assert_eq!(id1.lane(), LaneId::new(1));
        assert_eq!(id2.lane(), LaneId::new(2));
    }

    #[test]
    fn test_unique_id_clone() {
        let id1: UniqueId<TestInv> = unsafe { UniqueId::new(LaneId::new(1)) };
        let id2 = id1;

        assert_eq!(id1, id2);
    }
}
