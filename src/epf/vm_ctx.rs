//! Type-safe VM execution context with affine handle tracking.
//!
//! This module provides `VmCtx<'ctx, Spec, Obs>`, which uses compile-time
//! handle tracking.
//!
//! # Design Principles
//!
//! 1. **Pure borrow**: VmCtx owns nothing, only borrows from HandleBag/TraceLedger/TapWriter
//! 2. **Affine transitions**: Accessing a handle consumes it from the Spec
//! 3. **No runtime errors**: Missing handles → compile error
//! 4. **Zero overhead**: All tracking is compile-time

use crate::{
    control::{
        cap::{CapsMask, GenericCapToken, ResourceKind},
        handle::{
            bag::{BagStorage, HandleBag},
            spec::{Cons, HandleSpecList, Nil},
        },
    },
    observe::{
        AssociationSnapshot, TapEvent,
        scope::{ScopeTrace, tap_scope},
    },
    rendezvous::{Lane, SessionId},
    transport::TransportSnapshot,
};
use core::marker::PhantomData;

use super::Slot;
use super::dispatch::{RaOp, SyscallError};

/// Type-safe VM execution context.
///
/// # Type Parameters
///
/// - `'ctx`: Lifetime of all borrowed resources
/// - `Spec`: Type-level handle specification (Nil or Cons<K, Tail>)
/// - `Obs`: Observation state (for future extensibility)
///
/// # Affine Semantics
///
/// Accessing a handle transitions the Spec:
/// ```ignore
/// VmCtx<'ctx, Cons<K, Tail>, Obs>
///   .with_token::<K>()
///   → (GenericCapToken<K>, VmCtx<'ctx, Tail, Obs>)
/// ```
pub struct VmCtx<'ctx, Spec, Obs>
where
    Spec: HandleSpecList + BagStorage<'ctx>,
{
    /// Execution slot (Forward/EndpointRx/EndpointTx/Rendezvous)
    pub slot: Slot,
    /// Current tap event
    pub event: &'ctx TapEvent,
    /// Association snapshot (optional)
    pub assoc: Option<&'ctx AssociationSnapshot>,
    /// Capability mask for this invocation
    pub caps: CapsMask,
    /// Session identifier (optional)
    pub session: Option<SessionId>,
    /// Lane identifier (optional)
    pub lane: Option<Lane>,
    /// Scope trace associated with the tap event
    pub scope: Option<ScopeTrace>,
    /// Handle bag (type-level tracking)
    handles: HandleBag<'ctx, Spec>,
    /// Observation state
    _obs: PhantomData<Obs>,
    /// Transport metrics snapshot
    transport: TransportSnapshot,
}

/// Zero-sized observation state marker.
///
/// This can be extended in the future to carry observation-specific state.
pub struct NoObs;

impl<'ctx, Spec> VmCtx<'ctx, Spec, NoObs>
where
    Spec: HandleSpecList + BagStorage<'ctx>,
{
    /// Create a new VM context with the given handle bag.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let bag = HandleBag::new();
    /// let ctx = VmCtx::new(slot, event, caps, bag);
    /// ```
    #[inline]
    pub fn new(
        slot: Slot,
        event: &'ctx TapEvent,
        caps: CapsMask,
        handles: HandleBag<'ctx, Spec>,
    ) -> Self {
        Self {
            slot,
            event,
            assoc: None,
            caps,
            session: None,
            lane: None,
            scope: tap_scope(event),
            handles,
            _obs: PhantomData,
            transport: TransportSnapshot::default(),
        }
    }

    /// Attach session identifier.
    #[inline]
    pub fn with_session(mut self, session: SessionId) -> Self {
        self.session = Some(session);
        self
    }

    /// Attach lane identifier.
    #[inline]
    pub fn with_lane(mut self, lane: Lane) -> Self {
        self.lane = Some(lane);
        self
    }

    /// Attach scope trace metadata.
    #[inline]
    pub fn with_scope(mut self, scope: Option<ScopeTrace>) -> Self {
        self.scope = scope;
        self
    }

    /// Attach association snapshot.
    #[inline]
    pub fn with_assoc(mut self, assoc: &'ctx AssociationSnapshot) -> Self {
        self.assoc = Some(assoc);
        self
    }

    /// Attach transport metrics snapshot.
    #[inline]
    pub fn with_transport(mut self, snapshot: TransportSnapshot) -> Self {
        self.transport = snapshot;
        self
    }

    /// Validate that the VM is authorised to emit the given effect.
    #[inline]
    pub fn ensure_effect(&self, call: RaOp) -> Result<RaOp, SyscallError> {
        super::dispatch::ensure_allowed(self.slot, self.caps, call)
    }

    /// Returns `true` when the current capability set allows the specified effect.
    #[inline]
    pub fn effect_allowed(&self, call: RaOp) -> bool {
        self.ensure_effect(call).is_ok()
    }

    /// Inspect the attached transport metrics snapshot.
    #[inline]
    pub fn transport_snapshot(&self) -> TransportSnapshot {
        self.transport
    }

    /// Scope trace metadata when available.
    #[inline]
    pub fn scope_trace(&self) -> Option<ScopeTrace> {
        self.scope
    }
}

impl<'ctx, Obs> VmCtx<'ctx, Nil, Obs> {
    #[inline(always)]
    pub fn into_bag(self) -> HandleBag<'ctx, Nil> {
        self.handles
    }
}

impl<'ctx, K, Tail, Obs> VmCtx<'ctx, Cons<K, Tail>, Obs>
where
    K: ResourceKind,
    Tail: HandleSpecList + BagStorage<'ctx>,
{
    /// Consume the head token and hand it to the closure.
    #[inline]
    pub fn with_token<R>(
        self,
        f: impl FnOnce(GenericCapToken<K>, VmCtx<'ctx, Tail, Obs>) -> R,
    ) -> R {
        let VmCtx {
            slot,
            event,
            assoc,
            caps,
            session,
            lane,
            scope,
            handles,
            _obs,
            transport,
        } = self;

        handles.with_token(|token, tail_bag| {
            let tail_ctx = VmCtx {
                slot,
                event,
                assoc,
                caps,
                session,
                lane,
                scope,
                handles: tail_bag,
                _obs,
                transport,
            };
            f(token, tail_ctx)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::handle::bag::HandleBag;
    use crate::observe::{RawEvent, TapEvent};

    #[test]
    fn vm_ctx_empty_spec() {
        static EVENT: TapEvent = RawEvent::zero();
        let bag = HandleBag::new();
        let _ctx = VmCtx::new(Slot::Forward, &EVENT, CapsMask::empty(), bag);
        // Compiles → success
    }

    #[test]
    fn vm_ctx_with_builders() {
        static EVENT: TapEvent = RawEvent::zero();
        let bag = HandleBag::new();
        let _ctx = VmCtx::new(Slot::Forward, &EVENT, CapsMask::empty(), bag)
            .with_session(SessionId::new(42))
            .with_lane(Lane::new(1));
        // Compiles → success
    }
}
