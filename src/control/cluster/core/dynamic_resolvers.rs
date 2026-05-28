use super::{
    ContextValue, ControlOp, CpError, EffIndex, Lane, Location, MaybeUninit, PhantomData,
    PolicyMode, RendezvousId, ResourceScope, ScopeId, ScopeTrace, SessionId, UnsafeCell, context,
    fmt,
};
use crate::transport::context::PolicyInput;
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
pub enum RouteArm {
    Left,
    Right,
}

impl RouteArm {
    #[inline]
    pub const fn index(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteResolution {
    Arm(RouteArm),
    Defer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DynamicPolicyResolution {
    RouteArm { arm: u8 },
    Defer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResolverErrorLocation {
    location: &'static Location<'static>,
}

impl ResolverErrorLocation {
    #[inline]
    #[track_caller]
    pub(crate) fn caller() -> Self {
        Self {
            location: Location::caller(),
        }
    }

    #[inline]
    const fn file(self) -> &'static str {
        self.location.file()
    }

    #[inline]
    const fn line(self) -> u32 {
        self.location.line()
    }

    #[inline]
    const fn column(self) -> u32 {
        self.location.column()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolverOp {
    Reject,
    ResolveRoute,
    SetResolver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolverErrorKind {
    Reject,
    Control(CpError),
}

/// Semantic fail-closed error returned by resolver setup and dynamic resolvers.
///
/// A resolver error is diagnostic evidence. It rejects the resolver step and
/// does not grant route authority to transport hints, payload labels, or caller
/// alternate route logic.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ResolverError {
    pub(crate) op: ResolverOp,
    location: ResolverErrorLocation,
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
            location: ResolverErrorLocation::caller(),
            kind: ResolverErrorKind::Reject,
        }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn control(error: CpError) -> Self {
        Self {
            op: ResolverOp::SetResolver,
            location: ResolverErrorLocation::caller(),
            kind: ResolverErrorKind::Control(error),
        }
    }

    #[inline]
    pub(crate) const fn with_operation(mut self, op: ResolverOp) -> Self {
        self.op = op;
        self
    }

    #[inline]
    pub(crate) const fn with_operation_at(
        mut self,
        op: ResolverOp,
        location: ResolverErrorLocation,
    ) -> Self {
        self.op = op;
        self.location = location;
        self
    }

    #[inline]
    pub const fn operation(&self) -> &'static str {
        match self.op {
            ResolverOp::Reject => "reject",
            ResolverOp::ResolveRoute => "resolve_route",
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

impl From<CpError> for ResolverError {
    #[inline]
    #[track_caller]
    fn from(error: CpError) -> Self {
        Self::control(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolverContext {
    rv_id: RendezvousId,
    session: Option<SessionId>,
    pub(crate) lane: Lane,
    pub(crate) eff_index: EffIndex,
    tag: u8,
    pub(crate) scope_id: ScopeId,
    pub(crate) scope_trace: Option<ScopeTrace>,
    /// Slot-scoped policy input arguments.
    policy_input: PolicyInput,
    /// Slot-scoped policy attributes.
    policy_attrs: crate::transport::context::PolicyAttrs,
}

impl ResolverContext {
    #[inline]
    pub(crate) fn new(
        rv_id: RendezvousId,
        session: Option<SessionId>,
        lane: Lane,
        eff_index: EffIndex,
        tag: u8,
        scope_id: ScopeId,
        scope_trace: Option<ScopeTrace>,
        input: PolicyInput,
        attrs: &crate::transport::context::PolicyAttrs,
    ) -> Self {
        let mut policy_attrs = *attrs;
        policy_attrs.insert_core(context::core::RV_ID, ContextValue::from_u16(rv_id.raw()));
        if let Some(session) = session {
            policy_attrs.insert_core(
                context::core::SESSION_ID,
                ContextValue::from_u32(session.raw()),
            );
        }
        policy_attrs.insert_core(context::core::LANE, ContextValue::from_u32(lane.raw()));
        policy_attrs.insert_core(context::core::TAG, ContextValue::from_u8(tag));
        Self {
            rv_id,
            session,
            lane,
            eff_index,
            tag,
            scope_id,
            scope_trace,
            policy_input: input,
            policy_attrs,
        }
    }

    /// Read the slot-scoped policy input projection.
    #[inline]
    pub const fn policy_input(&self) -> PolicyInput {
        self.policy_input
    }

    /// Read the primary slot-scoped policy input word.
    #[inline]
    pub const fn primary_input(&self) -> u32 {
        self.policy_input.primary()
    }

    /// Read the latest slot-scoped latency observation, when supplied.
    #[inline]
    pub const fn latency_us(&self) -> Option<u64> {
        self.policy_attrs.latency_us()
    }

    /// Read the latest slot-scoped queue-depth observation, when supplied.
    #[inline]
    pub const fn queue_depth(&self) -> Option<u32> {
        self.policy_attrs.queue_depth()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RouteResolverStatePayload<S> {
    state: *const S,
    pub(crate) resolver: fn(&S, ResolverContext) -> Result<RouteResolution, ResolverError>,
}

#[derive(Clone, Copy)]
union RouteResolverStorage {
    stateless: fn(ResolverContext) -> Result<RouteResolution, ResolverError>,
    _stateful: RouteResolverStatePayload<()>,
}

#[derive(Clone, Copy)]
pub struct ResolverRef<'cfg> {
    storage: RouteResolverStorage,
    dispatch:
        unsafe fn(RouteResolverStorage, ResolverContext) -> Result<RouteResolution, ResolverError>,
    _marker: PhantomData<&'cfg ()>,
}

impl<'cfg> ResolverRef<'cfg> {
    #[inline]
    pub fn route_state<S: 'cfg>(
        state: &'cfg S,
        resolver: fn(&S, ResolverContext) -> Result<RouteResolution, ResolverError>,
    ) -> Self {
        const {
            assert!(
                core::mem::size_of::<RouteResolverStatePayload<S>>()
                    == core::mem::size_of::<RouteResolverStatePayload<()>>()
            );
            assert!(
                core::mem::align_of::<RouteResolverStatePayload<S>>()
                    == core::mem::align_of::<RouteResolverStatePayload<()>>()
            );
        }
        let payload = RouteResolverStatePayload {
            state: core::ptr::from_ref(state),
            resolver,
        };
        let mut storage = MaybeUninit::<RouteResolverStorage>::uninit();
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            storage
                .as_mut_ptr()
                .cast::<RouteResolverStatePayload<S>>()
                .write(payload);
        }
        Self {
            storage: /* SAFETY: the table owner tracks the initialized prefix and checks this slot before reading initialized storage. */ unsafe { storage.assume_init() },
            dispatch: dispatch_route_state::<S>,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn route_fn(
        resolver: fn(ResolverContext) -> Result<RouteResolution, ResolverError>,
    ) -> Self {
        Self {
            storage: RouteResolverStorage {
                stateless: resolver,
            },
            dispatch: dispatch_route_fn,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) const fn accepts_op(self, op: ControlOp) -> bool {
        matches!(op, ControlOp::RouteDecision)
    }

    #[inline]
    pub(crate) fn resolve_route(
        self,
        ctx: ResolverContext,
    ) -> Result<RouteResolution, ResolverError> {
        /* SAFETY: resolver storage is registered in the cluster table and borrowed only through the resolver slot owner. */
        unsafe {
            (self.dispatch)(self.storage, ctx)
                .map_err(|error| error.with_operation(ResolverOp::ResolveRoute))
        }
    }
}

unsafe fn dispatch_route_state<S>(
    storage: RouteResolverStorage,
    ctx: ResolverContext,
) -> Result<RouteResolution, ResolverError> {
    const {
        assert!(
            core::mem::size_of::<RouteResolverStatePayload<S>>()
                == core::mem::size_of::<RouteResolverStatePayload<()>>()
        );
        assert!(
            core::mem::align_of::<RouteResolverStatePayload<S>>()
                == core::mem::align_of::<RouteResolverStatePayload<()>>()
        );
    }
    let payload = /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */ unsafe {
        (&storage as *const RouteResolverStorage)
            .cast::<RouteResolverStatePayload<S>>()
            .read()
    };
    let state = /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */ unsafe { &*payload.state };
    (payload.resolver)(state, ctx)
}

unsafe fn dispatch_route_fn(
    storage: RouteResolverStorage,
    ctx: ResolverContext,
) -> Result<RouteResolution, ResolverError> {
    let resolver = /* SAFETY: resolver storage is registered in the cluster table and borrowed only through the resolver slot owner. */ unsafe { storage.stateless };
    resolver(ctx)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DynamicResolverKey {
    pub(crate) rv: RendezvousId,
    pub(crate) eff_index: EffIndex,
    pub(crate) op: ControlOp,
}

impl DynamicResolverKey {
    pub(crate) const fn new(rv: RendezvousId, eff_index: EffIndex, op: ControlOp) -> Self {
        Self { rv, eff_index, op }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DynamicResolverEntry<'cfg> {
    pub(crate) resolver: ResolverRef<'cfg>,
    pub(crate) policy: PolicyMode,
    pub(crate) scope_trace: Option<ScopeTrace>,
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
pub(in crate::control::cluster::core) struct ResolverBucketEntry<'cfg> {
    pub(crate) eff_index: EffIndex,
    pub(crate) op: ControlOp,
    entry: DynamicResolverEntry<'cfg>,
}

pub(crate) struct ResolverBucket<'cfg> {
    entries: UnsafeCell<*mut Option<ResolverBucketEntry<'cfg>>>,
    capacity: usize,
    _no_send_sync: PhantomData<*mut ()>,
}

impl<'cfg> ResolverBucket<'cfg> {
    pub(crate) const STORAGE_TAG_MASK: usize = Self::storage_align().saturating_sub(1);

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
        capacity.saturating_mul(core::mem::size_of::<Option<ResolverBucketEntry<'cfg>>>())
    }

    #[inline]
    pub(in crate::control::cluster::core) fn raw_entries(
        &self,
    ) -> *mut Option<ResolverBucketEntry<'cfg>> {
        /* SAFETY: resolver storage is registered in the cluster table and borrowed only through the resolver slot owner. */
        unsafe { *self.entries.get() }
    }

    #[inline]
    pub(in crate::control::cluster::core) fn entries_ptr(
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
        debug_assert_eq!(entries.addr() & Self::STORAGE_TAG_MASK, 0);
        debug_assert!(reclaim_delta <= Self::STORAGE_TAG_MASK);
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

    pub(crate) fn occupied_len(&self) -> usize {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return 0;
        }
        let mut idx = 0usize;
        let mut occupied = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                if (*entries.add(idx)).is_some() {
                    occupied += 1;
                }
            }
            idx += 1;
        }
        occupied
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
        let old_entries = self.entries_ptr();
        let old_capacity = self.capacity;
        let new_entries = storage.cast::<Option<ResolverBucketEntry<'cfg>>>();
        let mut idx = 0usize;
        while idx < new_capacity {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                new_entries.add(idx).write(None);
            }
            idx += 1;
        }

        if !old_entries.is_null() {
            let mut next = 0usize;
            let mut old_idx = 0usize;
            while old_idx < old_capacity {
                /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
                unsafe {
                    if let Some(entry) = (*old_entries.add(old_idx)).take() {
                        debug_assert!(next < new_capacity, "resolver bucket rebind overflow");
                        new_entries.add(next).write(Some(entry));
                        next += 1;
                    }
                }
                old_idx += 1;
            }
        }

        *self.entries.get_mut() = Self::encode_entries_ptr(new_entries, reclaim_delta);
        self.capacity = new_capacity;
    }

    pub(crate) fn insert(
        &mut self,
        eff_index: EffIndex,
        op: ControlOp,
        entry: DynamicResolverEntry<'cfg>,
    ) -> Result<(), CpError> {
        let entries = self.entries_ptr();
        if entries.is_null() {
            return Err(CpError::resource_exhausted(ResourceScope::ResolverTable));
        }
        let mut first_empty = None;
        let mut idx = 0usize;
        while idx < self.capacity {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe {
                let slot = &mut *entries.add(idx);
                match slot {
                    Some(stored) if stored.eff_index == eff_index && stored.op == op => {
                        stored.entry = entry;
                        return Ok(());
                    }
                    None if first_empty.is_none() => first_empty = Some(idx),
                    _ => {}
                }
            }
            idx += 1;
        }
        let Some(idx) = first_empty else {
            return Err(CpError::resource_exhausted(ResourceScope::ResolverTable));
        };
        /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
        unsafe {
            *entries.add(idx) = Some(ResolverBucketEntry {
                eff_index,
                op,
                entry,
            });
        }
        Ok(())
    }

    pub(crate) fn get(
        &self,
        eff_index: EffIndex,
        op: ControlOp,
    ) -> Option<&DynamicResolverEntry<'cfg>> {
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
                    && stored.op == op
                {
                    return Some(&stored.entry);
                }
            }
            idx += 1;
        }
        None
    }
}

pub(crate) const fn is_dynamic_control_op(op: ControlOp) -> bool {
    matches!(
        op,
        ControlOp::LoopContinue | ControlOp::LoopBreak | ControlOp::RouteDecision
    )
}
