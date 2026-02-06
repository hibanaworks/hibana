//! Observation primitives (tap ring and association snapshots).
//!
//! The current implementation maintains a fixed-length dual-ring buffer of
//! 20-byte tap events. Each entry captures coarse timing plus up to three
//! contextual arguments. This is intentionally small so that the ring fits
//! comfortably in cache or DMA-able memory regions.
//!
//! # Dual-Ring Architecture
//!
//! Events are routed to separate rings based on ID range:
//! - **User Ring** (`0x0000..0x00FF`): Application/EPF events (TAP_OUT, custom)
//! - **Infra Ring** (`0x0100..0xFFFF`): System events (ENDPOINT_SEND, etc.)
//!
//! This separation prevents Observer Effect feedback loops where streaming
//! infrastructure events would otherwise flood the ring.
//!
//! # Observational Equivalence: Implementation-Level Concept
//!
//! The tap ring enables **observational equivalence checking** between different
//! implementation strategies, notably `Forward::relay()` vs `Forward::splice()`:
//!
//! - **Raw tap events**: Reflect implementation details
//!   - `RELAY_FORWARD`: One event per frame (copying strategy)
//!   - `SPLICE_BEGIN` + `SPLICE_COMMIT`: Two-step zero-copy connection
//!
//! - **Normalized events**: Abstract over implementation differences
//!   - `normalise::forward_trace()` collapses both patterns to canonical
//!     `ForwardEvent` sequences
//!   - **`relay ≡ splice`**: Observationally equivalent at normalized level
//!
//! This is an **implementation-level property** (SOSP/OSDI-relevant performance
//! optimization) and is distinct from **type-level delegation** (MPST theory)
//! where `CapToken`-based session transfer achieves cut elimination at the
//! protocol level.
//!
//! See:
//! - `examples/forward_lowlevel_test.rs` - Implementation-level equivalence
//! - `examples/proxy_delegation.rs` - Type-level cut elimination
//! - [`crate::forward`] - Implementation-level forwarding API
//! - [`crate::endpoint::delegate`] - Type-level delegation API

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ptr, slice,
    sync::atomic::{AtomicPtr, AtomicU8, AtomicUsize, Ordering},
    task::Waker,
};

#[cfg(test)]
use core::{
    hint::spin_loop,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64},
};
#[cfg(test)]
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    thread,
};

use crate::{
    observe::ids,
    rendezvous::{Generation, Lane, SessionId},
    runtime::consts::{RING_BUFFER_SIZE, RING_EVENTS},
};

#[cfg(test)]
use core::mem;

/// 20-byte tap record with causal key tracking for roll-π reversibility.
///
/// Layout: `ts32, id16, causal_key16, arg0_32, arg1_32, arg2_32`
/// - `ts`: Timestamp (monotonic counter or wall-clock tick)
/// - `id`: Event identifier (from `crate::observe::ids::*`)
/// - `causal_key`: Causal key for reversible rollback tracking (roll-π)
///   - High 8 bits: role/lane index
///   - Low 8 bits: sequence number within epoch
/// - `arg0`, `arg1`: Context-dependent arguments (sid, gen, label, etc.)
/// - `arg2`: Extended context (e.g., ScopeId range/nest ordinals)
///
/// **Future extension**: For roll-π memory tracking, `causal_key` encodes
/// the (role, seq) pair that establishes causal dependencies. Rollback
/// operations can reconstruct causal history by following these keys.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct TapEvent {
    pub ts: u32,
    pub id: u16,
    pub causal_key: u16,
    pub arg0: u32,
    pub arg1: u32,
    pub arg2: u32,
}

impl TapEvent {
    #[inline]
    pub fn with_arg2(mut self, arg2: u32) -> Self {
        self.arg2 = arg2;
        self
    }

    /// Extract role/lane from causal key (high 8 bits).
    #[inline]
    pub const fn causal_role(self) -> u8 {
        (self.causal_key >> 8) as u8
    }

    /// Extract sequence number from causal key (low 8 bits).
    #[inline]
    pub const fn causal_seq(self) -> u8 {
        (self.causal_key & 0xFF) as u8
    }

    /// Construct causal key from role and sequence.
    #[inline]
    pub const fn make_causal_key(role: u8, seq: u8) -> u16 {
        ((role as u16) << 8) | (seq as u16)
    }

    /// Create a zeroed event (for array initialization).
    #[inline]
    pub const fn zero() -> Self {
        Self {
            ts: 0,
            id: 0,
            causal_key: 0,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        }
    }
}

// -----------------------------------------------------------------------------
// TapBatch: Batch of tap events for efficient streaming
// -----------------------------------------------------------------------------

/// Maximum number of events in a single batch.
///
/// Chosen to fit within a standard QUIC MTU (1200-1500 bytes):
/// - Header: 1 byte (count) + 4 bytes (lost_events) = 5 bytes
/// - Events: 50 × 20 bytes = 1000 bytes
/// - Total: 1005 bytes
pub const TAP_BATCH_MAX_EVENTS: usize = 50;

