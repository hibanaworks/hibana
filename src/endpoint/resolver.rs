//! Rendezvous resolver abstractions used by reroute/delegation.
//!
//! The resolver maps a policy-level [`Locator`](crate::endpoint::delegate::Locator) to an
//! actual control-plane rendezvous handle without assuming heap allocation.
//! Implementations can keep rendezvous instances in static storage, thread-
//! local caches, or any other arena that satisfies the borrow checker.

use core::convert::Infallible;

use crate::{
    endpoint::delegate::Locator,
    rendezvous::{Rendezvous, RendezvousId},
    transport::Transport,
};

/// GAT-based handle that borrows a rendezvous instance.
#[derive(Clone, Copy)]
pub struct RendezvousHandle<'a, R> {
    rendezvous: &'a R,
}

impl<'a, R> RendezvousHandle<'a, R> {
    /// Construct a new handle from a borrowed rendezvous instance.
    #[inline]
    pub const fn new(rendezvous: &'a R) -> Self {
        Self { rendezvous }
    }

    /// Access the borrowed rendezvous instance.
    #[inline]
    pub const fn rendezvous(&self) -> &'a R {
        self.rendezvous
    }
}

impl<'a, 'rv, 'cfg, T, U, C, E> RendezvousHandle<'a, Rendezvous<'rv, 'cfg, T, U, C, E>>
where
    T: Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::EpochTable,
    'cfg: 'rv,
{
    /// Retrieve the control-plane identifier via the rendezvous.
    #[inline]
    pub fn control_plane_id(&self) -> RendezvousId {
        self.rendezvous.id()
    }
}

/// Trait for resolving a policy-level locator into a rendezvous handle.
///
/// The trait is intentionally generic over the rendezvous type `R` so that
/// resolver implementations can work with any instantiation
/// (`Rendezvous<'rv, 'cfg, T, U, C, E>`, mock rendezvous used in tests, etc.).
///
/// ```ignore
/// struct MyResolver<'a, R> {
///     primary: &'a R,
/// }
///
/// impl<'a, R> RendezvousResolver<R> for MyResolver<'a, R> {
///     type Error = ResolveError;
///
///     fn resolve<'r>(&'r self, loc: &Locator) -> Result<RendezvousHandle<'r, R>, Self::Error>
///     where
///         Self: 'r,
///     {
///         match loc.node {
///             0 => Ok(RendezvousHandle::new(self.primary)),
///             _ => Err(ResolveError::NotFound),
///         }
///     }
/// }
/// ```
pub trait RendezvousResolver<R> {
    /// Error produced when the resolver cannot map a locator.
    type Error;

    /// Resolve the given [`Locator`] into a rendezvous handle.
    fn resolve<'a>(&'a self, locator: &Locator) -> Result<RendezvousHandle<'a, R>, Self::Error>
    where
        Self: 'a;
}

/// Trivial resolver that always returns the same rendezvous reference.
///
/// This is handy for unit tests or single-rendezvous deployments where routing
/// simply keeps using the existing control plane.
pub struct SingleResolver<'a, R> {
    rendezvous: &'a R,
}

impl<'a, R> SingleResolver<'a, R> {
    /// Create a new resolver that always yields `rendezvous`.
    #[inline]
    pub const fn new(rendezvous: &'a R) -> Self {
        Self { rendezvous }
    }
}

impl<'a, R> RendezvousResolver<R> for SingleResolver<'a, R> {
    type Error = Infallible;

    #[inline]
    fn resolve<'r>(&'r self, _locator: &Locator) -> Result<RendezvousHandle<'r, R>, Self::Error>
    where
        Self: 'r,
    {
        Ok(RendezvousHandle::new(self.rendezvous))
    }
}
