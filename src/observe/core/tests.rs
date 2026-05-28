use super::*;
use core::{cell::UnsafeCell, ptr, slice};
use std::{
    cell::Cell,
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    thread, thread_local,
};

use crate::observe::events::RawEvent;
use static_assertions::assert_not_impl_any;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolicyEventKind {
    Abort,
    Trap,
    Annotate,
    Effect,
    EffectOk,
    Commit,
    TxAbort,
    StateRestore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolicySidHint {
    None,
    Arg0SessionNonZero,
    Arg1SessionNonZero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PolicyEventSpec {
    id: u16,
    kind: PolicyEventKind,
    sid_hint: PolicySidHint,
}

impl PolicyEventSpec {
    #[inline]
    const fn new(id: u16, kind: PolicyEventKind, sid_hint: PolicySidHint) -> Self {
        Self { id, kind, sid_hint }
    }

    #[inline]
    const fn id(self) -> u16 {
        self.id
    }

    #[inline]
    fn sid_hint_from_tap(self, event: TapEvent) -> Option<u32> {
        match self.sid_hint {
            PolicySidHint::None => None,
            PolicySidHint::Arg0SessionNonZero => {
                if event.arg0 != 0 {
                    Some(event.arg0)
                } else {
                    None
                }
            }
            PolicySidHint::Arg1SessionNonZero => {
                if event.arg1 != 0 {
                    Some(event.arg1)
                } else {
                    None
                }
            }
        }
    }
}

const POLICY_EVENT_SPECS: [PolicyEventSpec; 9] = [
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
        PolicySidHint::Arg0SessionNonZero,
    ),
    PolicyEventSpec::new(
        ids::POLICY_TX_ABORT,
        PolicyEventKind::TxAbort,
        PolicySidHint::Arg0SessionNonZero,
    ),
    PolicyEventSpec::new(
        ids::POLICY_STATE_RESTORE,
        PolicyEventKind::StateRestore,
        PolicySidHint::Arg0SessionNonZero,
    ),
    PolicyEventSpec::new(
        ids::POLICY_AUDIT_DEFER,
        PolicyEventKind::Annotate,
        PolicySidHint::None,
    ),
];

fn policy_event_spec(id: u16) -> Option<PolicyEventSpec> {
    for spec in POLICY_EVENT_SPECS.iter() {
        if spec.id() == id {
            return Some(*spec);
        }
    }
    None
}

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

struct CheckerState {
    owner: Option<u64>,
    last_ts: u32,
    violation: bool,
}

impl CheckerState {
    const fn new() -> Self {
        Self {
            owner: None,
            last_ts: 0,
            violation: false,
        }
    }
}

thread_local! {
    static TEST_GLOBAL_TAP_RING: Cell<*mut TapRing<'static>> = const { Cell::new(ptr::null_mut()) };
    static TS_CHECKER: Cell<Option<fn(u32)>> = const { Cell::new(None) };
    static CHECKER_STATE: UnsafeCell<CheckerState> = const { UnsafeCell::new(CheckerState::new()) };
    static RING_STORAGE: UnsafeCell<[TapEvent; RING_EVENTS]> =
        const { UnsafeCell::new([TapEvent::zero(); RING_EVENTS]) };
}

pub(super) fn global_tap_ring_ptr() -> *mut TapRing<'static> {
    TEST_GLOBAL_TAP_RING.with(Cell::get)
}

pub(super) fn check_event_timestamp(ts: u32) {
    if let Some(checker) = TS_CHECKER.with(Cell::get) {
        let should_run = with_checker_state(|state| {
            state
                .owner
                .map(|owner| owner == current_thread_id_u64())
                .unwrap_or(true)
        });
        if should_run {
            checker(ts);
        }
    }
}

fn swap_ts_checker(new: Option<fn(u32)>) -> Option<fn(u32)> {
    TS_CHECKER.with(|checker| {
        let previous = checker.get();
        checker.set(new);
        previous
    })
}

fn with_checker_state<R>(f: impl FnOnce(&mut CheckerState) -> R) -> R {
    CHECKER_STATE.with(|state| {
        /* SAFETY: the test checker state is thread-local and borrowed uniquely through this helper. */
        unsafe { f(&mut *state.get()) }
    })
}

fn install_ts_checker(checker: Option<fn(u32)>) -> Option<fn(u32)> {
    swap_ts_checker(checker)
}

fn current_thread_id_u64() -> u64 {
    let mut hasher = DefaultHasher::new();
    thread::current().id().hash(&mut hasher);
    hasher.finish()
}

impl<'a> RingBuffer<'a> {
    fn as_slice(&self) -> &[TapEvent] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe { slice::from_raw_parts(self.storage, RING_BUFFER_SIZE) }
    }

    fn head(&self) -> usize {
        self.head.get()
    }

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
}