/// Batch of tap events for efficient streaming.
///
/// Wire format: `[count:u8][lost_events:u32][events:TapEvent×count]`
///
/// The `lost_events` field reports how many events were dropped due to
/// ring buffer overrun (backpressure loss). This enables consumers to
/// detect gaps in the event stream.
#[derive(Clone, Copy, Debug)]
pub struct TapBatch {
    events: [TapEvent; TAP_BATCH_MAX_EVENTS],
    count: u8,
    lost_events: u32,
}

impl Default for TapBatch {
    fn default() -> Self {
        Self::empty()
    }
}

impl TapBatch {
    /// Create an empty batch.
    #[inline]
    pub const fn empty() -> Self {
        Self {
            events: [TapEvent::zero(); TAP_BATCH_MAX_EVENTS],
            count: 0,
            lost_events: 0,
        }
    }

    /// Push an event into the batch. Returns `true` if successful.
    #[inline]
    pub fn push(&mut self, event: TapEvent) -> bool {
        if (self.count as usize) < TAP_BATCH_MAX_EVENTS {
            self.events[self.count as usize] = event;
            self.count += 1;
            true
        } else {
            false
        }
    }

    /// Check if the batch is full.
    #[inline]
    pub const fn is_full(&self) -> bool {
        self.count as usize >= TAP_BATCH_MAX_EVENTS
    }

    /// Check if the batch is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Number of events in the batch.
    #[inline]
    pub const fn len(&self) -> usize {
        self.count as usize
    }

    /// Number of events lost due to ring overrun.
    #[inline]
    pub const fn lost_events(&self) -> u32 {
        self.lost_events
    }

    /// Set the number of lost events.
    #[inline]
    pub fn set_lost_events(&mut self, lost: u32) {
        self.lost_events = lost;
    }

    /// Get a slice of the events in this batch.
    #[inline]
    pub fn as_slice(&self) -> &[TapEvent] {
        &self.events[..self.count as usize]
    }

    /// Iterate over events in the batch.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &TapEvent> {
        self.events[..self.count as usize].iter()
    }
}

// -----------------------------------------------------------------------------
// WakerSlot: no_std spinlock-based Waker management for streaming observe
// -----------------------------------------------------------------------------

const WAKER_UNLOCKED: u8 = 0;
const WAKER_LOCKED: u8 = 1;

/// Thread-safe slot for storing a single [`Waker`].
///
/// Uses a spinlock for `no_std` / `no_alloc` compatibility. Intended for
/// streaming observe where a single observer awaits new tap events.
pub struct WakerSlot {
    lock: AtomicU8,
    waker: UnsafeCell<Option<Waker>>,
}

unsafe impl Send for WakerSlot {}
unsafe impl Sync for WakerSlot {}

impl WakerSlot {
    /// Create a new empty waker slot.
    pub const fn new() -> Self {
        Self {
            lock: AtomicU8::new(WAKER_UNLOCKED),
            waker: UnsafeCell::new(None),
        }
    }

