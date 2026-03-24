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
    sync::Mutex,
    thread,
};

use crate::{
    observe::ids,
    runtime::consts::{RING_BUFFER_SIZE, RING_EVENTS},
};

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
    pub const fn with_arg0(mut self, arg0: u32) -> Self {
        self.arg0 = arg0;
        self
    }

    #[inline]
    pub const fn with_arg1(mut self, arg1: u32) -> Self {
        self.arg1 = arg1;
        self
    }

    #[inline]
    pub const fn with_arg2(mut self, arg2: u32) -> Self {
        self.arg2 = arg2;
        self
    }

    #[inline]
    pub const fn with_causal_key(mut self, causal_key: u16) -> Self {
        self.causal_key = causal_key;
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
/// Chosen to fit within a typical path MTU (1200-1500 bytes):
/// - Header: 1 byte (count) + 4 bytes (lost_events) = 5 bytes
/// - Events: 50 × 20 bytes = 1000 bytes
/// - Total: 1005 bytes
pub(crate) const TAP_BATCH_MAX_EVENTS: usize = 50;

/// Batch of tap events for efficient streaming.
///
/// Wire format: `[count:u8][lost_events:u32][events:TapEvent×count]`
///
/// The `lost_events` field reports how many events were dropped due to
/// ring buffer overrun (backpressure loss). This enables consumers to
/// detect gaps in the event stream.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TapBatch {
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
struct WakerSlot {
    lock: AtomicU8,
    waker: UnsafeCell<Option<Waker>>,
}

unsafe impl Send for WakerSlot {}
unsafe impl Sync for WakerSlot {}

impl WakerSlot {
    /// Create a new empty waker slot.
    const fn new() -> Self {
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
    fn register(&self, waker: &Waker) {
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
    fn wake(&self) {
        self.acquire();
        // SAFETY: We hold the spinlock
        let waker_ref = unsafe { (*self.waker.get()).as_ref() };
        if let Some(w) = waker_ref {
            w.wake_by_ref();
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

/// Register a waker to be notified when new USER tap events are pushed.
fn register_user_waker(waker: &Waker) {
    USER_WAKER.register(waker);
}

/// Wake observers waiting for USER ring events.
fn wake_user_observers() {
    USER_WAKER.wake();
}

/// Single-producer ring buffer suited for DMA/SHM environments.
struct RingBuffer<'a> {
    head: AtomicUsize,
    storage: *mut TapEvent,
    _marker: PhantomData<&'a mut [TapEvent]>,
    _no_send_sync: PhantomData<*mut ()>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicyEventKind {
    Abort,
    Trap,
    Annotate,
    Effect,
    EffectOk,
    Commit,
    Rollback,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolicySidHint {
    None,
    Arg1SessionNonZero,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PolicyEventSpec {
    id: u16,
    pub(super) kind: PolicyEventKind,
    sid_hint: PolicySidHint,
}

#[cfg(test)]
impl PolicyEventSpec {
    #[inline]
    const fn new(id: u16, kind: PolicyEventKind, sid_hint: PolicySidHint) -> Self {
        Self { id, kind, sid_hint }
    }

    #[inline]
    pub(super) const fn id(self) -> u16 {
        self.id
    }

    #[inline]
    #[cfg(test)]
    pub(super) fn sid_hint_from_tap(self, event: TapEvent) -> Option<u32> {
        self.sid_hint_from_arg(event.arg1)
    }

    #[inline]
    #[cfg(test)]
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

#[cfg(test)]
const POLICY_EVENT_SPECS: [PolicyEventSpec; 8] = [
    PolicyEventSpec::new(
        ids::POLICY_ABORT,
        PolicyEventKind::Abort,
        PolicySidHint::Arg1SessionNonZero,
    ),
    PolicyEventSpec::new(
        ids::POLICY_TRAP,
        PolicyEventKind::Trap,
        PolicySidHint::Arg1SessionNonZero,
    ),
    PolicyEventSpec::new(
        ids::POLICY_ANNOT,
        PolicyEventKind::Annotate,
        PolicySidHint::None,
    ),
    PolicyEventSpec::new(
        ids::POLICY_EFFECT,
        PolicyEventKind::Effect,
        PolicySidHint::None,
    ),
    PolicyEventSpec::new(
        ids::POLICY_RA_OK,
        PolicyEventKind::EffectOk,
        PolicySidHint::Arg1SessionNonZero,
    ),
    PolicyEventSpec::new(
        ids::POLICY_COMMIT,
        PolicyEventKind::Commit,
        PolicySidHint::None,
    ),
    PolicyEventSpec::new(
        ids::POLICY_ROLLBACK,
        PolicyEventKind::Rollback,
        PolicySidHint::None,
    ),
    PolicyEventSpec::new(
        ids::POLICY_AUDIT_DEFER,
        PolicyEventKind::Annotate,
        PolicySidHint::None,
    ),
];

#[cfg(test)]
#[inline]
pub(super) fn policy_event_spec(id: u16) -> Option<PolicyEventSpec> {
    for spec in POLICY_EVENT_SPECS.iter() {
        if spec.id() == id {
            return Some(*spec);
        }
    }
    None
}

#[cfg(test)]
struct TapEvents<'cursor, 'ring, T, F>
where
    F: FnMut(TapEvent) -> Option<T>,
{
    cursor: &'cursor mut usize,
    index: usize,
    head: usize,
    storage: &'ring [TapEvent],
    mapper: F,
}

#[cfg(test)]
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

#[cfg(test)]
impl<'cursor, 'ring, T, F> Drop for TapEvents<'cursor, 'ring, T, F>
where
    F: FnMut(TapEvent) -> Option<T>,
{
    fn drop(&mut self) {
        *self.cursor = self.index;
    }
}

struct GlobalTap {
    ring: AtomicPtr<TapRing<'static>>,
}

impl GlobalTap {
    const fn new() -> Self {
        Self {
            ring: AtomicPtr::new(ptr::null_mut()),
        }
    }

    #[cfg(test)]
    fn install(&self, ring: &'static TapRing<'static>) -> Option<&'static TapRing<'static>> {
        let ptr = ring as *const TapRing<'static> as *mut TapRing<'static>;
        let previous = self.ring.swap(ptr, Ordering::AcqRel);
        if previous.is_null() {
            None
        } else {
            Some(unsafe { &*previous })
        }
    }

    #[cfg(test)]
    fn uninstall(&self, target: &'static TapRing<'static>) -> bool {
        let target_ptr = target as *const TapRing<'static> as *mut TapRing<'static>;
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

    fn invoke_post(&self, event: &TapEvent) {
        #[cfg(test)]
        crate::observe::check::feed(*event);
        if event.id < ids::USER_EVENT_RANGE_END {
            wake_user_observers();
        }
    }
}

static GLOBAL_TAP: GlobalTap = GlobalTap::new();

#[cfg(test)]
pub(crate) fn install_ring(ring: &'static TapRing<'static>) -> Option<&'static TapRing<'static>> {
    GLOBAL_TAP.install(ring)
}

#[cfg(test)]
pub(crate) fn uninstall_ring(ring: &'static TapRing<'static>) -> bool {
    GLOBAL_TAP.uninstall(ring)
}

pub(crate) fn push(event: TapEvent) {
    let _ = GLOBAL_TAP.with_ring(|ring| {
        ring.push(event);
    });
}

pub(crate) fn emit(ring: &TapRing<'_>, event: TapEvent) {
    ring.push(event);
}

pub(crate) fn user_head() -> Option<usize> {
    GLOBAL_TAP.with_ring(|ring| ring.user_head())
}

/// Read the USER event at a specific index (modulo ring size).
pub(crate) fn read_user_at(index: usize) -> Option<TapEvent> {
    GLOBAL_TAP.with_ring(|ring| ring.user_slice()[index % RING_BUFFER_SIZE])
}

#[cfg(test)]
static TS_CHECKER: Mutex<Option<fn(u32)>> = Mutex::new(None);

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
    *TS_CHECKER.lock().expect("timestamp checker mutex poisoned")
}

#[cfg(test)]
fn swap_ts_checker(new: Option<fn(u32)>) -> Option<fn(u32)> {
    let mut checker = TS_CHECKER.lock().expect("timestamp checker mutex poisoned");
    core::mem::replace(&mut *checker, new)
}

#[cfg(test)]
pub(crate) fn install_ts_checker(checker: Option<fn(u32)>) -> Option<fn(u32)> {
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
    use crate::observe::events::RawEvent;
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
        // RawEvent::new(0, idx) -> ID is idx.
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
            ring.push(
                RawEvent::new(0, base_id + idx as u16)
                    .with_arg0(idx as u32)
                    .with_arg1(idx as u32),
            );
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
            ring.push(RawEvent::new(ts, 0).with_arg0(0).with_arg1(0));
        }
        assert!(!VIOLATION_FLAG.load(Ordering::Relaxed));

        ring.push(RawEvent::new(4, 0).with_arg0(0).with_arg1(0));
        assert!(VIOLATION_FLAG.load(Ordering::Relaxed));

        install_ts_checker(previous);
    }
}

impl<'a> RingBuffer<'a> {
    fn new(storage: &'a mut [TapEvent]) -> Self {
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
    /// Append an observation.
    fn push(&self, event: TapEvent) {
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

    fn as_slice(&self) -> &[TapEvent] {
        unsafe { slice::from_raw_parts(self.storage, RING_BUFFER_SIZE) }
    }

    /// Returns the number of events that have been pushed since initialisation.
    fn head(&self) -> usize {
        self.head.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn events_since<'cursor, T, F>(
        &'a self,
        cursor: &'cursor mut usize,
        mapper: F,
    ) -> impl Iterator<Item = T> + 'cursor
    where
        F: FnMut(TapEvent) -> Option<T>,
        F: 'cursor,
        T: 'cursor,
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

    #[cfg(test)]
    fn set_head_for_test(&self, value: usize) {
        self.head.store(value, Ordering::Relaxed);
    }
}

/// Dual-ring buffer separating User (Application) and Infra (System) events.
pub(crate) struct TapRing<'a> {
    user: RingBuffer<'a>,
    infra: RingBuffer<'a>,
}

impl<'a> TapRing<'a> {
    pub(crate) fn from_storage(storage: &'a mut [TapEvent; RING_EVENTS]) -> Self {
        let (user_slice, infra_slice) = storage.split_at_mut(RING_BUFFER_SIZE);
        Self {
            user: RingBuffer::new(user_slice),
            infra: RingBuffer::new(infra_slice),
        }
    }

    /// Treat this ring as having a `'static` lifetime.
    #[cfg(test)]
    pub(crate) unsafe fn assume_static(&self) -> &'static TapRing<'static> {
        let ptr: *const TapRing<'a> = self;
        unsafe { &*ptr.cast::<TapRing<'static>>() }
    }

    /// Append an observation (routing to appropriate ring).
    ///
    /// Events are routed based on ID range:
    /// - `id < USER_EVENT_RANGE_END` (0x0100): User Ring (application/EPF events)
    /// - `id >= USER_EVENT_RANGE_END`: Infra Ring (system events)
    pub(crate) fn push(&self, event: TapEvent) {
        if event.id < ids::USER_EVENT_RANGE_END {
            self.user.push(event);
        } else {
            self.infra.push(event);
        }

        GLOBAL_TAP.invoke_post(&event);
    }

    #[cfg(test)]
    pub(crate) fn as_slice(&self) -> &[TapEvent] {
        self.infra.as_slice()
    }

    /// Returns the head of the INFRA ring (default view).
    #[cfg(test)]
    pub(crate) fn head(&self) -> usize {
        self.infra.head()
    }

    /// Returns the raw storage of the USER ring.
    pub(crate) fn user_slice(&self) -> &[TapEvent] {
        self.user.as_slice()
    }

    /// Returns the head of the USER ring.
    pub(crate) fn user_head(&self) -> usize {
        self.user.head()
    }

    #[cfg(test)]
    pub(crate) fn events_since<'cursor, T, F>(
        &'a self,
        cursor: &'cursor mut usize,
        mapper: F,
    ) -> impl Iterator<Item = T> + 'cursor
    where
        F: FnMut(TapEvent) -> Option<T>,
        F: 'cursor,
        T: 'cursor,
        'a: 'cursor,
    {
        self.infra.events_since(cursor, mapper)
    }
}

// -----------------------------------------------------------------------------
// WaitForNewUserEvents: Future for streaming user observe
// -----------------------------------------------------------------------------

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Future that resolves when new USER tap events are available.
///
/// This future is only woken when User ring events (id < 0x0100) are pushed.
pub(crate) struct WaitForNewUserEvents {
    last_seen: usize,
}

impl WaitForNewUserEvents {
    /// Create a future that waits for user events after `cursor`.
    pub(crate) fn new(cursor: usize) -> Self {
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
