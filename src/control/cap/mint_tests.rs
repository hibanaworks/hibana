use super::{
    CapError, CapHeader, CapShot, ControlOp, ControlPath, ControlResourceKind, ControlScopeKind,
    E0, EndpointHandle, EndpointResource, GenericCapToken, HandleView, Owner, ResourceKind,
};
use crate::{
    control::{
        brand::with_brand,
        cap::resource_kinds::{LoopContinueKind, LoopDecisionHandle},
        types::{Lane, SessionId},
    },
    global::const_dsl::ScopeId,
    transport::wire::{CodecError, Payload, WirePayload},
};

fn endpoint_header_fixture() -> [u8; super::CAP_HEADER_LEN] {
    let handle = EndpointHandle::new(SessionId::new(7), Lane::new(3), 1);
    let mut header = [0u8; super::CAP_HEADER_LEN];
    CapHeader::new(
        handle.sid,
        handle.lane,
        handle.role,
        EndpointResource::TAG,
        ControlOp::Fence,
        ControlPath::Local,
        CapShot::One,
        ControlScopeKind::None,
        0,
        0,
        0,
        EndpointResource::encode_handle(&handle),
    )
    .encode(&mut header);
    header
}

fn token_from_wire<K: ResourceKind>(
    nonce: [u8; super::CAP_NONCE_LEN],
    header: [u8; super::CAP_HEADER_LEN],
) -> GenericCapToken<K> {
    let mut bytes = [0u8; super::CAP_TOKEN_LEN];
    bytes[..super::CAP_NONCE_LEN].copy_from_slice(&nonce);
    bytes[super::CAP_NONCE_LEN..super::CAP_NONCE_LEN + super::CAP_HEADER_LEN]
        .copy_from_slice(&header);
    GenericCapToken::from_bytes(bytes)
}

fn endpoint_token_with_mutated_header(
    mutate: fn(&mut [u8; super::CAP_HEADER_LEN]),
) -> GenericCapToken<EndpointResource> {
    let mut header = endpoint_header_fixture();
    mutate(&mut header);
    token_from_wire::<EndpointResource>([0u8; super::CAP_NONCE_LEN], header)
}

#[test]
fn owner_binds_rendezvous_brand() {
    with_brand(|rv_brand| {
        let owner: Owner<'_, E0> = Owner::new(rv_brand.guard());
        let _ = owner;
    });
}

#[test]
fn handle_view_decodes_payload() {
    let handle = LoopDecisionHandle {
        sid: 12,
        lane: 4,
        scope: ScopeId::route(3),
    };
    let payload = LoopContinueKind::encode_handle(&handle);
    let view =
        HandleView::<LoopContinueKind>::decode(&payload, Some(handle.scope)).expect("decode");
    assert_eq!(view.bytes(), &payload);
    assert_eq!(view.handle(), &handle);
    assert_eq!(view.scope(), Some(handle.scope));
}

#[test]
fn handle_view_decodes_endpoint_payload() {
    let handle = EndpointHandle::new(SessionId::new(1), Lane::new(0), 3);
    let payload = EndpointResource::encode_handle(&handle);
    let view = HandleView::<EndpointResource>::decode(&payload, None).expect("decode");
    assert_eq!(view.bytes(), &payload);
    assert_eq!(view.handle(), &handle);
    assert_eq!(view.scope(), None);
}

/// Regression test: `HandleView::decode` is stateless.
///
/// Re-decoding the same handle bytes is valid. Runtime release semantics are
/// owned by rendezvous registered-token cleanup, not by handle decoding.
#[test]
fn simulate_abort_then_retry() {
    let handle = EndpointHandle::new(SessionId::new(42), Lane::new(1), 2);
    let payload = EndpointResource::encode_handle(&handle);

    // First decode succeeds
    let view1 = HandleView::<EndpointResource>::decode(&payload, None);
    assert!(view1.is_ok());
    let view1 = view1.unwrap();
    assert_eq!(view1.handle(), &handle);

    // Second decode uses the same payload again. HandleView::decode is
    // stateless; rendezvous registered-token state owns release tracking.
    let view2 = HandleView::<EndpointResource>::decode(&payload, None);
    assert!(view2.is_ok());
}