    #[inline]
    fn acquire(&self) {
        while self
            .lock
            .compare_exchange_weak(
                WAKER_UNLOCKED,
                WAKER_LOCKED,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    #[inline]
    fn release(&self) {
        self.lock.store(WAKER_UNLOCKED, Ordering::Release);
    }

    /// Register a waker to be notified on new events.
    pub fn register(&self, waker: &Waker) {
        self.acquire();
        // SAFETY: We hold the spinlock
        unsafe {
            *self.waker.get() = Some(waker.clone());
        }
        self.release();
    }

    /// Wake the registered waker (if any) WITHOUT clearing it.
    ///
    /// This allows the same waker to be notified multiple times without
    /// requiring re-registration after each wake. Essential for high-throughput
    /// streaming where events may arrive faster than the observer can poll.
    pub fn wake(&self) {
        self.acquire();
        // SAFETY: We hold the spinlock
        let waker_ref = unsafe { (*self.waker.get()).as_ref() };
        if let Some(w) = waker_ref {
            w.wake_by_ref();
        }
        self.release();
    }

    /// Clear the registered waker without waking.
    pub fn clear(&self) {
        self.acquire();
        // SAFETY: We hold the spinlock
        unsafe {
            *self.waker.get() = None;
        }
        self.release();
    }
}

impl Default for WakerSlot {
    fn default() -> Self {
        Self::new()
    }
}

/// Waker for User ring observers (id < 0x0100).
static USER_WAKER: WakerSlot = WakerSlot::new();

/// Waker for Infra ring observers (id >= 0x0100).
static INFRA_WAKER: WakerSlot = WakerSlot::new();

/// Register a waker to be notified when new INFRA tap events are pushed.
///
/// For backward compatibility, this registers with the Infra ring waker.
pub fn register_observe_waker(waker: &Waker) {
    INFRA_WAKER.register(waker);
}

/// Register a waker to be notified when new USER tap events are pushed.
pub fn register_user_waker(waker: &Waker) {
    USER_WAKER.register(waker);
}

/// Wake observers waiting for USER ring events.
fn wake_user_observers() {
    USER_WAKER.wake();
}

/// Wake observers waiting for INFRA ring events.
fn wake_infra_observers() {
    INFRA_WAKER.wake();
}

/// Clear the observe waker without waking (Infra ring).
pub fn clear_observe_waker() {
    INFRA_WAKER.clear();
}

/// Clear the user waker without waking.
pub fn clear_user_waker() {
    USER_WAKER.clear();
}

/// Single-producer ring buffer suited for DMA/SHM environments.
pub struct RingBuffer<'a> {
    head: AtomicUsize,
    storage: *mut TapEvent,
    _marker: PhantomData<&'a mut [TapEvent]>,
    _no_send_sync: PhantomData<*mut ()>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelEventKind {
    Begin,
    Ack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CancelEvent {
    pub kind: CancelEventKind,
    pub sid: u32,
    pub lane: u32,
}

#[inline]
fn map_cancel_event(event: TapEvent) -> Option<CancelEvent> {
    match event.id {
        ids::CANCEL_BEGIN => Some(CancelEvent {
            kind: CancelEventKind::Begin,
            sid: event.arg0,
            lane: event.arg1,
        }),
        ids::CANCEL_ACK => Some(CancelEvent {
            kind: CancelEventKind::Ack,
            sid: event.arg0,
            lane: event.arg1,
        }),
        _ => None,
    }
}

pub struct CancelTrace<'a> {
    events: &'a [TapEvent],
}

impl<'a> CancelTrace<'a> {
    #[inline]
    pub fn new(events: &'a [TapEvent]) -> Self {
        Self { events }
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = CancelEvent> + 'a {
        self.events.iter().copied().filter_map(map_cancel_event)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyEventKind {
    Abort,
    Trap,
    Annotate,
    Effect,
    EffectOk,
    Commit,
    Rollback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyEventDomain {
    Policy,
    Epf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyLaneExpectation {
    None,
    Optional,
    Required,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicySidHint {
    None,
    Arg1SessionNonZero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyEventSpec {
    id: u16,
    name: &'static str,
    pub kind: PolicyEventKind,
    pub domain: PolicyEventDomain,
    pub lane: PolicyLaneExpectation,
    pub sid_hint: PolicySidHint,
}

impl PolicyEventSpec {
    #[inline]
    pub const fn new(
        id: u16,
        name: &'static str,
        kind: PolicyEventKind,
        domain: PolicyEventDomain,
        lane: PolicyLaneExpectation,
        sid_hint: PolicySidHint,
    ) -> Self {
        Self {
            id,
            name,
            kind,
            domain,
            lane,
            sid_hint,
        }
    }

    #[inline]
    pub const fn id(self) -> u16 {
        self.id
    }

    #[inline]
    pub const fn name(self) -> &'static str {
        self.name
    }

    #[inline]
    pub const fn lane_expectation(self) -> PolicyLaneExpectation {
        self.lane
    }

    #[inline]
    pub fn sid_hint(self, event: &PolicyEvent) -> Option<u32> {
        self.sid_hint_from_arg(event.arg1)
    }

    #[inline]
    pub fn sid_hint_from_tap(self, event: TapEvent) -> Option<u32> {
        self.sid_hint_from_arg(event.arg1)
    }

    #[inline]
    fn sid_hint_from_arg(self, arg1: u32) -> Option<u32> {
        match self.sid_hint {
            PolicySidHint::None => None,
            PolicySidHint::Arg1SessionNonZero => {
                if arg1 != 0 {
                    Some(arg1)
                } else {
                    None
                }
            }
        }
    }
}

pub const POLICY_EVENT_SPECS: [PolicyEventSpec; 7] = [
    PolicyEventSpec::new(
        ids::POLICY_ABORT,
        "policy_abort",
        PolicyEventKind::Abort,
        PolicyEventDomain::Policy,
        PolicyLaneExpectation::Optional,
        PolicySidHint::Arg1SessionNonZero,
    ),
    PolicyEventSpec::new(
        ids::POLICY_TRAP,
        "policy_trap",
        PolicyEventKind::Trap,
        PolicyEventDomain::Policy,
        PolicyLaneExpectation::Optional,
        PolicySidHint::Arg1SessionNonZero,
    ),
    PolicyEventSpec::new(
        ids::POLICY_ANNOT,
        "policy_annot",
        PolicyEventKind::Annotate,
        PolicyEventDomain::Epf,
        PolicyLaneExpectation::Optional,
        PolicySidHint::None,
    ),
    PolicyEventSpec::new(
        ids::POLICY_EFFECT,
        "policy_effect",
        PolicyEventKind::Effect,
        PolicyEventDomain::Epf,
        PolicyLaneExpectation::Optional,
        PolicySidHint::None,
    ),
    PolicyEventSpec::new(
        ids::POLICY_RA_OK,
        "policy_effect_ok",
        PolicyEventKind::EffectOk,
        PolicyEventDomain::Epf,
        PolicyLaneExpectation::Optional,
        PolicySidHint::Arg1SessionNonZero,
    ),
    PolicyEventSpec::new(
        ids::POLICY_COMMIT,
        "policy_commit",
        PolicyEventKind::Commit,
        PolicyEventDomain::Policy,
        PolicyLaneExpectation::Optional,
        PolicySidHint::None,
    ),
    PolicyEventSpec::new(
        ids::POLICY_ROLLBACK,
        "policy_rollback",
        PolicyEventKind::Rollback,
        PolicyEventDomain::Policy,
        PolicyLaneExpectation::Optional,
        PolicySidHint::None,
    ),
];

#[inline]
pub fn policy_event_spec(id: u16) -> Option<PolicyEventSpec> {
    for spec in POLICY_EVENT_SPECS.iter() {
        if spec.id() == id {
            return Some(*spec);
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyEvent {
    pub kind: PolicyEventKind,
    pub arg0: u32,
    pub arg1: u32,
    pub lane: Option<u8>,
}

impl PolicyEvent {
    #[inline]
    pub fn from_tap(event: TapEvent) -> Option<Self> {
        map_policy_event(event)
    }
}

#[inline]
fn map_policy_event(event: TapEvent) -> Option<PolicyEvent> {
    let spec = policy_event_spec(event.id)?;
    let lane_hint = match spec.lane_expectation() {
        PolicyLaneExpectation::None => None,
        PolicyLaneExpectation::Optional | PolicyLaneExpectation::Required => {
            match event.causal_role() {
                0 => None,
                value => Some(value.saturating_sub(1)),
            }
        }
    };
    Some(PolicyEvent {
        kind: spec.kind,
        arg0: event.arg0,
        arg1: event.arg1,
        lane: lane_hint,
    })
}

pub struct PolicyTrace<'a> {
    events: &'a [TapEvent],
}

impl<'a> PolicyTrace<'a> {
    #[inline]
    pub fn new(events: &'a [TapEvent]) -> Self {
        Self { events }
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = PolicyEvent> + 'a {
        self.events.iter().copied().filter_map(map_policy_event)
    }
}

pub struct TapEvents<'cursor, 'ring, T, F>
where
    F: FnMut(TapEvent) -> Option<T>,
{
    cursor: &'cursor mut usize,
    index: usize,
    head: usize,
    storage: &'ring [TapEvent],
    mapper: F,
}

impl<'cursor, 'ring, T, F> Iterator for TapEvents<'cursor, 'ring, T, F>
where
    F: FnMut(TapEvent) -> Option<T>,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.head {
            let event = self.storage[self.index % RING_BUFFER_SIZE];
            self.index += 1;
            if let Some(mapped) = (self.mapper)(event) {
                return Some(mapped);
            }
        }
        None
    }
}

impl<'cursor, 'ring, T, F> Drop for TapEvents<'cursor, 'ring, T, F>
where
    F: FnMut(TapEvent) -> Option<T>,
{
    fn drop(&mut self) {
        *self.cursor = self.index;
    }
}

pub type CancelEvents<'cursor, 'ring> =
    TapEvents<'cursor, 'ring, CancelEvent, fn(TapEvent) -> Option<CancelEvent>>;
pub type PolicyEvents<'cursor, 'ring> =
    TapEvents<'cursor, 'ring, PolicyEvent, fn(TapEvent) -> Option<PolicyEvent>>;

pub type TapHook = fn(TapEvent);

struct GlobalTap {
    ring: AtomicPtr<TapRing<'static>>,
    pre: AtomicUsize,
    post: AtomicUsize,
}

impl GlobalTap {
    const fn new() -> Self {
        Self {
            ring: AtomicPtr::new(ptr::null_mut()),
            pre: AtomicUsize::new(0),
            post: AtomicUsize::new(0),
        }
    }

    fn install(&self, ring: &'static TapRing<'static>) -> Option<&'static TapRing<'static>> {
        let ptr = ring as *const TapRing<'static> as *mut TapRing<'static>;
        let previous = self.ring.swap(ptr, Ordering::AcqRel);
        if previous.is_null() {
            None
        } else {
            Some(unsafe { &*previous })
        }
    }

    fn uninstall(&self, target: *const TapRing<'static>) -> bool {
        let target_ptr = target as *mut TapRing<'static>;
        self.ring
            .compare_exchange(
                target_ptr,
                ptr::null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn with_ring<R>(&self, f: impl FnOnce(&TapRing<'static>) -> R) -> Option<R> {
        let ptr = self.ring.load(Ordering::Acquire);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { f(&*ptr) })
        }
    }

    fn load_hook(slot: &AtomicUsize) -> Option<TapHook> {
        let raw = slot.load(Ordering::Acquire);
        if raw == 0 {
            None
        } else {
            Some(unsafe { core::mem::transmute::<usize, TapHook>(raw) })
        }
    }

    fn store_hook(slot: &AtomicUsize, hook: Option<TapHook>) -> Option<TapHook> {
        let new_raw = hook.map_or(0usize, |cb| cb as usize);
        let previous = slot.swap(new_raw, Ordering::AcqRel);
        if previous == 0 {
            None
        } else {
            Some(unsafe { core::mem::transmute::<usize, TapHook>(previous) })
        }
    }

    fn invoke_pre(&self, event: &TapEvent) {
        if let Some(callback) = Self::load_hook(&self.pre) {
            callback(*event);
        }
    }

    fn invoke_post(&self, event: &TapEvent) {
        crate::observe::check::feed(*event);
        if let Some(callback) = Self::load_hook(&self.post) {
            callback(*event);
        }
        // Wake appropriate ring's observers based on event ID
        if event.id < ids::USER_EVENT_RANGE_END {
            wake_user_observers();
        } else {
            wake_infra_observers();
        }
    }
}

static GLOBAL_TAP: GlobalTap = GlobalTap::new();

#[derive(Clone, Copy, Debug)]
pub struct HookRegistration {
    pub previous_pre: Option<TapHook>,
    pub previous_post: Option<TapHook>,
}

pub fn install_ring(ring: &'static TapRing<'static>) -> Option<&'static TapRing<'static>> {
    GLOBAL_TAP.install(ring)
}

#[cfg(feature = "std")]
pub fn global_ring_ptr() -> usize {
    GLOBAL_TAP.with_ring(|ring| ring as *const _ as usize).unwrap_or(0)
}

pub fn uninstall_ring(ring: *const TapRing<'static>) -> bool {
    GLOBAL_TAP.uninstall(ring)
}

pub fn register_hooks(pre: Option<TapHook>, post: Option<TapHook>) -> HookRegistration {
    HookRegistration {
        previous_pre: GlobalTap::store_hook(&GLOBAL_TAP.pre, pre),
        previous_post: GlobalTap::store_hook(&GLOBAL_TAP.post, post),
    }
}

pub fn clear_hooks() {
    let _ = GlobalTap::store_hook(&GLOBAL_TAP.pre, None);
    let _ = GlobalTap::store_hook(&GLOBAL_TAP.post, None);
}

pub fn push(event: TapEvent) {
    let _ = GLOBAL_TAP.with_ring(|ring| {
        ring.push(event);
    });
}

pub fn emit(ring: &TapRing<'_>, event: TapEvent) {
    ring.push(event);
}

pub fn head() -> Option<usize> {
    GLOBAL_TAP.with_ring(|ring| ring.head())
}

pub fn user_head() -> Option<usize> {
    GLOBAL_TAP.with_ring(|ring| ring.user_head())
}

/// Read the event at a specific index (modulo ring size).
pub fn read_at(index: usize) -> Option<TapEvent> {
    GLOBAL_TAP.with_ring(|ring| ring.as_slice()[index % RING_BUFFER_SIZE])
}

/// Read the USER event at a specific index (modulo ring size).
pub fn read_user_at(index: usize) -> Option<TapEvent> {
    GLOBAL_TAP.with_ring(|ring| ring.user_slice()[index % RING_BUFFER_SIZE])
}

pub fn for_each_since(cursor: &mut usize, mut f: impl FnMut(TapEvent)) {
    let _ = GLOBAL_TAP.with_ring(|ring| {
        let head = ring.head();
        let storage = ring.as_slice();
        while *cursor < head {
            let idx = *cursor % RING_BUFFER_SIZE;
            f(storage[idx]);
            *cursor += 1;
        }
    });
}

#[cfg(test)]
static TS_CHECKER: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
static CHECKER_LOCK: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static CHECKER_OWNER: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
static LAST_TS: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static VIOLATION_FLAG: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
fn current_ts_checker() -> Option<fn(u32)> {
    let raw = TS_CHECKER.load(Ordering::Relaxed);
    if raw == 0 {
        None
    } else {
        Some(unsafe { mem::transmute::<usize, fn(u32)>(raw) })
    }
}

#[cfg(test)]
fn swap_ts_checker(new: Option<fn(u32)>) -> Option<fn(u32)> {
    let raw = new.map(|f| f as usize).unwrap_or(0);
    let prev = TS_CHECKER.swap(raw, Ordering::Relaxed);
    if prev == 0 {
        None
    } else {
        Some(unsafe { mem::transmute::<usize, fn(u32)>(prev) })
    }
}

#[cfg(test)]
pub fn install_ts_checker(checker: Option<fn(u32)>) -> Option<fn(u32)> {
    swap_ts_checker(checker)
}

#[cfg(test)]
fn current_thread_id_u64() -> u64 {
    let mut hasher = DefaultHasher::new();
    thread::current().id().hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observe::RawEvent;
    use static_assertions::assert_not_impl_any;

    assert_not_impl_any!(TapRing<'static>: Send, Sync);

    struct CheckerGuard;

    impl CheckerGuard {
        fn acquire() -> Self {
            while CHECKER_LOCK
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                spin_loop();
            }
            CHECKER_OWNER.store(current_thread_id_u64(), Ordering::Relaxed);
            Self
        }
    }

    impl Drop for CheckerGuard {
        fn drop(&mut self) {
            CHECKER_OWNER.store(0, Ordering::Relaxed);
            CHECKER_LOCK.store(false, Ordering::Release);
        }
    }

    #[test]
    fn head_wraps_without_losing_alignment() {
        let mut storage = [TapEvent::default(); RING_EVENTS];
        let ring = TapRing::from_storage(&mut storage);
        // We are testing the Infra ring (default push goes to Infra if ID >= 0x0100)
        // RawEvent::new(0, idx, ...) -> ID is idx.
        // If idx < 0x0100, it goes to User ring.
        // We need to use IDs >= 0x0100 to test Infra ring wrapping.
        
        // Mock GlobalTap to avoid null pointer deref in push if hooks are called?
        // TapRing::push calls GLOBAL_TAP hooks.
        // GLOBAL_TAP is static. It should be fine.

        // Use IDs >= 0x0100
        let base_id = 0x0200;

        // Set head on Infra ring
        ring.infra.set_head_for_test(usize::MAX - 2);

        for idx in 0..4 {
            ring.push(RawEvent::new(0, base_id + idx as u16, idx as u32, idx as u32));
        }

        let expected = (usize::MAX - 2).wrapping_add(4);
        assert_eq!(ring.head(), expected);

        // The last two writes should have wrapped around to the start of the ring.
        // Modulo is RING_BUFFER_SIZE
        let first_index = (usize::MAX - 2) % RING_BUFFER_SIZE;
        // Storage for Infra ring is the second half of the array
        let infra_offset = RING_BUFFER_SIZE;
        
        for offset in 0..4 {
            let idx = (first_index + offset) % RING_BUFFER_SIZE;
            // Check in the original storage array
            assert_eq!(storage[infra_offset + idx].id, base_id + offset as u16);
        }
    }

    #[test]
    fn timestamp_checker_detects_non_monotonic_push() {
        let _guard = CheckerGuard::acquire();
        fn checker(ts: u32) {
            let prev = LAST_TS.load(Ordering::Relaxed);
            if ts < prev {
                VIOLATION_FLAG.store(true, Ordering::Relaxed);
            }
            LAST_TS.store(ts, Ordering::Relaxed);
        }

        let mut storage = [TapEvent::default(); RING_EVENTS];
        let ring = TapRing::from_storage(&mut storage);

        let previous = install_ts_checker(Some(checker));
        LAST_TS.store(0, Ordering::Relaxed);
        VIOLATION_FLAG.store(false, Ordering::Relaxed);
        for ts in [1, 2, 3, 3, 5] {
            ring.push(RawEvent::new(ts, 0, 0, 0));
        }
        assert!(!VIOLATION_FLAG.load(Ordering::Relaxed));

        ring.push(RawEvent::new(4, 0, 0, 0));
        assert!(VIOLATION_FLAG.load(Ordering::Relaxed));

        install_ts_checker(previous);
    }
}

impl<'a> RingBuffer<'a> {
    pub fn new(storage: &'a mut [TapEvent]) -> Self {
        assert!(storage.len() >= RING_BUFFER_SIZE);
        Self {
            head: AtomicUsize::new(0),
            storage: storage.as_mut_ptr(),
            _marker: PhantomData,
            _no_send_sync: PhantomData,
        }
    }

    /// Treat this ring as having a `'static` lifetime.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the storage backing this ring lives for
    /// the remainder of the program or that any global registrations are
    /// cleared before the storage is reclaimed.
    pub unsafe fn assume_static(&self) -> &'static RingBuffer<'static> {
        let ptr: *const RingBuffer<'a> = self;
        unsafe { &*ptr.cast::<RingBuffer<'static>>() }
    }

    /// Obtain a raw pointer suitable for [`uninstall_ring`].
    ///
    /// # Safety
    ///
    /// See [`assume_static`](RingBuffer::assume_static) for the required lifetime
    /// guarantees.
    pub unsafe fn as_static_ptr(&self) -> *const RingBuffer<'static> {
        (self as *const RingBuffer<'a>).cast::<RingBuffer<'static>>()
    }

    /// Append an observation.
    pub fn push(&self, event: TapEvent) {
        let idx = self.head.fetch_add(1, Ordering::Relaxed) % RING_BUFFER_SIZE;
        #[cfg(test)]
        {
            if let Some(checker) = current_ts_checker() {
                let should_run = if CHECKER_LOCK.load(Ordering::Relaxed) {
                    let owner = CHECKER_OWNER.load(Ordering::Relaxed);
                    owner != 0 && owner == current_thread_id_u64()
                } else {
                    true
                };
                if should_run {
                    checker(event.ts);
                }
            }
        }
        unsafe {
            self.storage.add(idx).write(event);
        }
    }

    /// Returns the raw storage for external inspection.
    ///
    /// The view is best-effort: concurrent pushes may race with reads, so this
    /// is intended for offline inspection or environments that can quiesce the
    /// producer before reading.
    pub fn as_slice(&self) -> &[TapEvent] {
        unsafe { slice::from_raw_parts(self.storage, RING_BUFFER_SIZE) }
    }

    /// Returns the number of events that have been pushed since initialisation.
    pub fn head(&self) -> usize {
        self.head.load(Ordering::Relaxed)
    }

    pub fn events_since<'cursor, T, F>(
        &'a self,
        cursor: &'cursor mut usize,
        mapper: F,
    ) -> TapEvents<'cursor, 'a, T, F>
    where
        F: FnMut(TapEvent) -> Option<T>,
        'a: 'cursor,
    {
        let index = *cursor;
        TapEvents {
            cursor,
            index,
            head: self.head(),
            storage: self.as_slice(),
            mapper,
        }
    }

    pub fn cancel_events_since<'cursor>(
        &'a self,
        cursor: &'cursor mut usize,
    ) -> CancelEvents<'cursor, 'a>
    where
        'a: 'cursor,
    {
        self.events_since(
            cursor,
            map_cancel_event as fn(TapEvent) -> Option<CancelEvent>,
        )
    }

    pub fn policy_events_since<'cursor>(
        &'a self,
        cursor: &'cursor mut usize,
    ) -> PolicyEvents<'cursor, 'a>
    where
        'a: 'cursor,
    {
        self.events_since(
            cursor,
            map_policy_event as fn(TapEvent) -> Option<PolicyEvent>,
        )
    }

    #[cfg(test)]
    pub fn set_head_for_test(&self, value: usize) {
        self.head.store(value, Ordering::Relaxed);
    }
}

/// Dual-ring buffer separating User (Application) and Infra (System) events.
pub struct TapRing<'a> {
    user: RingBuffer<'a>,
    infra: RingBuffer<'a>,
}

impl<'a> TapRing<'a> {
    pub fn from_storage(storage: &'a mut [TapEvent; RING_EVENTS]) -> Self {
        let (user_slice, infra_slice) = storage.split_at_mut(RING_BUFFER_SIZE);
        Self {
            user: RingBuffer::new(user_slice),
            infra: RingBuffer::new(infra_slice),
        }
    }

    /// Treat this ring as having a `'static` lifetime.
    pub unsafe fn assume_static(&self) -> &'static TapRing<'static> {
        let ptr: *const TapRing<'a> = self;
        unsafe { &*ptr.cast::<TapRing<'static>>() }
    }

    /// Obtain a raw pointer suitable for [`uninstall_ring`].
    pub unsafe fn as_static_ptr(&self) -> *const TapRing<'static> {
        (self as *const TapRing<'a>).cast::<TapRing<'static>>()
    }

    /// Append an observation (routing to appropriate ring).
    ///
    /// Events are routed based on ID range:
    /// - `id < USER_EVENT_RANGE_END` (0x0100): User Ring (application/EPF events)
    /// - `id >= USER_EVENT_RANGE_END`: Infra Ring (system events)
    pub fn push(&self, event: TapEvent) {
        GLOBAL_TAP.invoke_pre(&event);

        if event.id < ids::USER_EVENT_RANGE_END {
            self.user.push(event);
        } else {
            self.infra.push(event);
        }

        GLOBAL_TAP.invoke_post(&event);
    }

    /// Returns the raw storage of the INFRA ring (default view).
    pub fn as_slice(&self) -> &[TapEvent] {
        self.infra.as_slice()
    }

    /// Returns the head of the INFRA ring (default view).
    pub fn head(&self) -> usize {
        self.infra.head()
    }

    /// Returns the raw storage of the USER ring.
    pub fn user_slice(&self) -> &[TapEvent] {
        self.user.as_slice()
    }

    /// Returns the head of the USER ring.
    pub fn user_head(&self) -> usize {
        self.user.head()
    }

    /// Iterate over events since `cursor` (INFRA ring).
    ///
    /// For backward compatibility, this delegates to the Infra ring.
    pub fn events_since<'cursor, T, F>(
        &'a self,
        cursor: &'cursor mut usize,
        mapper: F,
    ) -> TapEvents<'cursor, 'a, T, F>
    where
        F: FnMut(TapEvent) -> Option<T>,
        'a: 'cursor,
    {
        self.infra.events_since(cursor, mapper)
    }

    /// Iterate over cancel events since `cursor` (INFRA ring).
    pub fn cancel_events_since<'cursor>(
        &'a self,
        cursor: &'cursor mut usize,
    ) -> CancelEvents<'cursor, 'a>
    where
        'a: 'cursor,
    {
        self.infra.cancel_events_since(cursor)
    }

    /// Iterate over policy events since `cursor` (INFRA ring).
    pub fn policy_events_since<'cursor>(
        &'a self,
        cursor: &'cursor mut usize,
    ) -> PolicyEvents<'cursor, 'a>
    where
        'a: 'cursor,
    {
        self.infra.policy_events_since(cursor)
    }
}

/// Fence counters recorded per lane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FenceCounters {
    pub tx: Option<u32>,
    pub rx: Option<u32>,
}

/// Cancellation acknowledgements tracked per lane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AckCounters {
    pub last_gen: Option<Generation>,
    pub cancel_begin: u32,
    pub cancel_ack: u32,
}

/// Snapshot of association state for a session identifier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AssociationSnapshot {
    pub sid: SessionId,
    pub lane: Lane,
    pub active: bool,
    pub last_generation: Option<Generation>,
    pub last_checkpoint: Option<Generation>,
    pub in_splice: bool,
    pub pending_fences: Option<(u32, u32)>,
    pub pending_generation: Option<Generation>,
    pub fences: FenceCounters,
    pub acks: AckCounters,
}

