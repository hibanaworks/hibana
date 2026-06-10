//! Internal normalisation helpers for tap traces used by crate tests.
//!
//! # Unsafe Owner Contract
//!
//! The `cfg(test)` fixed traces in this module own `MaybeUninit` arrays with an
//! initialized-prefix invariant. Push writes the next slot before increasing
//! length; slice views are bounded by that length and borrow the trace, so they
//! cannot outlive or alias a later mutable push.
use crate::observe::core::TapEvent;
use crate::observe::ids;
use crate::observe::scope::ScopeTrace;
use crate::runtime::consts::{RING_BUFFER_SIZE, RING_EVENTS};
use core::{mem::MaybeUninit, ops::Index, slice};

const ROUTE_PICK_ID: u16 = 0x0231;

/// Boundary events emitted while route/topology control progresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlTraceEvent {
    Pick {
        sid: u32,
        policy: u32,
        shard: u32,
    },
    TopologyAck {
        sid: u32,
        from: u8,
        to: u8,
        generation: u16,
    },
    RouteArmSelection {
        sid: u32,
        lane: u8,
        scope: u16,
        arm: u16,
        decision: u8,
        range: u16,
        nest: u16,
    },
}

/// Events emitted by endpoints while they exchange frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EndpointEvent {
    Send {
        sid: u32,
        lane: u8,
        role: u8,
        label: u8,
        flags: u8,
        scope: Option<ScopeTrace>,
    },
    Recv {
        sid: u32,
        lane: u8,
        role: u8,
        label: u8,
        flags: u8,
        scope: Option<ScopeTrace>,
    },
    Control {
        sid: u32,
        lane: u8,
        role: u8,
        label: u8,
        flags: u8,
        scope: Option<ScopeTrace>,
    },
}

const fn tap_scope(event: &TapEvent) -> Option<ScopeTrace> {
    let packed = event.arg2;
    if (packed & 0x8000_0000) == 0 {
        None
    } else {
        let range = ((packed & 0x7FFF_0000) >> 16) as u16;
        let nest = (packed & 0x0000_FFFF) as u16;
        Some(ScopeTrace::new(range, nest))
    }
}

#[derive(Clone, Copy, Debug)]
struct FixedTrace<T: Copy, const N: usize> {
    len: usize,
    items: [MaybeUninit<T>; N],
}

impl<T: Copy, const N: usize> FixedTrace<T, N> {
    fn new() -> Self {
        Self {
            len: 0,
            items: /* SAFETY: the table owner tracks the initialized prefix and checks this slot before reading initialized storage. */ unsafe { MaybeUninit::<[MaybeUninit<T>; N]>::uninit().assume_init() },
        }
    }

    fn push(&mut self, value: T) {
        assert!(self.len < N, "fixed trace capacity exceeded");
        self.items[self.len].write(value);
        self.len += 1;
    }

    fn len(&self) -> usize {
        self.len
    }

    fn as_slice(&self) -> &[T] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe { slice::from_raw_parts(self.items.as_ptr() as *const T, self.len) }
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        /* SAFETY: the pointer and length are carved from one backing slice after bounds and alignment checks. */
        unsafe { slice::from_raw_parts_mut(self.items.as_mut_ptr() as *mut T, self.len) }
    }

    fn iter(&self) -> slice::Iter<'_, T> {
        self.as_slice().iter()
    }
}

impl<T: Copy + Ord, const N: usize> FixedTrace<T, N> {
    fn sort_unstable(&mut self) {
        self.as_mut_slice().sort_unstable();
    }
}

impl<T: Copy, const N: usize> Index<usize> for FixedTrace<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

impl EndpointEvent {
    #[inline]
    fn sid(&self) -> u32 {
        match *self {
            EndpointEvent::Send { sid, .. }
            | EndpointEvent::Recv { sid, .. }
            | EndpointEvent::Control { sid, .. } => sid,
        }
    }

    #[inline]
    fn lane(&self) -> u8 {
        match *self {
            EndpointEvent::Send { lane, .. }
            | EndpointEvent::Recv { lane, .. }
            | EndpointEvent::Control { lane, .. } => lane,
        }
    }

    #[inline]
    fn role(&self) -> u8 {
        match *self {
            EndpointEvent::Send { role, .. }
            | EndpointEvent::Recv { role, .. }
            | EndpointEvent::Control { role, .. } => role,
        }
    }