/// Test GenericCapToken::as_view() ergonomic API
///
/// This tests the mint → HandleView extraction chain:
/// 1. Create a token with embedded handle
/// 2. Extract HandleView via as_view()
/// 3. Verify descriptor/header fields survive round-trip
/// 4. Verify handle bytes survive round-trip
#[test]
fn generic_cap_token_as_view() {
    use super::{CAP_HEADER_LEN, CAP_NONCE_LEN};

    let handle = EndpointHandle::new(SessionId::new(7), Lane::new(3), 1);
    let handle_bytes = EndpointResource::encode_handle(&handle);

    let mut header = [0u8; CAP_HEADER_LEN];
    CapHeader::new(
        handle.sid,
        handle.lane,
        handle.role,
        EndpointResource::TAG,
        ControlOp::Fence,
        crate::control::cap::mint::ControlPath::Local,
        CapShot::One,
        ControlScopeKind::None,
        0,
        0,
        0,
        handle_bytes,
    )
    .encode(&mut header);

    let token = token_from_wire::<EndpointResource>([0u8; CAP_NONCE_LEN], header);

    // Extract HandleView via as_view()
    let view = token.as_view().expect("as_view should succeed");

    // Verify handle matches
    assert_eq!(view.handle(), &handle);
    // Verify bytes match
    assert_eq!(view.bytes(), &handle_bytes);
    let header = token.control_header().expect("header");
    assert_eq!(header.sid(), handle.sid);
    assert_eq!(header.lane(), handle.lane);
    assert_eq!(header.role(), handle.role);
}

#[test]
fn generic_cap_token_typed_views_reject_resource_tag_mismatch() {
    use super::{CAP_HEADER_LEN, CAP_NONCE_LEN};

    let handle = EndpointHandle::new(SessionId::new(7), Lane::new(3), 1);
    let mut header = [0u8; CAP_HEADER_LEN];
    CapHeader::new(
        handle.sid,
        handle.lane,
        handle.role,
        LoopContinueKind::TAG,
        LoopContinueKind::OP,
        LoopContinueKind::PATH,
        CapShot::One,
        LoopContinueKind::SCOPE,
        0,
        1,
        2,
        EndpointResource::encode_handle(&handle),
    )
    .encode(&mut header);

    let token = token_from_wire::<EndpointResource>([0u8; CAP_NONCE_LEN], header);

    assert!(matches!(token.decode_handle(), Err(CapError)));
    assert!(matches!(token.as_view(), Err(CapError)));
    assert!(matches!(token.scope(), Err(CapError)));
}

#[test]
fn cap_header_decode_rejects_unknown_atomic_fields() {
    let mut raw = [0u8; super::CAP_HEADER_LEN];
    CapHeader::new(
        SessionId::new(7),
        Lane::new(3),
        1,
        LoopContinueKind::TAG,
        LoopContinueKind::OP,
        LoopContinueKind::PATH,
        CapShot::One,
        LoopContinueKind::SCOPE,
        0,
        1,
        2,
        LoopContinueKind::encode_handle(&LoopDecisionHandle {
            sid: 7,
            lane: 3,
            scope: ScopeId::loop_scope(1),
        }),
    )
    .encode(&mut raw);

    for (index, value) in [(8usize, 0xFF), (9, 0xFF), (10, 0xFF), (11, 0xFF)] {
        let mut corrupted = raw;
        corrupted[index] = value;
        assert!(
            matches!(CapHeader::decode(corrupted), Err(super::CapError)),
            "unknown control header field at byte {index} must fail closed",
        );
    }
}

#[test]
fn cap_header_decode_rejects_reserved_flags() {
    let mut raw = [0u8; super::CAP_HEADER_LEN];
    CapHeader::new(
        SessionId::new(7),
        Lane::new(3),
        1,
        LoopContinueKind::TAG,
        LoopContinueKind::OP,
        LoopContinueKind::PATH,
        CapShot::One,
        LoopContinueKind::SCOPE,
        0,
        1,
        2,
        LoopContinueKind::encode_handle(&LoopDecisionHandle {
            sid: 7,
            lane: 3,
            scope: ScopeId::loop_scope(1),
        }),
    )
    .encode(&mut raw);
    raw[12] = 0x80;

    assert!(
        matches!(CapHeader::decode(raw), Err(super::CapError)),
        "reserved control header flags must fail closed",
    );
}

