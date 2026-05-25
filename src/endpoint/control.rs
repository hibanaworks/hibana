//! Control-context storage for endpoints.
//!
//! Endpoints store rendezvous-scoped control context directly inside
//! [`Endpoint`](super::Endpoint) so that operations like reroute, state
//! snapshot, state restore, and abort do not require the caller to thread
//! additional parameters. This module provides the control context that carries
//! both policy configuration and a reference to the control plane.

use core::ptr::NonNull;

use crate::{control::types::Lane, transport::Transport};

/// Rendezvous-scoped control context stored inside endpoints.
///
/// This holds references to the control plane (SessionCluster), allowing
/// control operations to be invoked directly from the endpoint without
/// requiring external references.
pub(crate) struct SessionControlCtx<'rv, T, U, C, E, const MAX_RV: usize = 8>
where
    T: Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    cluster: Option<NonNull<crate::control::cluster::core::SessionCluster<'rv, T, U, C, MAX_RV>>>,
    _marker: core::marker::PhantomData<E>,
}

impl<'rv, T, U, C, E, const MAX_RV: usize> SessionControlCtx<'rv, T, U, C, E, MAX_RV>
where
    T: Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    pub(crate) fn new(
        _lane: Lane,
        cluster: Option<&'rv crate::control::cluster::core::SessionCluster<'rv, T, U, C, MAX_RV>>,
        _resolver: Option<()>,
    ) -> Self {
        Self {
            cluster: cluster.map(NonNull::from),
            _marker: core::marker::PhantomData,
        }
    }

    #[inline]
    pub(crate) fn cluster(
        &self,
    ) -> Option<&'rv crate::control::cluster::core::SessionCluster<'rv, T, U, C, MAX_RV>> {
        self.cluster.map(|ptr| /* SAFETY: this owner validates the concrete pointer identity and initialized storage before raw access. */ unsafe { ptr.as_ref() })
    }
}
