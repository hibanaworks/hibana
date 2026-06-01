use super::ControlCore;
use crate::control::cluster::core::{
    CpError, PreparedDistributedTopologyBegin, ResourceScope, SessionId, TopologyAck,
    TopologyOperands,
};
use crate::control::lease::core::RendezvousOwnerProof;

pub(crate) enum ReservedDistributedTopologyBeginCapacity {
    Inline,
    External {
        owner: RendezvousOwnerProof,
        capacity: usize,
        ptr: *mut u8,
        bytes: usize,
        reclaim_delta: usize,
    },
}

impl ReservedDistributedTopologyBeginCapacity {
    #[inline]
    const fn external(
        owner: RendezvousOwnerProof,
        capacity: usize,
        ptr: *mut u8,
        bytes: usize,
        reclaim_delta: usize,
    ) -> Self {
        Self::External {
            owner,
            capacity,
            ptr,
            bytes,
            reclaim_delta,
        }
    }
}

impl<'cfg, T, U, C, E, const MAX_RV: usize> ControlCore<'cfg, T, U, C, E, MAX_RV>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    pub(crate) fn reserve_distributed_topology_begin_capacity(
        &mut self,
        sid: SessionId,
        operands: TopologyOperands,
        owner: RendezvousOwnerProof,
    ) -> Result<ReservedDistributedTopologyBeginCapacity, CpError> {
        assert_eq!(
            owner.id(),
            operands.src_rv,
            "distributed topology begin capacity owner must match source rendezvous"
        );
        let Some((capacity, bytes, align)) = self
            .topology_state
            .begin_capacity_reservation_layout(sid, operands)?
        else {
            return Ok(ReservedDistributedTopologyBeginCapacity::Inline);
        };
        let rv = self.locals.get_mut_by_proof(owner);
        let (ptr, reclaim_delta) = rv
            .allocate_external_persistent_sidecar_bytes(bytes, align)
            .ok_or(CpError::resource_exhausted(ResourceScope::Generic))?;
        Ok(ReservedDistributedTopologyBeginCapacity::external(
            owner,
            capacity,
            ptr,
            bytes,
            reclaim_delta,
        ))
    }

    pub(crate) fn rollback_distributed_topology_begin_capacity(
        &mut self,
        reservation: ReservedDistributedTopologyBeginCapacity,
    ) {
        if let ReservedDistributedTopologyBeginCapacity::External {
            owner,
            ptr,
            bytes,
            reclaim_delta,
            ..
        } = reservation
        {
            let rv = self.locals.get_mut_by_proof(owner);
            rv.free_external_persistent_sidecar_bytes(ptr, bytes, reclaim_delta);
        }
    }

    pub(crate) fn publish_distributed_topology_begin(
        &mut self,
        reservation: ReservedDistributedTopologyBeginCapacity,
        sid: SessionId,
        operands: TopologyOperands,
    ) -> (TopologyAck, PreparedDistributedTopologyBegin) {
        if let ReservedDistributedTopologyBeginCapacity::External {
            owner,
            capacity,
            ptr,
            reclaim_delta,
            ..
        } = reservation
        {
            let rv = self.locals.get_mut_by_proof(owner);
            let rv_ptr = core::ptr::from_mut(rv);
            self.topology_state.bind_reserved_begin_capacity(
                owner.id(),
                capacity,
                ptr,
                reclaim_delta,
                |ptr, bytes, reclaim_delta| {
                    /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
                    unsafe {
                        (&mut *rv_ptr).free_external_persistent_sidecar_bytes(
                            ptr,
                            bytes,
                            reclaim_delta,
                        );
                    }
                },
            );
        }
        self.topology_state.reserve_preflighted_begin(sid, operands)
    }
}