impl<'a> TapRing<'a> {
    pub(crate) fn head(&self) -> usize {
        self.infra.head()
    }

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

assert_not_impl_any!(TapRing<'static>: Send, Sync);

struct CheckerGuard;

impl CheckerGuard {
    fn acquire() -> Self {
        with_checker_state(|state| {
            state.owner = Some(current_thread_id_u64());
        });
        Self
    }
}

impl Drop for CheckerGuard {
    fn drop(&mut self) {
        with_checker_state(|state| {
            state.owner = None;
        });
    }
}

fn with_ring_storage<R>(f: impl FnOnce(&'static mut [TapEvent; RING_EVENTS]) -> R) -> R {
    RING_STORAGE.with(|storage| {
        let storage = /* SAFETY: the test ring storage is thread-local and borrowed uniquely through this helper. */ unsafe {
            &mut *storage.get()
        };
        storage.fill(TapEvent::zero());
        f(storage)
    })
}

#[test]
fn tap_event_fixed_decoder_rejects_trailing_bytes() {
    let event = TapEvent {
        ts: 0x0102_0304,
        id: 0x0506,
        causal_key: 0x0708,
        arg0: 0x1112_1314,
        arg1: 0x2122_2324,
        arg2: 0x3132_3334,
    };
    let mut encoded = [0u8; 21];
    assert_eq!(event.encode_into(&mut encoded[..20]), Ok(20));
    assert_eq!(
        TapEvent::decode_payload(Payload::new(&encoded[..20])),
        Ok(event)
    );
    assert_eq!(
        TapEvent::decode_payload(Payload::new(&encoded)),
        Err(CodecError::Invalid("payload length"))
    );
}

#[test]
fn head_wraps_without_losing_alignment() {
    with_ring_storage(|storage| {
        let ring = TapRing::from_storage(storage);
        let base_id = 0x0200;
        ring.infra.head.set(usize::MAX - 2);

        for idx in 0..4 {
            ring.push(
                RawEvent::new(0, base_id + idx as u16)
                    .with_arg0(idx as u32)
                    .with_arg1(idx as u32),
            );
        }

        let expected = (usize::MAX - 2).wrapping_add(4);
        assert_eq!(ring.head(), expected);

        let first_index = (usize::MAX - 2) % RING_BUFFER_SIZE;
        let infra_offset = RING_BUFFER_SIZE;
        for offset in 0..4 {
            let idx = (first_index + offset) % RING_BUFFER_SIZE;
            assert_eq!(storage[infra_offset + idx].id, base_id + offset as u16);
        }
    });
}

#[test]
fn timestamp_checker_detects_non_monotonic_push() {
    let checker_guard = CheckerGuard::acquire();
    core::hint::black_box(&checker_guard);
    fn checker(ts: u32) {
        with_checker_state(|state| {
            if ts < state.last_ts {
                state.violation = true;
            }
            state.last_ts = ts;
        });
    }

    with_ring_storage(|storage| {
        let ring = TapRing::from_storage(storage);

        let previous = install_ts_checker(Some(checker));
        with_checker_state(|state| {
            state.last_ts = 0;
            state.violation = false;
        });
        for ts in [1, 2, 3, 3, 5] {
            ring.push(RawEvent::new(ts, 0).with_arg0(0).with_arg1(0));
        }
        assert!(!with_checker_state(|state| state.violation));

        ring.push(RawEvent::new(4, 0).with_arg0(0).with_arg1(0));
        assert!(with_checker_state(|state| state.violation));

        install_ts_checker(previous);
    });
}

#[test]
fn policy_tx_abort_id_stays_distinct_from_audit_stream_ids() {
    let policy_and_audit_ids = [
        ids::POLICY_TX_ABORT,
        ids::POLICY_AUDIT,
        ids::POLICY_AUDIT_EXT,
        ids::POLICY_AUDIT_RESULT,
        ids::POLICY_REPLAY_EVENT,
        ids::POLICY_REPLAY_INPUT0,
        ids::POLICY_REPLAY_INPUT1,
        ids::POLICY_REPLAY_ATTRS0,
        ids::POLICY_REPLAY_ATTRS1,
        ids::POLICY_REPLAY_EVENT_EXT,
        ids::POLICY_AUDIT_DEFER,
    ];

    let mut outer = 0usize;
    while outer < policy_and_audit_ids.len() {
        let mut inner = outer + 1;
        while inner < policy_and_audit_ids.len() {
            assert_ne!(
                policy_and_audit_ids[outer], policy_and_audit_ids[inner],
                "policy/audit tap ids must stay unique"
            );
            inner += 1;
        }
        outer += 1;
    }

    let tx_abort = policy_event_spec(ids::POLICY_TX_ABORT).expect("tx abort policy event");
    assert_eq!(tx_abort.kind, PolicyEventKind::TxAbort);
    assert_eq!(
        tx_abort.sid_hint_from_tap(TapEvent::zero().with_arg0(0x1234_5678).with_arg1(7)),
        Some(0x1234_5678)
    );
    assert!(
        policy_event_spec(ids::POLICY_AUDIT).is_none(),
        "audit digest tuples must not collide with policy execution events"
    );
}

#[test]
fn commit_and_state_restore_policy_events_carry_sid_hints_in_arg0() {
    let commit = policy_event_spec(ids::POLICY_COMMIT).expect("commit policy event");
    assert_eq!(commit.kind, PolicyEventKind::Commit);
    assert_eq!(
        commit.sid_hint_from_tap(TapEvent::zero().with_arg0(0x0102_0304)),
        Some(0x0102_0304)
    );

    let restore = policy_event_spec(ids::POLICY_STATE_RESTORE).expect("state restore policy event");
    assert_eq!(restore.kind, PolicyEventKind::StateRestore);
    assert_eq!(
        restore.sid_hint_from_tap(TapEvent::zero().with_arg0(0x5566_7788)),
        Some(0x5566_7788)
    );
}
