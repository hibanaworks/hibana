use core::cell::UnsafeCell;

use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverError},
        tap,
        wire::{CodecError, WireEncode},
    },
};

use crate::common::TestTransport;
pub(crate) use crate::runtime_support::with_runtime_workspace;
pub(crate) use crate::tls_ref_support::with_resident_tls_ref;

type TestKitStorage = SessionKitStorage<'static, TestTransport>;

pub(crate) const ROUTE_RESOLVER: u16 = 0x91;
pub(crate) const OUTER_ROUTE_RESOLVER: u16 = 0x92;
pub(crate) const INNER_ROUTE_RESOLVER: u16 = 0x93;
pub(crate) const LEFT_A: u8 = 31;
pub(crate) const LEFT_B: u8 = 32;
pub(crate) const RIGHT: u8 = 33;
pub(crate) const RIGHT_B: u8 = 34;
pub(crate) const NESTED_LEFT: u8 = 41;
pub(crate) const NESTED_INNER_RIGHT: u8 = 42;
pub(crate) const NESTED_OUTER_RIGHT: u8 = 43;
pub(crate) const SAME_LABEL: u8 = 55;
pub(crate) const SAME_LABEL_ROUTE_RESOLVER: u16 = 0x94;
pub(crate) const COUNTING_ROUTE_RESOLVER: u16 = 0x95;
pub(crate) const FLIP_ROUTE_RESOLVER: u16 = 0x96;
pub(crate) const REJECT_ROUTE_RESOLVER: u16 = 0x97;
pub(crate) const DROP_ROUTE_RESOLVER: u16 = 0x98;
pub(crate) const PREFIX_OUTER_ROUTE_RESOLVER: u16 = 0xa0;
pub(crate) const PREFIX_INNER_ROUTE_RESOLVER: u16 = 0xa1;
pub(crate) const PREFIX_LEFT: u8 = 64;
pub(crate) const PREFIX_INNER_LEFT: u8 = 65;
pub(crate) const PREFIX_INNER_RIGHT: u8 = 66;
pub(crate) const PREFIX_OUTER_RIGHT: u8 = 67;
pub(crate) static LEFT_ARM: DecisionArm = DecisionArm::Left;
pub(crate) static RIGHT_ARM: DecisionArm = DecisionArm::Right;
pub(crate) static UNIT: () = ();

std::thread_local! {
    pub(crate) static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
    static COUNTERS: UnsafeCell<ResolverCounters> = const {
        UnsafeCell::new(ResolverCounters::new())
    };
}

#[derive(Clone, Copy)]
pub(crate) struct ResolverCounters {
    pub(crate) counting_calls: usize,
    pub(crate) flip_calls: usize,
    pub(crate) reject_calls: usize,
    pub(crate) reject_payload_encodes: usize,
    pub(crate) drop_calls: usize,
}

impl ResolverCounters {
    const fn new() -> Self {
        Self {
            counting_calls: 0,
            flip_calls: 0,
            reject_calls: 0,
            reject_payload_encodes: 0,
            drop_calls: 0,
        }
    }
}

pub(crate) fn reset_counters() {
    COUNTERS.with(|cell| unsafe {
        *cell.get() = ResolverCounters::new();
    });
}

fn update_counters(f: impl FnOnce(&mut ResolverCounters)) {
    COUNTERS.with(|cell| unsafe {
        f(&mut *cell.get());
    });
}

pub(crate) fn read_counters() -> ResolverCounters {
    COUNTERS.with(|cell| unsafe { *cell.get() })
}

pub(crate) fn choose_arm(arm: &DecisionArm) -> Result<DecisionArm, ResolverError> {
    Ok(*arm)
}

pub(crate) fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::par(
        g::send::<0, 1, Msg<LEFT_A, u8>>(),
        g::send::<0, 2, Msg<LEFT_B, u8>>(),
    );
    let right = g::par(
        g::send::<0, 1, Msg<RIGHT, u8>>(),
        g::send::<0, 2, Msg<RIGHT_B, u8>>(),
    );
    project(&g::route(left, right).resolve::<ROUTE_RESOLVER>())
}

pub(crate) fn nested_resolver_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<0, 1, Msg<NESTED_LEFT, u8>>(),
        g::send::<0, 1, Msg<NESTED_INNER_RIGHT, u8>>(),
    )
    .resolve::<INNER_ROUTE_RESOLVER>();
    project(
        &g::route(inner, g::send::<0, 1, Msg<NESTED_OUTER_RIGHT, u8>>())
            .resolve::<OUTER_ROUTE_RESOLVER>(),
    )
}

