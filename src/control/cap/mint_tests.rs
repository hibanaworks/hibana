use super::{
    CapError, CapHeader, CapShot, ControlOp, ControlPath, ControlScopeKind, E0, EndpointHandle,
    EndpointResource, GenericCapToken, Owner, WireControlEffect, WireControlKind,
};
use crate::{
    control::{
        brand::with_brand,
        cap::resource_kinds::{LoopContinueKind, LoopDecisionHandle},
        types::{Lane, SessionId},
    },
    global::MessageRuntime,
    transport::wire::{CodecError, Payload},
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
        EndpointResource::encode_identity(&handle),
    )
    .encode(&mut header);
    header
}

fn token_from_wire<K>(
    nonce: [u8; super::CAP_NONCE_LEN],
    header: [u8; super::CAP_HEADER_LEN],
) -> GenericCapToken<K> {
    let mut bytes = [0u8; super::CAP_TOKEN_LEN];
    bytes[..super::CAP_NONCE_LEN].copy_from_slice(&nonce);
    bytes[super::CAP_NONCE_LEN..super::CAP_NONCE_LEN + super::CAP_HEADER_LEN]
        .copy_from_slice(&header);
    GenericCapToken::from_raw_bytes(bytes)
}

struct ExactLengthWireKind;

impl WireControlKind for ExactLengthWireKind {
    const TAG: u8 = 0x7E;
    const EFFECT: WireControlEffect = WireControlEffect::Fence;
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
fn generic_cap_token_preserves_opaque_wire_bytes() {
    use super::{CAP_HEADER_LEN, CAP_NONCE_LEN};

    let handle = LoopDecisionHandle::new(7, 3);
    let handle_bytes = handle.encode();

    let mut header = [0u8; CAP_HEADER_LEN];
    CapHeader::new(
        SessionId::new(7),
        Lane::new(3),
        1,
        <ExactLengthWireKind as WireControlKind>::TAG,
        ControlOp::Fence,
        crate::control::cap::mint::ControlPath::Wire,
        CapShot::Many,
        WireControlEffect::Fence.scope_kind(),
        0,
        0,
        0,
        handle_bytes,
    )
    .encode(&mut header);

    let token = token_from_wire::<ExactLengthWireKind>([0u8; CAP_NONCE_LEN], header);

    assert_eq!(token.handle_bytes(), handle_bytes);
    let header = token.control_header().expect("header");
    assert_eq!(header.sid(), SessionId::new(7));
    assert_eq!(header.lane(), Lane::new(3));
    assert_eq!(header.role(), 1);
}

#[test]
fn cap_header_decode_rejects_unknown_atomic_fields() {
    let mut raw = [0u8; super::CAP_HEADER_LEN];
    CapHeader::new(
        SessionId::new(7),
        Lane::new(3),
        1,
        <LoopContinueKind as super::LocalControlKind>::TAG,
        <LoopContinueKind as super::LocalControlKind>::OP,
        ControlPath::Local,
        CapShot::One,
        <LoopContinueKind as super::LocalControlKind>::SCOPE,
        0,
        1,
        2,
        LoopDecisionHandle::new(7, 3).encode(),
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
        <LoopContinueKind as super::LocalControlKind>::TAG,
        <LoopContinueKind as super::LocalControlKind>::OP,
        ControlPath::Local,
        CapShot::One,
        <LoopContinueKind as super::LocalControlKind>::SCOPE,
        0,
        1,
        2,
        LoopDecisionHandle::new(7, 3).encode(),
    )
    .encode(&mut raw);
    raw[12] = 0x80;

    assert!(
        matches!(CapHeader::decode(raw), Err(super::CapError)),
        "reserved control header flags must fail closed",
    );
}

#[test]
fn generic_cap_token_message_requires_exact_wire_length() {
    type TokenMsg = crate::g::Msg<0x7E, GenericCapToken<ExactLengthWireKind>>;
    let exact = [0u8; super::CAP_TOKEN_LEN];
    assert!(
        <TokenMsg as MessageRuntime>::validate_payload(Payload::new(&exact)).is_ok(),
        "explicit wire-control token messages must accept exact token bytes"
    );

    let mut short = [0u8; super::CAP_TOKEN_LEN - 1];
    short.copy_from_slice(&exact[..super::CAP_TOKEN_LEN - 1]);
    assert!(matches!(
        <TokenMsg as MessageRuntime>::validate_payload(Payload::new(&short)),
        Err(CodecError::Truncated)
    ));

    let mut trailing = [0u8; super::CAP_TOKEN_LEN + 1];
    trailing[..super::CAP_TOKEN_LEN].copy_from_slice(&exact);
    trailing[super::CAP_TOKEN_LEN] = 0xA5;
    assert!(
        matches!(
            <TokenMsg as MessageRuntime>::validate_payload(Payload::new(&trailing)),
            Err(CodecError::Invalid("GenericCapToken payload"))
        ),
        "control tokens are fixed-size and must reject ignored trailing bytes"
    );
}

#[test]
fn malformed_generic_cap_token_preserves_raw_header_bytes() {
    let handle = LoopDecisionHandle::new(7, 3);
    let mut header = [0u8; super::CAP_HEADER_LEN];
    CapHeader::new(
        SessionId::new(handle.sid()),
        Lane::new(handle.lane() as u32),
        5,
        <LoopContinueKind as super::LocalControlKind>::TAG,
        <LoopContinueKind as super::LocalControlKind>::OP,
        ControlPath::Local,
        CapShot::One,
        <LoopContinueKind as super::LocalControlKind>::SCOPE,
        0,
        1,
        2,
        handle.encode(),
    )
    .encode(&mut header);
    header[8] = 0xFF;

    let token = token_from_wire::<LoopContinueKind>([0u8; super::CAP_NONCE_LEN], header);

    assert!(matches!(token.control_header(), Err(CapError)));
    assert_eq!(token.raw_header(), header);
}

#[test]
fn malformed_generic_cap_token_header_fails_closed_for_untyped_token() {
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
        EndpointResource::encode_identity(&handle),
    )
    .encode(&mut header);
    header[9] = 0xFF;

    let token = token_from_wire::<()>([0u8; super::CAP_NONCE_LEN], header);

    assert!(matches!(token.control_header(), Err(CapError)));
}

#[test]
fn endpoint_header_rejects_noncanonical_decodable_fields() {
    fn mutate_tag(header: &mut [u8; super::CAP_HEADER_LEN]) {
        header[7] = <LoopContinueKind as super::LocalControlKind>::TAG;
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
            EndpointResource::decode_identity(token.handle_bytes()).is_ok(),
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
