//! Shared session context for one actor's logical Endpoint views.
//!
//! The actor retains the pinned cluster and rendezvous independently of its
//! view ports, so terminal I/O retirement cannot discard shared workspace access.

use core::ptr::NonNull;

use crate::{rendezvous::core::Rendezvous, transport::Transport};

/// Pinned cluster and rendezvous borrows for a resident actor.
pub(crate) struct SessionCtx<'rv, T>
where
    T: Transport,
{
    cluster: NonNull<crate::session::cluster::core::SessionCluster<'rv, T>>,
    rendezvous: &'rv Rendezvous<'rv, 'rv, T>,
}

impl<'rv, T> SessionCtx<'rv, T>
where
    T: Transport,
{
    pub(crate) fn new(
        cluster: &'rv crate::session::cluster::core::SessionCluster<'rv, T>,
        rendezvous: &'rv Rendezvous<'rv, 'rv, T>,
    ) -> Self {
        Self {
            cluster: NonNull::from(cluster),
            rendezvous,
        }
    }

    #[inline]
    pub(crate) fn rendezvous(&self) -> &'rv Rendezvous<'rv, 'rv, T> {
        self.rendezvous
    }

    #[inline]
    pub(crate) fn cluster(&self) -> &'rv crate::session::cluster::core::SessionCluster<'rv, T> {
        /* SAFETY: endpoints are constructed only from resident cluster attach,
        and the cluster outlives attached endpoint storage. */
        unsafe { self.cluster.as_ref() }
    }
}
