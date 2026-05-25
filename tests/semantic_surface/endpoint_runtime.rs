use super::common::*;

#[test]
fn endpoint_kernel_stays_monomorphic_behind_raw_ops() {
    let endpoint = endpoint_facade_source();
    let flow = read("src/endpoint/flow.rs");
    let kernel = read("src/endpoint/kernel/core.rs");

    assert!(
        !endpoint.contains("dyn Any")
            && !flow.contains("dyn Any")
            && !kernel.contains("TypeId")
            && !kernel.contains("Box<dyn"),
        "typed Endpoint APIs must not recover behavior through runtime type-erasure escape hatches"
    );
}

#[test]
fn completed_raw_futures_fail_fast_on_repoll() {
    let endpoint = endpoint_facade_source();
    let flow = read("src/endpoint/flow.rs");
    let cursor = cursor_send_recv_tests_source();
    let no_policy = read("tests/no_policy_route_transport_hint.rs");

    assert!(
        !endpoint.contains("post-ready poll advances")
            && !flow.contains("post-ready poll advances")
            && !endpoint.contains("silently repoll")
            && !flow.contains("silently repoll"),
        "raw futures must not document or implement silent post-Ready progress"
    );
    for required in [
        "completed_recv_future_repoll_is_fail_fast_and_does_not_advance_again",
        "completed_send_future_repoll_is_fail_fast_and_does_not_advance_again",
        "completed offer future must fail fast on post-Ready poll",
        "completed decode future must fail fast on post-Ready poll",
    ] {
        assert!(
            cursor.contains(required) || no_policy.contains(required),
            "post-Ready fail-fast must have runtime coverage: {required}"
        );
    }
}

#[test]
fn payload_decode_after_commit_is_infallible() {
    let endpoint = endpoint_facade_source();
    let wire = read("src/transport/wire.rs");

    assert!(
        wire.contains("fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a>;")
            && wire.contains(
                "fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError>"
            ),
        "WirePayload must split pre-commit validation from infallible post-commit decode"
    );
    assert!(
        endpoint.contains("decode_validated_payload(payload)")
            && !endpoint.contains("::decode_payload(payload);"),
        "Endpoint recv/decode must not run a fallible payload decoder after committing progress"
    );
}

#[test]
fn decode_failure_completion_is_terminal_without_branch_restore() {
    let endpoint = endpoint_facade_source();
    let decode = read("src/endpoint/kernel/decode.rs");

    assert!(
        !endpoint.contains("core::hint::black_box") && !decode.contains("core::hint::black_box"),
        "decode terminal cleanup must not rely on black_box to hide branch ownership"
    );
    assert!(
        !endpoint.contains("unsafe fn begin_public_decode_state(&mut self) -> RecvResult<()>"),
        "begin_public_decode_state must not expose a dead Result"
    );

    assert!(
        read("tests/no_policy_route_transport_hint.rs")
            .contains("completed decode future must fail fast on post-Ready poll"),
        "decode terminal paths must be guarded by behavior coverage, not private cleanup helper names"
    );
}

#[test]
fn offer_transport_payload_presence_is_not_length_sentinel() {
    let offer = offer_frontier_source();
    let offer_ingress = read("src/endpoint/kernel/route_frontier/offer/ingress.rs");
    let offer_materialization = read("src/endpoint/kernel/route_frontier/offer/materialization.rs");
    let offer_state = read("src/endpoint/kernel/route_frontier/offer/state.rs");
    let core = read("src/endpoint/kernel/core.rs");

    for forbidden in [
        "transport_payload_len",
        "transport_payload_lane",
        "binding_evidence: [Option<LaneIngressEvidence>; 2]",
        "transport_payload: [Option<",
    ] {
        assert!(
            !offer.contains(forbidden)
                && !offer_ingress.contains(forbidden)
                && !offer_materialization.contains(forbidden)
                && !offer_state.contains(forbidden),
            "offer preview staging must not resurrect stale sentinel or anonymous rollback storage: {forbidden}"
        );
    }
    assert!(
        !offer.contains("!payload.as_bytes().is_empty()")
            && !offer_ingress.contains("!payload.as_bytes().is_empty()")
            && !offer_materialization.contains("!payload.as_bytes().is_empty()"),
        "offer preview staging must keep zero-length transport payloads as real consumed frames"
    );
    assert!(
        !core.contains("for (len, lane, _payload) in rollback.transport_payload"),
        "offer rollback must not hide ingress ownership in tuple mini-vec iteration"
    );
}

#[test]
fn array_map_unsafe_boundaries_are_explicit_and_panic_safe() {
    let map = read("src/control/lease/map.rs");
    let lease_core = read("src/control/lease/core.rs");

    assert!(
        map.contains("pub(crate) unsafe fn try_push_with")
            && map.contains(
                "`init` must fully initialize the provided slot before returning `Ok(())`"
            ),
        "ArrayMap::try_push_with must expose its MaybeUninit invariant as an unsafe contract"
    );
    assert!(
        lease_core.contains("SAFETY: The key written before delegation is `RendezvousId: Copy`")
            && lease_core.contains(".try_push_with("),
        "ArrayMap::try_push_with callers must document the exact initialized-state invariant"
    );
    assert!(
        !map.contains(
            "assume_init_drop();\n                    self.entries[i].write((key, value));"
        ),
        "ArrayMap::insert must not drop a live slot before replacement is committed"
    );
    assert!(
        map.contains("pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool)")
            && map.contains("V: Copy"),
        "ArrayMap::retain must stay constrained to Copy values instead of exposing a generic panic-unsafe compactor"
    );
    assert!(
        !map.contains("let old_len = self.len;\n        // compact retained entries later"),
        "ArrayMap::retain must not reintroduce a deferred-compaction shape that leaves len stale during unwinding"
    );
}