// -----------------------------------------------------------------------------
// WaitForNewEvents: Future for streaming observe
// -----------------------------------------------------------------------------

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Future that resolves when new INFRA tap events are available.
///
/// Uses `INFRA_WAKER` and polls the Infra ring's `head()` to detect new events.
/// Uses double-check pattern to avoid races.
pub struct WaitForNewEvents {
    last_seen: usize,
}

impl WaitForNewEvents {
    /// Create a future that waits for events after `cursor`.
    pub fn new(cursor: usize) -> Self {
        Self { last_seen: cursor }
    }
}

impl Future for WaitForNewEvents {
    type Output = usize;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let current_head = head().unwrap_or(0);
        if current_head > self.last_seen {
            return Poll::Ready(current_head);
        }
        // Register waker before re-checking to avoid races
        register_observe_waker(cx.waker());
        // Double-check after registration
        let current_head = head().unwrap_or(0);
        if current_head > self.last_seen {
            Poll::Ready(current_head)
        } else {
            Poll::Pending
        }
    }
}

/// Future that resolves when new USER tap events are available.
///
/// Uses a dedicated `USER_WAKER` separate from `INFRA_WAKER`, so this future
/// is only woken when User ring events (id < 0x0100) are pushed.
pub struct WaitForNewUserEvents {
    last_seen: usize,
}

