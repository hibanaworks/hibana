use super::{
    CapError, CapHeader, CapShot, ControlOp, ControlPath, GenericCapToken, WireControlKind,
};
use crate::{
    control::{
        cap::resource_kinds::{LoopContinueKind, LoopDecisionHandle},
        types::{Lane, SessionId},
    },
    global::MessageRuntime,
    global::const_dsl::ControlScopeKind,
    transport::wire::{CodecError, Payload, WireEncode},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EndpointHandle {
    sid: SessionId,
    lane: Lane,
    role: u8,
}

impl EndpointHandle {
    const fn new(sid: SessionId, lane: Lane, role: u8) -> Self {
        Self { sid, lane, role }
    }
}

enum EndpointResource {}

impl WireControlKind for EndpointResource {
    const TAG: u8 = 0;
}

fn encode_endpoint_identity(handle: &EndpointHandle) -> [u8; super::CAP_HANDLE_LEN] {
    let mut data = [0u8; super::CAP_HANDLE_LEN];
    data[0..4].copy_from_slice(&handle.sid.raw().to_be_bytes());
    data[4] = handle.lane.as_wire();
    data[5] = handle.role;
    data
}

fn decode_endpoint_identity(data: [u8; super::CAP_HANDLE_LEN]) -> Result<EndpointHandle, CapError> {
    Ok(EndpointHandle::new(
        SessionId::new(u32::from_be_bytes([data[0], data[1], data[2], data[3]])),
        Lane::new(u32::from(data[4])),
        data[5],
    ))
}

const fn is_canonical_endpoint_header(header: CapHeader) -> bool {
    header.tag() == <EndpointResource as WireControlKind>::TAG
        && matches!(header.op(), ControlOp::Fence)
        && matches!(header.path(), ControlPath::Local)
        && matches!(header.shot(), CapShot::One)
        && matches!(header.scope_kind(), ControlScopeKind::None)
        && header.flags() == 0
        && header.scope_id() == 0
        && header.epoch() == 0
}

fn decode_canonical_endpoint_identity(
    token: &GenericCapToken<EndpointResource>,
) -> Result<(CapHeader, EndpointHandle), CapError> {
    let header = token.control_header()?;
    if !is_canonical_endpoint_header(header) {
        return Err(CapError);
    }

    let handle = decode_endpoint_identity(token.handle_bytes())?;
    let matches_header =
        handle.sid == header.sid() && handle.lane == header.lane() && handle.role == header.role();
    let matches_encoding = encode_endpoint_identity(&handle) == token.handle_bytes();
    if !matches_header || !matches_encoding {
        return Err(CapError);
    }

    Ok((header, handle))
}

fn endpoint_header(token: &GenericCapToken<EndpointResource>) -> Result<CapHeader, CapError> {
    decode_canonical_endpoint_identity(token).map(|(header, _handle)| header)
}

fn endpoint_identity(
    token: &GenericCapToken<EndpointResource>,
) -> Result<EndpointHandle, CapError> {
    decode_canonical_endpoint_identity(token).map(|(_header, handle)| handle)
}

fn token_raw_header<K: WireControlKind>(token: &GenericCapToken<K>) -> [u8; super::CAP_HEADER_LEN] {
    let mut bytes = [0u8; super::CAP_TOKEN_LEN];
    token
        .encode_into(&mut bytes)
        .expect("test token wire image must encode");
    bytes[super::CAP_NONCE_LEN..super::CAP_NONCE_LEN + super::CAP_HEADER_LEN]
        .try_into()
        .expect("test token header slice must fit")
}

fn endpoint_header_fixture() -> [u8; super::CAP_HEADER_LEN] {
    let handle = EndpointHandle::new(SessionId::new(7), Lane::new(3), 1);
    let mut header = [0u8; super::CAP_HEADER_LEN];
    CapHeader::new(
        handle.sid,
        handle.lane,
        handle.role,
        <EndpointResource as WireControlKind>::TAG,
        ControlOp::Fence,
        ControlPath::Local,
        CapShot::One,
        ControlScopeKind::None,
        0,
        0,
        0,
        encode_endpoint_identity(&handle),
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
}

fn endpoint_token_with_mutated_header(
    mutate: fn(&mut [u8; super::CAP_HEADER_LEN]),
) -> GenericCapToken<EndpointResource> {
    let mut header = endpoint_header_fixture();
    mutate(&mut header);
    token_from_wire::<EndpointResource>([0u8; super::CAP_NONCE_LEN], header)
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
        ControlScopeKind::Policy,
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
        SessionId::new(7),
        Lane::new(3),
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

    let token = token_from_wire::<ExactLengthWireKind>([0u8; super::CAP_NONCE_LEN], header);

    assert!(matches!(token.control_header(), Err(CapError)));
    assert_eq!(token_raw_header(&token), header);
}

#[test]
fn malformed_generic_cap_token_header_fails_closed_for_untyped_token() {
    let handle = EndpointHandle::new(SessionId::new(9), Lane::new(2), 4);
    let mut header = [0u8; super::CAP_HEADER_LEN];
    CapHeader::new(
        handle.sid,
        handle.lane,
        handle.role,
        <EndpointResource as WireControlKind>::TAG,
        ControlOp::Fence,
        ControlPath::Local,
        CapShot::One,
        ControlScopeKind::None,
        0,
        0,
        0,
        encode_endpoint_identity(&handle),
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
            matches!(endpoint_header(&token), Err(CapError)),
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
            decode_endpoint_identity(token.handle_bytes()).is_ok(),
            "{name} mutation must stay in decodable handle space",
        );
        assert!(
            matches!(endpoint_header(&token), Err(CapError)),
            "{name} mutation must be rejected by endpoint header canonical validation",
        );
        assert!(
            matches!(endpoint_identity(&token), Err(CapError)),
            "{name} mutation must be rejected by endpoint identity validation",
        );
    }
}