pub(crate) fn same_label_outbound_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    same_label_outbound_program_for::<ROLE, SAME_LABEL_ROUTE_RESOLVER>()
}

pub(crate) fn same_label_outbound_program_for<const ROLE: u8, const RESOLVER: u16>()
-> RoleProgram<ROLE> {
    let left = g::send::<0, 1, Msg<SAME_LABEL, u32>>();
    let right = g::send::<0, 1, Msg<SAME_LABEL, u64>>();
    project(&g::route(left, right).resolve::<RESOLVER>())
}

#[derive(Clone, Copy)]
pub(crate) struct RejectCountedPayload(pub(crate) u32);

impl WireEncode for RejectCountedPayload {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        update_counters(|counters| counters.reject_payload_encodes += 1);
        self.0.encode_into(out)
    }
}

impl hibana::runtime::wire::WirePayload for RejectCountedPayload {
    const SCHEMA_ID: u32 = 0x4000_0005;

    type Decoded<'a> = u32;

    fn validate_payload(input: hibana::runtime::wire::Payload<'_>) -> Result<(), CodecError> {
        if input.as_bytes().len() == 4 {
            Ok(())
        } else {
            Err(CodecError::Malformed)
        }
    }

    fn decode_validated_payload<'a>(
        input: hibana::runtime::wire::Payload<'a>,
    ) -> Self::Decoded<'a> {
        let bytes = input.as_bytes();
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

pub(crate) fn same_label_reject_payload_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<0, 1, Msg<SAME_LABEL, RejectCountedPayload>>();
    let right = g::send::<0, 1, Msg<SAME_LABEL, u64>>();
    project(&g::route(left, right).resolve::<REJECT_ROUTE_RESOLVER>())
}

pub(crate) fn prefix_then_nested_resolver_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::route(
        g::send::<0, 1, Msg<PREFIX_INNER_LEFT, u8>>(),
        g::send::<0, 1, Msg<PREFIX_INNER_RIGHT, u8>>(),
    )
    .resolve::<PREFIX_INNER_ROUTE_RESOLVER>();
    let left = g::seq(g::send::<0, 1, Msg<PREFIX_LEFT, u8>>(), inner);
    project(
        &g::route(left, g::send::<0, 1, Msg<PREFIX_OUTER_RIGHT, u8>>())
            .resolve::<PREFIX_OUTER_ROUTE_RESOLVER>(),
    )
}

pub(crate) fn resolver_audits(events: &[tap::TapEvent]) -> Vec<tap::TapEvent> {
    events
        .iter()
        .copied()
        .filter(|event| event.id() == tap::RESOLVER_AUDIT)
        .collect()
}

pub(crate) fn resolver_id(event: tap::TapEvent) -> u32 {
    event.arg1() & 0xffff
}

pub(crate) fn route_site(event: tap::TapEvent) -> u32 {
    event.arg1() >> 16
}

pub(crate) fn reject(_: &()) -> Result<DecisionArm, ResolverError> {
    Err(ResolverError::reject())
}

pub(crate) fn counting_left(_: &()) -> Result<DecisionArm, ResolverError> {
    update_counters(|counters| counters.counting_calls += 1);
    Ok(DecisionArm::Left)
}

pub(crate) fn flip_left_then_right(_: &()) -> Result<DecisionArm, ResolverError> {
    let call = read_counters().flip_calls;
    update_counters(|counters| counters.flip_calls += 1);
    Ok(if call == 0 {
        DecisionArm::Left
    } else {
        DecisionArm::Right
    })
}

pub(crate) fn rejecting_counted(_: &()) -> Result<DecisionArm, ResolverError> {
    update_counters(|counters| counters.reject_calls += 1);
    Err(ResolverError::reject())
}

pub(crate) fn drop_left(_: &()) -> Result<DecisionArm, ResolverError> {
    update_counters(|counters| counters.drop_calls += 1);
    Ok(DecisionArm::Left)
}

pub(crate) fn resolver_ids(events: &[tap::TapEvent]) -> Vec<u32> {
    resolver_audits(events)
        .into_iter()
        .map(resolver_id)
        .collect()
}

pub(crate) fn resolver_sites(events: &[tap::TapEvent]) -> Vec<u32> {
    resolver_audits(events)
        .into_iter()
        .map(route_site)
        .collect()
}

pub(crate) fn route_arm_selections(events: &[tap::TapEvent]) -> Vec<tap::TapEvent> {
    events
        .iter()
        .copied()
        .filter(|event| event.id() == tap::ROUTE_ARM_SELECTION)
        .collect()
}

pub(crate) fn selected_arm(event: tap::TapEvent) -> u32 {
    event.arg1() & 0xff
}