impl WaitForNewUserEvents {
    /// Create a future that waits for user events after `cursor`.
    pub fn new(cursor: usize) -> Self {
        Self { last_seen: cursor }
    }
}

impl Future for WaitForNewUserEvents {
    type Output = usize;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let current_head = user_head().unwrap_or(0);
        if current_head > self.last_seen {
            return Poll::Ready(current_head);
        }
        // Register with USER_WAKER (not INFRA_WAKER)
        register_user_waker(cx.waker());
        // Double-check after registration
        let current_head = user_head().unwrap_or(0);
        if current_head > self.last_seen {
            Poll::Ready(current_head)
        } else {
            Poll::Pending
        }
    }
}

// Canonical tap identifiers are generated at build time (see [`crate::observe::ids`]).
//
// # Event ID Ranges (Dual-Ring Routing)
//
// - `0x0000..0x00FF`: **User Ring** — Application/EPF events (TAP_OUT, custom)
// - `0x0100..0x013F`: Checkpoint / Rollback coordination
// - `0x0200..0x020F`: Splice / Cancel / Relay / Endpoint core events
// - `0x0210..0x021F`: Lane lifecycle + Transport telemetry
// - `0x0220..0x022F`: Loop / Route control (LOOP_DECISION, ROUTE_DECISION)
// - `0x0230..0x023F`: Delegation (DELEG_BEGIN, ROUTE_PICK, DELEG_SPLICE)
// - `0x0240..0x024F`: Capability lifecycle (CAP_MINT_BASE, CAP_CLAIM_BASE)
// - `0x0400..0x040F`: Policy VM events (ABORT, ANNOT, TRAP, EFFECT, COMMIT)
// - `0x0500`: Effect initialization (EFFECT_INIT)
// - `0x02FF`: Misuse detection (MISUSE_RECVGUARD_DROP)
