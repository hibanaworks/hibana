use super::{
    ClusterError, EffIndex, MaybeUninit, PhantomData, RendezvousId, ResourceScope, UnsafeCell, fmt,
};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionArm {
    Left,
    Right,
}

impl DecisionArm {
    #[inline]
    pub const fn index(self) -> u8 {
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
    location: Callsite,
    kind: ResolverErrorKind,
}

impl fmt::Debug for ResolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolverError")
            .field("operation", &self.operation())
            .field("file", &self.file())
            .field("line", &self.line())
            .field("column", &self.column())
            .field("kind", &self.kind)
            .finish()
    }
}

impl ResolverError {
    #[inline]
    #[track_caller]
    pub fn reject() -> Self {
        Self {
            op: ResolverOp::Reject,
            location: Callsite::caller(),
            kind: ResolverErrorKind::Reject,
        }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn cluster(error: ClusterError) -> Self {
        Self {
            op: ResolverOp::SetResolver,
            location: Callsite::caller(),
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
        self.location = location;
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

    #[inline]
    pub const fn file(&self) -> &'static str {
        self.location.file()
    }

    #[inline]
    pub const fn line(&self) -> u32 {
        self.location.line()
    }

    #[inline]
    pub const fn column(&self) -> u32 {
        self.location.column()
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

#[inline]
pub(crate) const fn cluster_rendezvous_slot<const MAX_RV: usize>(
    rv_id: RendezvousId,
) -> Option<usize> {
    let raw = rv_id.raw() as usize;
    if raw == 0 || raw > MAX_RV {
        None
    } else {
        Some(raw - 1)
    }
}

#[derive(Clone, Copy)]
pub(in crate::session::cluster::core) struct ResolverBucketEntry<'cfg> {
    pub(crate) eff_index: EffIndex,
    entry: DynamicResolverEntry<'cfg>,
}

pub(crate) struct ResolverBucket<'cfg> {
    entries: UnsafeCell<*mut Option<ResolverBucketEntry<'cfg>>>,
    capacity: usize,
    _no_send_sync: PhantomData<*mut ()>,
}

impl<'cfg> ResolverBucket<'cfg> {
    pub(crate) const STORAGE_TAG_MASK: usize = Self::storage_align() - 1;

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).entries).write(UnsafeCell::new(core::ptr::null_mut()));
            core::ptr::addr_of_mut!((*dst).capacity).write(0);
            core::ptr::addr_of_mut!((*dst)._no_send_sync).write(PhantomData);
        }
    }

    #[inline]
    pub(crate) const fn storage_align() -> usize {
        core::mem::align_of::<Option<ResolverBucketEntry<'cfg>>>()
    }

    #[inline]
    pub(crate) const fn storage_bytes(capacity: usize) -> usize {
        let size = core::mem::size_of::<Option<ResolverBucketEntry<'cfg>>>();
        if size != 0 && capacity > usize::MAX / size {
            crate::invariant();
        }
        capacity * size
    }

    #[inline]
    pub(in crate::session::cluster::core) fn raw_entries(
        &self,
    ) -> *mut Option<ResolverBucketEntry<'cfg>> {
        /* SAFETY: resolver storage is registered in the cluster table and borrowed only through the resolver slot owner. */
        unsafe { *self.entries.get() }
    }

    #[inline]
    pub(in crate::session::cluster::core) fn entries_ptr(
        &self,
    ) -> *mut Option<ResolverBucketEntry<'cfg>> {
        self.raw_entries()
            .map_addr(|addr| addr & !Self::STORAGE_TAG_MASK)
    }

    #[inline]
    fn encode_entries_ptr(
        entries: *mut Option<ResolverBucketEntry<'cfg>>,
        reclaim_delta: usize,
    ) -> *mut Option<ResolverBucketEntry<'cfg>> {
        if entries.addr() & Self::STORAGE_TAG_MASK != 0 {
            crate::invariant();
        }
        if reclaim_delta > Self::STORAGE_TAG_MASK {
            crate::invariant();
        }
        entries.map_addr(|addr| addr | reclaim_delta)
    }

    #[inline]
    pub(crate) fn storage_ptr(&self) -> *mut u8 {
        self.entries_ptr().cast::<u8>()
    }

    #[inline]
    pub(crate) fn storage_reclaim_delta(&self) -> usize {
        self.raw_entries().addr() & Self::STORAGE_TAG_MASK
    }

    #[inline]
    pub(crate) fn storage_len(&self) -> usize {
        Self::storage_bytes(self.capacity)
    }

    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn entry_count(&self) -> usize {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return 0;
        }
        let mut idx = 0usize;
        let mut count = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                if (*entries.add(idx)).is_some() {
                    count += 1;
                }
            }
            idx += 1;
        }
        count
    }

    pub(crate) unsafe fn bind_from_storage(
        &mut self,
        storage: *mut u8,
        capacity: usize,
        reclaim_delta: usize,
    ) {
        let entries = storage.cast::<Option<ResolverBucketEntry<'cfg>>>();
        let mut idx = 0usize;
        while idx < capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                entries.add(idx).write(None);
            }
            idx += 1;
        }
        *self.entries.get_mut() = Self::encode_entries_ptr(entries, reclaim_delta);
        self.capacity = capacity;
    }

    pub(crate) unsafe fn rebind_from_storage(
        &mut self,
        storage: *mut u8,
        new_capacity: usize,
        reclaim_delta: usize,
    ) {
        let source_entries = self.entries_ptr();
        let source_capacity = self.capacity;
        let new_entries = storage.cast::<Option<ResolverBucketEntry<'cfg>>>();
        let mut idx = 0usize;
        while idx < new_capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                new_entries.add(idx).write(None);
            }
            idx += 1;
        }

        if !source_entries.is_null() {
            let mut next = 0usize;
            let mut source_idx = 0usize;
            while source_idx < source_capacity {
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    if let Some(entry) = (*source_entries.add(source_idx)).take() {
                        if next >= new_capacity {
                            crate::invariant();
                        }
                        new_entries.add(next).write(Some(entry));
                        next += 1;
                    }
                }
                source_idx += 1;
            }
        }

        *self.entries.get_mut() = Self::encode_entries_ptr(new_entries, reclaim_delta);
        self.capacity = new_capacity;
    }

    pub(crate) fn insert(
        &mut self,
        eff_index: EffIndex,
        entry: DynamicResolverEntry<'cfg>,
    ) -> Result<(), ClusterError> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return Err(ClusterError::resource_exhausted(
                ResourceScope::ResolverTable,
            ));
        }
        let mut first_empty = None;
        let mut idx = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                let slot = &mut *entries.add(idx);
                if let Some(stored) = slot {
                    if stored.eff_index == eff_index {
                        stored.entry = entry;
                        return Ok(());
                    }
                } else if first_empty.is_none() {
                    first_empty = Some(idx);
                }
            }
            idx += 1;
        }
        let Some(idx) = first_empty else {
            return Err(ClusterError::resource_exhausted(
                ResourceScope::ResolverTable,
            ));
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *entries.add(idx) = Some(ResolverBucketEntry { eff_index, entry });
        }
        Ok(())
    }

    pub(crate) fn get(&self, eff_index: EffIndex) -> Option<&DynamicResolverEntry<'cfg>> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return None;
        }
        let mut idx = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                if let Some(stored) = (&*entries.add(idx)).as_ref()
                    && stored.eff_index == eff_index
                {
                    return Some(&stored.entry);
                }
            }
            idx += 1;
        }
        None
    }
}
