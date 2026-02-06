//! Control-context scaffolding for endpoints (B+ execution plan).
//!
//! The typestate rewrite stores rendezvous-scoped control context directly
//! inside [`Endpoint`](super::Endpoint) so that operations like reroute,
//! checkpoint, rollback, and cancel no longer require the caller to thread
//! additional parameters.  This module provides the control context that
//! carries both policy configuration and a reference to the control plane.

use core::{any::Any, ptr::NonNull};

use crate::{
    control::{
        CapRegisteredToken,
        cap::{GenericCapToken, ResourceKind},
    },
    rendezvous::RendezvousId,
    transport::Transport,
};

/// Rendezvous-scoped control context stored inside endpoints.
///
/// This holds references to the control plane (SessionCluster), allowing
/// control operations to be invoked directly from the endpoint without
/// requiring external references.
pub struct SessionControlCtx<'rv, T, U, C, E, const MAX_RV: usize = 8>
where
    T: Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::EpochTable,
{
    _rendezvous_id: RendezvousId,
    cluster: Option<NonNull<crate::runtime::SessionCluster<'rv, T, U, C, MAX_RV>>>,
    transport_handle: Option<&'rv (dyn Any + Send + Sync)>,
    _marker: core::marker::PhantomData<E>,
}

/// Outcome of a control payload emission.
#[derive(Debug)]
pub enum ControlOutcome<'rv, K: ResourceKind> {
    /// No control payload was transmitted (pure data message).
    None,
    /// Canonical control payload minted locally.
    Canonical(CapRegisteredToken<'rv, K>),
    /// External control payload supplied by the caller.
    External(GenericCapToken<K>),
}

impl<'rv, T, U, C, E, const MAX_RV: usize> SessionControlCtx<'rv, T, U, C, E, MAX_RV>
where
    T: Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::EpochTable,
{
    pub fn new(
        rendezvous_id: RendezvousId,
        _lane: crate::rendezvous::Lane,
        cluster: Option<&'rv crate::runtime::SessionCluster<'rv, T, U, C, MAX_RV>>,
        _resolver: Option<()>,
    ) -> Self {
        Self {
            _rendezvous_id: rendezvous_id,
            cluster: cluster.map(NonNull::from),
            transport_handle: None,
            _marker: core::marker::PhantomData,
        }
    }

    #[inline]
    pub fn cluster(&self) -> Option<&'rv crate::runtime::SessionCluster<'rv, T, U, C, MAX_RV>> {
        self.cluster.map(|ptr| unsafe { ptr.as_ref() })
    }

    /// Install a transport command handle (type-erased) for dispatcher integration.
    pub fn install_transport_handle<H>(&mut self, handle: &'rv H)
    where
        H: Any + Send + Sync,
    {
        self.transport_handle = Some(handle);
    }

    /// Retrieve a previously installed transport command handle of type `T`.
    pub fn transport_handle<H>(&self) -> Option<&H>
    where
        H: Any + Send + Sync,
    {
        self.transport_handle
            .and_then(|raw| raw.downcast_ref::<H>())
    }

    /// Clear any installed transport command handle.
    pub fn clear_transport_handle(&mut self) {
        self.transport_handle = None;
    }
}
