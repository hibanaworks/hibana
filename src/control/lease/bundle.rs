//! LeaseGraph facet bundle combining topology contexts.

use core::marker::PhantomData;

use crate::{
    control::types::RendezvousId,
    control::{
        automaton::topology::TopologyGraphContext,
        lease::{core::ControlCore, graph::LeaseFacet},
    },
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

struct RuntimeFacetMarker<T, U, C, E> {
    _transport: PhantomData<fn() -> T>,
    _universe: PhantomData<fn() -> U>,
    _clock: PhantomData<fn() -> C>,
    _epoch: PhantomData<fn() -> E>,
}

impl<T, U, C, E> Copy for RuntimeFacetMarker<T, U, C, E> {}

impl<T, U, C, E> Clone for RuntimeFacetMarker<T, U, C, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, U, C, E> RuntimeFacetMarker<T, U, C, E> {
    const fn new() -> Self {
        Self {
            _transport: PhantomData,
            _universe: PhantomData,
            _clock: PhantomData,
            _epoch: PhantomData,
        }
    }
}

/// Facet marker used by LeaseGraph nodes that require bundling.
pub(crate) struct LeaseBundleFacet<T, U, C, E>(RuntimeFacetMarker<T, U, C, E>)
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable;

impl<T, U, C, E> Copy for LeaseBundleFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
}

impl<T, U, C, E> Clone for LeaseBundleFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, U, C, E> Default for LeaseBundleFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn default() -> Self {
        Self(RuntimeFacetMarker::new())
    }
}

/// Per-node bundle stored in LeaseGraph when using [`LeaseBundleFacet`].
pub(crate) struct LeaseBundleContext<'ctx, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    topology: Option<TopologyGraphContext>,
    _lease_marker: PhantomData<(&'ctx (), &'cfg ())>,
    _marker: RuntimeFacetMarker<T, U, C, E>,
}

impl<'ctx, 'cfg, T, U, C, E> Default for LeaseBundleContext<'ctx, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<'ctx, 'cfg, T, U, C, E> LeaseBundleContext<'ctx, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            topology: None,
            _lease_marker: PhantomData,
            _marker: RuntimeFacetMarker::new(),
        }
    }

    #[inline]
    pub(crate) fn set_topology(&mut self, ctx: TopologyGraphContext) {
        self.topology = Some(ctx);
    }

    #[inline]
    pub(crate) fn topology(&mut self) -> Option<&mut TopologyGraphContext> {
        self.topology.as_mut()
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn populate_local(
        &mut self,
        _rendezvous: &mut crate::rendezvous::core::Rendezvous<'ctx, 'cfg, T, U, C, E>,
    ) where
        'cfg: 'ctx,
    {
        self.set_topology(TopologyGraphContext::default());
    }

    #[inline]
    pub(crate) fn on_commit(&mut self) {
        if let Some(ctx) = self.topology.as_mut() {
            ctx.clear();
        }
    }

    #[inline]
    pub(crate) fn on_rollback(&mut self) {
        if let Some(ctx) = self.topology.as_mut() {
            ctx.clear();
        }
    }
}

impl<'cfg, T, U, C, E> LeaseBundleContext<'cfg, 'cfg, T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline]
    #[cfg(test)]
    pub(crate) fn from_control_core<const MAX_RV: usize>(
        core: &mut ControlCore<'cfg, T, U, C, E, MAX_RV>,
        rv_id: RendezvousId,
    ) -> Option<Self> {
        let mut ctx = Self::new();
        if core.get_mut(&rv_id).is_some() {
            ctx.set_topology(TopologyGraphContext::default());
            Some(ctx)
        } else {
            None
        }
    }

    #[inline]
    pub(crate) fn from_control_core_or_default<const MAX_RV: usize>(
        core: &mut ControlCore<'cfg, T, U, C, E, MAX_RV>,
        rv_id: RendezvousId,
    ) -> Self {
        let mut ctx = Self::new();
        if core.get_mut(&rv_id).is_some() {
            ctx.set_topology(TopologyGraphContext::default());
        }
        ctx
    }
}

impl<T, U, C, E> LeaseFacet for LeaseBundleFacet<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type Context<'ctx> = LeaseBundleContext<'ctx, 'ctx, T, U, C, E>;

    #[inline]
    fn on_commit<'ctx>(&self, ctx: &mut Self::Context<'ctx>) {
        ctx.on_commit();
    }

    #[inline]
    fn on_rollback<'ctx>(&self, ctx: &mut Self::Context<'ctx>) {
        ctx.on_rollback();
    }
}

#[cfg(all(test, hibana_repo_tests))]
mod tests;