#[test]
fn generic_cap_token_decode_requires_exact_wire_length() {
    let exact = [0u8; super::CAP_TOKEN_LEN];
    assert!(
        <GenericCapToken<()> as WirePayload>::decode_payload(Payload::new(&exact)).is_ok(),
        "exact-size capability tokens must decode"
    );

    let mut short = [0u8; super::CAP_TOKEN_LEN - 1];
    short.copy_from_slice(&exact[..super::CAP_TOKEN_LEN - 1]);
    assert!(matches!(
        <GenericCapToken<()> as WirePayload>::decode_payload(Payload::new(&short)),
        Err(CodecError::Truncated)
    ));

    let mut trailing = [0u8; super::CAP_TOKEN_LEN + 1];
    trailing[..super::CAP_TOKEN_LEN].copy_from_slice(&exact);
    trailing[super::CAP_TOKEN_LEN] = 0xA5;
    assert!(
        matches!(
            <GenericCapToken<()> as WirePayload>::decode_payload(Payload::new(&trailing)),
            Err(CodecError::Invalid("trailing bytes after GenericCapToken"))
        ),
        "control tokens are fixed-size and must reject ignored trailing bytes"
    );
}

#[test]
fn malformed_generic_cap_token_preserves_raw_header_bytes() {
    let handle = LoopDecisionHandle {
        sid: 7,
        lane: 3,
        scope: ScopeId::loop_scope(1),
    };
    let mut header = [0u8; super::CAP_HEADER_LEN];
    CapHeader::new(
        SessionId::new(handle.sid),
        Lane::new(handle.lane as u32),
        5,
        LoopContinueKind::TAG,
        LoopContinueKind::OP,
        LoopContinueKind::PATH,
        CapShot::One,
        LoopContinueKind::SCOPE,
        0,
        1,
        2,
        LoopContinueKind::encode_handle(&handle),
    )
    .encode(&mut header);
    header[8] = 0xFF;

    let token = token_from_wire::<LoopContinueKind>([0u8; super::CAP_NONCE_LEN], header);

    assert!(matches!(token.control_header(), Err(CapError)));
    assert_eq!(token.raw_header(), header);
}

#[test]
fn malformed_generic_cap_token_decode_handle_fails_closed_for_unit_kind() {
    let handle = EndpointHandle::new(SessionId::new(9), Lane::new(2), 4);
    let mut header = [0u8; super::CAP_HEADER_LEN];
    CapHeader::new(
        handle.sid,
        handle.lane,
        handle.role,
        EndpointResource::TAG,
        ControlOp::Fence,
        ControlPath::Local,
        CapShot::One,
        ControlScopeKind::None,
        0,
        0,
        0,
        EndpointResource::encode_handle(&handle),
    )
    .encode(&mut header);
    header[9] = 0xFF;

    let token = token_from_wire::<()>([0u8; super::CAP_NONCE_LEN], header);

    assert!(matches!(token.control_header(), Err(CapError)));
    assert!(matches!(token.decode_handle(), Err(CapError)));
}

#[test]
fn endpoint_header_rejects_noncanonical_decodable_fields() {
    fn mutate_tag(header: &mut [u8; super::CAP_HEADER_LEN]) {
        header[7] = LoopContinueKind::TAG;
    }

    fn mutate_op(header: &mut [u8; super::CAP_HEADER_LEN]) {
        header[8] = ControlOp::TopologyBegin.as_u8();
    }

    fn mutate_path(header: &mut [u8; super::CAP_HEADER_LEN]) {
        header[9] = ControlPath::Wire.as_u8();
    }

    fn mutate_shot(header: &mut [u8; super::CAP_HEADER_LEN]) {
        header[10] = CapShot::Many.as_u8();
    }

    fn mutate_scope_kind(header: &mut [u8; super::CAP_HEADER_LEN]) {
        header[11] = ControlScopeKind::Route as u8;
    }

    fn mutate_flags(header: &mut [u8; super::CAP_HEADER_LEN]) {
        header[12] = 0x01;
    }

    fn mutate_scope_id(header: &mut [u8; super::CAP_HEADER_LEN]) {
        header[13..15].copy_from_slice(&1u16.to_be_bytes());
    }

    fn mutate_epoch(header: &mut [u8; super::CAP_HEADER_LEN]) {
        header[15..17].copy_from_slice(&1u16.to_be_bytes());
    }

    let cases: &[(&str, fn(&mut [u8; super::CAP_HEADER_LEN]))] = &[
        ("tag", mutate_tag),
        ("op", mutate_op),
        ("path", mutate_path),
        ("shot", mutate_shot),
        ("scope_kind", mutate_scope_kind),
        ("flags", mutate_flags),
        ("scope_id", mutate_scope_id),
        ("epoch", mutate_epoch),
    ];

    for (name, mutate) in cases {
        let token = endpoint_token_with_mutated_header(*mutate);
        assert!(
            token.control_header().is_ok(),
            "{name} mutation must stay within decodable header space",
        );
        assert!(
            matches!(token.endpoint_header(), Err(CapError)),
            "{name} mutation must be rejected by endpoint canonical validation",
        );
    }
}

