//! Session-context storage for endpoints.
//!
//! Endpoints store rendezvous-scoped session context directly inside
//! [`Endpoint`](super::Endpoint) so that session faulting, route resolution,
//! and rendezvous-owned endpoint release remain bound to the resident session
//! cluster.

use core::ptr::NonNull;

use crate::{session::types::Lane, transport::Transport};

/// Rendezvous-scoped session context stored inside endpoints.
pub(crate) struct SessionCtx<'rv, T>
where
    T: Transport,
{
    cluster: NonNull<crate::session::cluster::core::SessionCluster<'rv, T>>,
}

impl<'rv, T> SessionCtx<'rv, T>
where
    T: Transport,
{
    pub(crate) fn new(
        _lane: Lane,
        cluster: &'rv crate::session::cluster::core::SessionCluster<'rv, T>,
    ) -> Self {
        Self {
            cluster: NonNull::from(cluster),
        }
    }

    #[inline]
    pub(crate) fn cluster(&self) -> &'rv crate::session::cluster::core::SessionCluster<'rv, T> {
        /* SAFETY: endpoints are constructed only from resident cluster attach,
        and the cluster outlives attached endpoint storage. */
        unsafe { self.cluster.as_ref() }
    }
}
