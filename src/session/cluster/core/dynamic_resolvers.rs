use super::{ClusterError, PhantomData, RendezvousId, fmt};
// # Unsafe Owner Contract
//
// This file owns dynamic resolver erased-storage dispatch for the session
// cluster. Resolver registration records one typed state pointer together with
// the matching trampoline; invocation must use the recorded trampoline for that
// exact `(scope, resolver id)` slot. The resident resolver table is supplied by
// the cluster storage owner and bound/rebound only through the explicit
// storage-layout paths in this module. Slot contents are initialized optional
// bucket entries, and the raw state pointer is never exposed outside the
// resolver dispatch boundary.

mod bucket;
pub(crate) use bucket::ResolverBucket;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionArm {
    Left,
    Right,
}

impl DecisionArm {
    #[inline]
    pub(crate) const fn index(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolverOp {
    Reject,
    ResolveDecision,
    SetResolver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolverErrorKind {
    Reject,
    Cluster(ClusterError),
}

/// Semantic fail-closed error returned by resolver setup and dynamic resolvers.
///
/// A resolver error is diagnostic evidence. It rejects the resolver step and
/// does not grant route authority to transport hints, payload labels, or caller
/// alternate route logic.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ResolverError {
    pub(crate) op: ResolverOp,
    kind: ResolverErrorKind,
}

impl fmt::Debug for ResolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("ResolverError");
        debug.field("operation", &self.op_name());
        debug.field("kind", &self.kind).finish()
    }
}

impl ResolverError {
    #[inline]
    pub fn reject() -> Self {
        Self {
            op: ResolverOp::Reject,
            kind: ResolverErrorKind::Reject,
        }
    }

    #[inline]
    pub(crate) fn cluster(error: ClusterError) -> Self {
        Self {
            op: ResolverOp::SetResolver,
            kind: ResolverErrorKind::Cluster(error),
        }
    }

    #[inline]
    pub(crate) const fn with_operation(mut self, op: ResolverOp) -> Self {
        self.op = op;
        self
    }

    #[inline]
    const fn op_name(&self) -> &'static str {
        match self.op {
            ResolverOp::Reject => "reject",
            ResolverOp::ResolveDecision => "resolve_decision",
            ResolverOp::SetResolver => "set_resolver",
        }
    }
}

impl From<ClusterError> for ResolverError {
    #[inline]
    fn from(error: ClusterError) -> Self {
        Self::cluster(error)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DecisionResolverStatePayload<S> {
    state: *const S,
    pub(crate) resolver: fn(&S) -> Result<DecisionArm, ResolverError>,
}

#[derive(Clone, Copy)]
struct DecisionResolverStorage {
    payload: DecisionResolverStatePayload<()>,
}

impl DecisionResolverStorage {
    #[inline]
    fn erase<S>(payload: DecisionResolverStatePayload<S>) -> Self {
        const {
            assert!(
                core::mem::size_of::<DecisionResolverStatePayload<S>>()
                    == core::mem::size_of::<DecisionResolverStatePayload<()>>()
            );
            assert!(
                core::mem::align_of::<DecisionResolverStatePayload<S>>()
                    == core::mem::align_of::<DecisionResolverStatePayload<()>>()
            );
        }
        Self {
            payload: unsafe {
                /* SAFETY: the payload layout is asserted above; the typed trampoline restores S before invocation. */
                core::mem::transmute_copy(&payload)
            },
        }
    }

    #[inline]
    unsafe fn restore<S>(self) -> DecisionResolverStatePayload<S> {
        const {
            assert!(
                core::mem::size_of::<DecisionResolverStatePayload<S>>()
                    == core::mem::size_of::<DecisionResolverStatePayload<()>>()
            );
            assert!(
                core::mem::align_of::<DecisionResolverStatePayload<S>>()
                    == core::mem::align_of::<DecisionResolverStatePayload<()>>()
            );
        }
        unsafe {
            /* SAFETY: `erase::<S>` stored the payload together with the matching typed trampoline. */
            core::mem::transmute_copy(&self.payload)
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ErasedResolverRef<'cfg> {
    storage: DecisionResolverStorage,
    dispatch: unsafe fn(DecisionResolverStorage) -> Result<DecisionArm, ResolverError>,
    _marker: PhantomData<&'cfg ()>,
}

impl<'cfg> ErasedResolverRef<'cfg> {
    #[inline]
    pub(crate) fn resolve_decision(self) -> Result<DecisionArm, ResolverError> {
        /* SAFETY: resolver storage is registered in the cluster table and borrowed only through the resolver slot owner. */
        unsafe {
            (self.dispatch)(self.storage)
                .map_err(|error| error.with_operation(ResolverOp::ResolveDecision))
        }
    }
}

/// Resolver-id typed dynamic resolver handle.
///
/// The const `RESOLVER_ID` is part of the public handle type so a resolver for one
/// choreography resolver point cannot be registered at another resolver point by
/// accident. The cluster table stores an erased copy internally after
/// `set_resolver::<RESOLVER_ID>(...)` has checked the type.
#[derive(Clone, Copy)]
pub struct ResolverRef<'cfg, const RESOLVER_ID: u16> {
    inner: ErasedResolverRef<'cfg>,
}

impl<'cfg, const RESOLVER_ID: u16> ResolverRef<'cfg, RESOLVER_ID> {
    #[inline]
    pub fn decision_state<S: 'cfg>(
        state: &'cfg S,
        resolver: fn(&S) -> Result<DecisionArm, ResolverError>,
    ) -> Self {
        let payload = DecisionResolverStatePayload {
            state: core::ptr::from_ref(state),
            resolver,
        };
        Self {
            inner: ErasedResolverRef {
                storage: DecisionResolverStorage::erase(payload),
                dispatch: dispatch_decision_state::<S>,
                _marker: PhantomData,
            },
        }
    }

    /// Decide through this typed resolver without erasing its resolver id.
    ///
    /// This is for typed resolver owners and resolver combinators. It does not
    /// commit route/session progress. It does not expose the erased storage
    /// handle.
    #[inline]
    pub fn decide(self) -> Result<DecisionArm, ResolverError> {
        self.inner.resolve_decision()
    }

    #[inline]
    pub(crate) const fn erase(self) -> ErasedResolverRef<'cfg> {
        self.inner
    }
}

unsafe fn dispatch_decision_state<S>(
    storage: DecisionResolverStorage,
) -> Result<DecisionArm, ResolverError> {
    let payload = unsafe { storage.restore::<S>() };
    let state = /* SAFETY: `ResolverRef::decision_state` stored `state` with
    the matching `dispatch_decision_state::<S>` trampoline. The cluster
    resolver table only copies this erased pair after resolver-id validation,
    and evaluation creates one shared borrow for the callback duration. */
        unsafe { &*payload.state };
    (payload.resolver)(state)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DynamicResolverKey {
    rv: RendezvousId,
    resolver: crate::global::const_dsl::DynamicRouteResolver,
}

impl DynamicResolverKey {
    pub(crate) const fn new(
        rv: RendezvousId,
        resolver: crate::global::const_dsl::DynamicRouteResolver,
    ) -> Self {
        Self { rv, resolver }
    }

    pub(crate) const fn rendezvous(self) -> RendezvousId {
        self.rv
    }

    pub(crate) const fn resolver(self) -> crate::global::const_dsl::DynamicRouteResolver {
        self.resolver
    }
}
