//! Control-context scaffolding for endpoints (B+ execution plan).
//!
//! The typestate rewrite stores rendezvous-scoped control context directly
//! inside [`Endpoint`](super::Endpoint) so that operations like reroute,
//! checkpoint, rollback, and cancel no longer require the caller to thread
//! additional parameters.  This module provides the control context that
//! carries both policy configuration and a reference to the control plane.

use core::ptr::NonNull;

use crate::{
    control::{
        cap::mint::{GenericCapToken, ResourceKind},
        cap::typed_tokens::CapRegisteredToken,
        types::{Lane, RendezvousId},
    },
    transport::Transport,
};

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
    rendezvous_id: RendezvousId,
    cluster: Option<NonNull<crate::control::cluster::core::SessionCluster<'rv, T, U, C, MAX_RV>>>,
    liveness_policy: crate::runtime::config::LivenessPolicy,
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

impl<'rv, K: ResourceKind> ControlOutcome<'rv, K> {
    #[inline]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    #[inline]
    pub const fn is_canonical(&self) -> bool {
        matches!(self, Self::Canonical(_))
    }

    #[inline]
    pub const fn is_external(&self) -> bool {
        matches!(self, Self::External(_))
    }

    #[inline]
    pub fn into_canonical(self) -> Option<CapRegisteredToken<'rv, K>> {
        match self {
            Self::Canonical(token) => Some(token),
            _ => None,
        }
    }

    #[inline]
    pub fn into_external(self) -> Option<GenericCapToken<K>> {
        match self {
            Self::External(token) => Some(token),
            _ => None,
        }
    }
}

impl<'rv, T, U, C, E, const MAX_RV: usize> SessionControlCtx<'rv, T, U, C, E, MAX_RV>
where
    T: Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    pub(crate) fn new(
        rendezvous_id: RendezvousId,
        _lane: Lane,
        cluster: Option<&'rv crate::control::cluster::core::SessionCluster<'rv, T, U, C, MAX_RV>>,
        liveness_policy: crate::runtime::config::LivenessPolicy,
        _resolver: Option<()>,
    ) -> Self {
        Self {
            rendezvous_id,
            cluster: cluster.map(NonNull::from),
            liveness_policy,
            _marker: core::marker::PhantomData,
        }
    }

    #[inline]
    pub(crate) fn cluster(
        &self,
    ) -> Option<&'rv crate::control::cluster::core::SessionCluster<'rv, T, U, C, MAX_RV>> {
        self.cluster.map(|ptr| unsafe { ptr.as_ref() })
    }

    #[inline]
    pub(crate) fn rendezvous_id(&self) -> RendezvousId {
        self.rendezvous_id
    }

    #[inline]
    pub(crate) fn liveness_policy(&self) -> crate::runtime::config::LivenessPolicy {
        self.liveness_policy
    }
}