    #[inline]
    fn label(&self) -> u8 {
        match *self {
            EndpointEvent::Send { label, .. }
            | EndpointEvent::Recv { label, .. }
            | EndpointEvent::Control { label, .. } => label,
        }
    }

        #[inline]
    fn sort_key(&self) -> (u8, u32, u8, u8, u8, u8) {
        match *self {
            EndpointEvent::Send {
                sid,
                lane,
                role,
                label,
                flags,
                ..
            } => (0, sid, lane, role, label, flags),
            EndpointEvent::Recv {
                sid,
                lane,
                role,
                label,
                flags,
                ..
            } => (1, sid, lane, role, label, flags),
            EndpointEvent::Control {
                sid,
                lane,
                role,
                label,
                flags,
                ..
            } => (2, sid, lane, role, label, flags),
        }
    }
}

/// Normalises the tap range `[start, end)` into route/topology boundary events.
fn control_trace(
    storage: &[TapEvent],
    start: usize,
    end: usize,
) -> FixedTrace<ControlTraceEvent, RING_EVENTS> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = FixedTrace::new();
    let mut cursor = start;
    let mut current_sid: Option<u32> = None;
    while cursor < end {
        let raw = storage[cursor % capacity];
        match raw.id {
            ROUTE_PICK_ID => {
                let sid = current_sid.unwrap_or(0);
                events.push(ControlTraceEvent::Pick {
                    sid,
                    policy: raw.arg0,
                    shard: raw.arg1,
                });
            }
            ids::ROUTE_ARM_SELECTION => {
                let scope = (raw.arg1 >> 16) as u16;
                let arm = (raw.arg1 & 0xFFFF) as u16;
                let (range, nest) = tap_scope(&raw)
                    .map(|trace| (trace.range, trace.nest))
                    .unwrap_or_else(|| {
                        let pack = raw.arg2;
                        (((pack >> 16) & 0xFFFF) as u16, (pack & 0xFFFF) as u16)
                    });
                events.push(ControlTraceEvent::RouteArmSelection {
                    sid: raw.arg0,
                    lane: raw.causal_role(),
                    scope,
                    arm,
                    decision: raw.causal_seq(),
                    range,
                    nest,
                });
            }
            ids::TOPOLOGY_ACK => {
                let sid = raw.arg1;
                current_sid = Some(sid);
                let encoded = raw.arg0;
                let from = (encoded & 0xFF) as u8;
                let to = ((encoded >> 8) & 0xFF) as u8;
                let generation = ((encoded >> 16) & 0xFFFF) as u16;
                events.push(ControlTraceEvent::TopologyAck {
                    sid,
                    from,
                    to,
                    generation,
                });
            }
            _ => {}
        }
        cursor += 1;
    }
    events
}

/// Normalises the tap range `[start, end)` into endpoint events, decoding
/// packed fields for easier comparison across seq/alt/par compositions.
fn endpoint_trace(
    storage: &[TapEvent],
    start: usize,
    end: usize,
) -> FixedTrace<EndpointEvent, RING_EVENTS> {
    let capacity = storage.len();
    debug_assert!(capacity == RING_EVENTS || capacity == RING_BUFFER_SIZE);
    let mut events = FixedTrace::new();
    let mut cursor = start;
    while cursor < end {
        let raw = storage[cursor % capacity];
        let packed = raw.arg1;
        let lane = ((packed >> 16) & 0xFF) as u8;
        let role = ((packed >> 24) & 0xFF) as u8;
        let label = ((packed >> 8) & 0xFF) as u8;
        let flags = (packed & 0xFF) as u8;
        let scope = tap_scope(&raw);
        match raw.id {
            ids::ENDPOINT_SEND => {
                events.push(EndpointEvent::Send {
                    sid: raw.arg0,
                    lane,
                    role,
                    label,
                    flags,
                    scope,
                });
            }
            ids::ENDPOINT_RECV => {
                events.push(EndpointEvent::Recv {
                    sid: raw.arg0,
                    lane,
                    role,
                    label,
                    flags,
                    scope,
                });
            }
            ids::ENDPOINT_CONTROL => {
                events.push(EndpointEvent::Control {
                    sid: raw.arg0,
                    lane,
                    role,
                    label,
                    flags,
                    scope,
                });
            }
            _ => {}
        }
        cursor += 1;
    }
    events
}

#[cfg(all(test, hibana_repo_tests))]
#[path = "normalise/tests.rs"]
mod tests;
