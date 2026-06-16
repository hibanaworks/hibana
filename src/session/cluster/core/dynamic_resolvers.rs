use super::{ClusterError, EffIndex, MaybeUninit, PhantomData, RendezvousId, fmt};
use crate::diag::Callsite;
// # Unsafe Owner Contract
//
// This file owns dynamic resolver erased-storage dispatch for the session
// cluster. Resolver registration records either a stateless function pointer or
// a typed state pointer together with the matching trampoline; invocation must
// use the recorded trampoline for that exact resolver slot. The resident
// resolver table is supplied by the cluster storage owner and bound/rebound
// only through the explicit storage-layout paths in this module. Slot contents
// are represented as initialized `Option<DynamicResolverEntry>` values, and the
// raw state pointer is never exposed outside the resolver dispatch boundary.

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
pub enum DecisionResolution {
    Arm(DecisionArm),
    /// No arm is currently justified.
    ///
    /// Passive offer resolution keeps waiting for new evidence. Active
    /// controller sends cannot park after choosing to send a frame, so they
    /// fail the attempt with `ResolverReject`.
    Defer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DynamicResolverResolution {
    DecisionArm { arm: u8 },
    Defer,
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
    _location: Callsite,
    kind: ResolverErrorKind,
}

impl fmt::Debug for ResolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("ResolverError");
        debug.field("operation", &self.operation());
        #[cfg(feature = "std")]
        {
            debug
                .field("file", &self._location.file())
                .field("line", &self._location.line())
                .field("column", &self._location.column());
        }
        debug.field("kind", &self.kind).finish()
    }
}

impl ResolverError {
    #[inline]
    #[track_caller]
    pub fn reject() -> Self {
        Self {
            op: ResolverOp::Reject,
            _location: Callsite::caller(),
            kind: ResolverErrorKind::Reject,
        }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn cluster(error: ClusterError) -> Self {
        Self {
            op: ResolverOp::SetResolver,
            _location: Callsite::caller(),
            kind: ResolverErrorKind::Cluster(error),
        }
    }

    #[inline]
    pub(crate) const fn with_operation(mut self, op: ResolverOp) -> Self {
        self.op = op;
        self
    }

    #[inline]
    pub(crate) const fn with_operation_at(mut self, op: ResolverOp, location: Callsite) -> Self {
        self.op = op;
        self._location = location;
        self
    }

    #[inline]
    pub const fn operation(&self) -> &'static str {
        match self.op {
            ResolverOp::Reject => "reject",
            ResolverOp::ResolveDecision => "resolve_decision",
            ResolverOp::SetResolver => "set_resolver",
        }
    }
}

impl From<ClusterError> for ResolverError {
    #[inline]
    #[track_caller]
    fn from(error: ClusterError) -> Self {
        Self::cluster(error)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DecisionResolverStatePayload<S> {
    state: *const S,
    pub(crate) resolver: fn(&S) -> Result<DecisionResolution, ResolverError>,
}

#[derive(Clone, Copy)]
union DecisionResolverStorage {
    stateless: fn() -> Result<DecisionResolution, ResolverError>,
    _stateful: DecisionResolverStatePayload<()>,
}

#[derive(Clone, Copy)]
pub(crate) struct ErasedResolverRef<'cfg> {
    storage: DecisionResolverStorage,
    dispatch: unsafe fn(DecisionResolverStorage) -> Result<DecisionResolution, ResolverError>,
    _marker: PhantomData<&'cfg ()>,
}

impl<'cfg> ErasedResolverRef<'cfg> {
    #[inline]
    pub(crate) fn resolve_decision(self) -> Result<DecisionResolution, ResolverError> {
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
        resolver: fn(&S) -> Result<DecisionResolution, ResolverError>,
    ) -> Self {
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
        let payload = DecisionResolverStatePayload {
            state: core::ptr::from_ref(state),
            resolver,
        };
        let mut storage = MaybeUninit::<DecisionResolverStorage>::uninit();
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            storage
                .as_mut_ptr()
                .cast::<DecisionResolverStatePayload<S>>()
                .write(payload);
        }
        Self {
            inner: ErasedResolverRef {
                storage: /* SAFETY: the table owner tracks the initialized prefix and checks this slot before reading initialized storage. */ unsafe { storage.assume_init() },
                dispatch: dispatch_decision_state::<S>,
                _marker: PhantomData,
            },
        }
    }

    #[inline]
    pub fn decision_fn(resolver: fn() -> Result<DecisionResolution, ResolverError>) -> Self {
        Self {
            inner: ErasedResolverRef {
                storage: DecisionResolverStorage {
                    stateless: resolver,
                },
                dispatch: dispatch_decision_fn,
                _marker: PhantomData,
            },
        }
    }

    /// Evaluate this typed resolver without erasing its resolver id.
    ///
    /// This is for typed resolver owners. It does not commit route/session progress.
    /// It does not expose the erased storage handle.
    #[inline]
    pub fn evaluate(self) -> Result<DecisionResolution, ResolverError> {
        self.inner.resolve_decision()
    }

    #[inline]
    pub(crate) const fn erase(self) -> ErasedResolverRef<'cfg> {
        self.inner
    }
}

unsafe fn dispatch_decision_state<S>(
    storage: DecisionResolverStorage,
) -> Result<DecisionResolution, ResolverError> {
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
    let payload = /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */ unsafe {
        (&storage as *const DecisionResolverStorage)
            .cast::<DecisionResolverStatePayload<S>>()
            .read()
    };
    let state = /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */ unsafe { &*payload.state };
    (payload.resolver)(state)
}

unsafe fn dispatch_decision_fn(
    storage: DecisionResolverStorage,
) -> Result<DecisionResolution, ResolverError> {
    let resolver = /* SAFETY: resolver storage is registered in the cluster table and borrowed only through the resolver slot owner. */ unsafe { storage.stateless };
    resolver()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DynamicResolverKey {
    pub(crate) rv: RendezvousId,
    pub(crate) eff_index: EffIndex,
}

impl DynamicResolverKey {
    pub(crate) const fn new(rv: RendezvousId, eff_index: EffIndex) -> Self {
        Self { rv, eff_index }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DynamicResolverEntry<'cfg> {
    pub(crate) resolver_ref: ErasedResolverRef<'cfg>,
    pub(crate) resolver_id: u16,
    pub(crate) scope: crate::global::const_dsl::CompactScopeId,
}
