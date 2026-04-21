//! Delegation lease graph sizing.

#[cfg(test)]
use core::marker::PhantomData;

#[cfg(test)]
use crate::{
    control::{
        lease::{
            bundle::LeaseBundleFacet,
            graph::{InlineLeaseChildStorage, InlineLeaseNodeStorage, LeaseSpec},
        },
        types::RendezvousId,
    },
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

/// Maximum node capacity for [`DelegationLeaseSpec`].
pub(crate) const DELEGATION_LEASE_MAX_NODES: usize = 8;
/// Maximum child capacity for [`DelegationLeaseSpec`].
pub(crate) const DELEGATION_LEASE_MAX_CHILDREN: usize = 6;

/// LeaseGraph specification for delegation orchestration.
#[cfg(test)]
pub(crate) struct DelegationLeaseSpec<T, U, C, E>(PhantomData<(T, U, C, E)>);

#[cfg(test)]
impl<T, U, C, E> LeaseSpec for DelegationLeaseSpec<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type NodeId = RendezvousId;
    type Facet = LeaseBundleFacet<T, U, C, E>;
    type ChildStorage = InlineLeaseChildStorage<RendezvousId, DELEGATION_LEASE_MAX_CHILDREN>;
    type NodeStorage<'graph>
        = InlineLeaseNodeStorage<'graph, Self, DELEGATION_LEASE_MAX_NODES>
    where
        Self: 'graph;
    const MAX_NODES: usize = DELEGATION_LEASE_MAX_NODES;
    const MAX_CHILDREN: usize = DELEGATION_LEASE_MAX_CHILDREN;
}