#[test]
fn endpoint_identity_rejects_decodable_handle_payload_mismatches() {
    fn endpoint_token_with_mutated_handle(
        mutate: fn(&mut [u8; super::CAP_HANDLE_LEN]),
    ) -> GenericCapToken<EndpointResource> {
        let mut header = endpoint_header_fixture();
        let handle = &mut header[super::CAP_CONTROL_HEADER_FIXED_LEN
            ..super::CAP_CONTROL_HEADER_FIXED_LEN + super::CAP_HANDLE_LEN];
        let handle: &mut [u8; super::CAP_HANDLE_LEN] =
            handle.try_into().expect("endpoint handle payload must fit");
        mutate(handle);
        token_from_wire::<EndpointResource>([0u8; super::CAP_NONCE_LEN], header)
    }

    fn mutate_sid(handle: &mut [u8; super::CAP_HANDLE_LEN]) {
        handle[0] ^= 0x01;
    }

    fn mutate_lane(handle: &mut [u8; super::CAP_HANDLE_LEN]) {
        handle[4] ^= 0x01;
    }

    fn mutate_role(handle: &mut [u8; super::CAP_HANDLE_LEN]) {
        handle[5] ^= 0x01;
    }

    fn mutate_trailing_padding(handle: &mut [u8; super::CAP_HANDLE_LEN]) {
        handle[6] = 0x7F;
    }

    let cases: &[(&str, fn(&mut [u8; super::CAP_HANDLE_LEN]))] = &[
        ("sid", mutate_sid),
        ("lane", mutate_lane),
        ("role", mutate_role),
        ("trailing_padding", mutate_trailing_padding),
    ];

    for (name, mutate) in cases {
        let token = endpoint_token_with_mutated_handle(*mutate);
        assert!(
            token.control_header().is_ok(),
            "{name} mutation must preserve fixed header decoding",
        );
        assert!(
            token.decode_handle().is_ok(),
            "{name} mutation must stay in decodable handle space",
        );
        assert!(
            matches!(token.endpoint_header(), Err(CapError)),
            "{name} mutation must be rejected by endpoint header canonical validation",
        );
        assert!(
            matches!(token.endpoint_identity(), Err(CapError)),
            "{name} mutation must be rejected by endpoint identity validation",
        );
    }
}

#[cfg(feature = "std")]
mod sampled_roundtrip_tests {
    use super::*;

    #[test]
    fn handle_view_roundtrip_samples() {
        for sid in [0, 1, 7, 999] {
            for lane in [0, 1, 3, 63] {
                for role in [0, 1, 15] {
                    assert_endpoint_handle_view_roundtrip(sid, lane, role);
                }
            }
        }
    }

    fn assert_endpoint_handle_view_roundtrip(sid: u32, lane: u32, role: u8) {
        let sid = SessionId::new(sid);
        let lane = Lane::new(lane);
        let handle = EndpointHandle::new(sid, lane, role);
        let payload = EndpointResource::encode_handle(&handle);
        let view = HandleView::<EndpointResource>::decode(&payload, None).expect("decode");
        assert_eq!(view.handle(), &handle);
        assert_eq!(view.bytes(), &payload);
    }

    #[test]
    fn handle_view_loop_continue_roundtrip_samples() {
        for generation in [0, 1, 42, 9999] {
            for lane in [0, 1, 127, 255] {
                assert_loop_continue_handle_view_roundtrip(generation, lane);
            }
        }
    }

    fn assert_loop_continue_handle_view_roundtrip(generation: u32, lane: u8) {
        let handle = LoopDecisionHandle {
            sid: generation,
            lane,
            scope: ScopeId::loop_scope(1),
        };
        let payload = LoopContinueKind::encode_handle(&handle);
        let view =
            HandleView::<LoopContinueKind>::decode(&payload, Some(handle.scope)).expect("decode");
        assert_eq!(view.handle(), &handle);
        assert_eq!(view.bytes(), &payload);
    }
}
