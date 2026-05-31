use super::*;

pub(super) const MANUAL_WIRE_CONTROL_LOGICAL: u8 = 122;
pub(super) const MANUAL_WIRE_ABORT_ACK_LOGICAL: u8 = 123;
pub(super) const ABORT_ACK_ID: u16 = 0x0201;
pub(super) const MANUAL_TOKEN_NONCE_LEN: usize = 16;
pub(super) const MANUAL_TOKEN_HEADER_LEN: usize = 40;
const MANUAL_TOKEN_FIXED_HEADER_LEN: usize = 17;
const MANUAL_TOKEN_HANDLE_LEN: usize = MANUAL_TOKEN_HEADER_LEN - MANUAL_TOKEN_FIXED_HEADER_LEN;
pub(super) const MANUAL_TOKEN_LEN: usize = MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN;

fn encode_manual_cap_header(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    role: u8,
    tag: u8,
    op: u8,
    path: u8,
    shot: u8,
    scope_kind: u8,
    flags: u8,
    scope_id: u16,
    epoch: u16,
    handle: [u8; MANUAL_TOKEN_HANDLE_LEN],
) -> [u8; MANUAL_TOKEN_HEADER_LEN] {
    let mut header = [0u8; MANUAL_TOKEN_HEADER_LEN];
    header[0] = 1;
    header[1..5].copy_from_slice(&sid.raw().to_be_bytes());
    header[5] = lane.as_wire();
    header[6] = role;
    header[7] = tag;
    header[8] = op;
    header[9] = path;
    header[10] = shot;
    header[11] = scope_kind;
    header[12] = flags;
    header[13..15].copy_from_slice(&scope_id.to_be_bytes());
    header[15..17].copy_from_slice(&epoch.to_be_bytes());
    header[17..].copy_from_slice(&handle);
    header
}

fn wire_effect_op(effect: WireControlEffect) -> u8 {
    match effect {
        WireControlEffect::Fence => 11,
        WireControlEffect::StateSnapshot => 3,
        WireControlEffect::StateRestore => 4,
        WireControlEffect::TxCommit => 12,
        WireControlEffect::TxAbort => 13,
        WireControlEffect::AbortBegin => 9,
        WireControlEffect::AbortAck => 10,
        WireControlEffect::TopologyBegin => 5,
        WireControlEffect::TopologyAck => 6,
        WireControlEffect::TopologyCommit => 7,
    }
}

fn wire_effect_scope(effect: WireControlEffect) -> u8 {
    match effect {
        WireControlEffect::Fence => 6,
        WireControlEffect::StateSnapshot
        | WireControlEffect::StateRestore
        | WireControlEffect::TxCommit
        | WireControlEffect::TxAbort => 2,
        WireControlEffect::AbortBegin | WireControlEffect::AbortAck => 3,
        WireControlEffect::TopologyBegin
        | WireControlEffect::TopologyAck
        | WireControlEffect::TopologyCommit => 4,
    }
}

#[test]
fn add_rendezvous_from_config_returns_attach_error_at_callsite() {
    let clock = CounterClock::new();
    let mut tap_buf = [TapEvent::zero(); 128];
    let mut slab = [0u8; 4096];
    let mut kit_storage =
        SessionKitStorage::<TestTransport, DefaultLabelUniverse, CounterClock, 0>::uninit();
    let kit = kit_storage.init();
    let config = Config::from_resources((&mut tap_buf, &mut slab), clock);

    let add_line = line!() + 2;
    let error = kit
        .add_rendezvous_from_config(config, TestTransport::default())
        .expect_err("zero-capacity kit must reject rendezvous registration");

    assert_eq!(error.operation(), "add_rendezvous");
    assert!(
        error
            .file()
            .ends_with("tests/cursor_send_recv/manual_wire_support.rs")
    );
    assert_eq!(error.line(), add_line);
}

pub(super) fn assert_progress_invariant_fault(error: &hibana::EndpointError) {
    let rendered = format!("{error:?}");
    if rendered.contains("SessionFault") {
        assert!(
            rendered.contains("ProgressInvariantViolated"),
            "progress invariant poison must preserve terminal cause: {rendered}"
        );
    } else {
        assert!(
            rendered.contains("PhaseInvariant"),
            "first progress invariant fault must preserve root evidence: {rendered}"
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ManualWireControl;

impl WireControlKind for ManualWireControl {
    const TAG: u8 = 0x72;
    const NAME: &'static str = "ManualWireControl";
    const TAP_ID: u16 = 0x0472;
    const EFFECT: WireControlEffect = WireControlEffect::Fence;
}

fn encode_manual_wire_handle(handle_sid: u32, handle_lane: u16) -> [u8; MANUAL_TOKEN_HANDLE_LEN] {
    let mut out = [0u8; MANUAL_TOKEN_HANDLE_LEN];
    out[0..4].copy_from_slice(&handle_sid.to_be_bytes());
    out[4..6].copy_from_slice(&handle_lane.to_be_bytes());
    out
}

pub(super) fn manual_wire_control_handle(token: GenericCapToken<ManualWireControl>) -> (u32, u16) {
    let bytes = token.into_bytes();
    let offset = MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_FIXED_HEADER_LEN;
    (
        u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]),
        u16::from_be_bytes([bytes[offset + 4], bytes[offset + 5]]),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ManualWireAbortAckControl;

impl WireControlKind for ManualWireAbortAckControl {
    const TAG: u8 = 0x74;
    const NAME: &'static str = "ManualWireAbortAckControl";
    const TAP_ID: u16 = ABORT_ACK_ID;
    const EFFECT: WireControlEffect = WireControlEffect::AbortAck;
}

fn encode_manual_wire_abort_ack_handle(
    handle_sid: u32,
    handle_lane: u16,
) -> [u8; MANUAL_TOKEN_HANDLE_LEN] {
    let mut out = [0u8; MANUAL_TOKEN_HANDLE_LEN];
    out[0..4].copy_from_slice(&handle_sid.to_le_bytes());
    out[4..6].copy_from_slice(&handle_lane.to_le_bytes());
    out
}

pub(super) fn manual_wire_token(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
) -> GenericCapToken<ManualWireControl> {
    let handle = encode_manual_wire_handle(sid.raw(), lane.as_wire() as u16);
    let header = encode_manual_cap_header(
        sid,
        lane,
        peer,
        ManualWireControl::TAG,
        wire_effect_op(ManualWireControl::EFFECT),
        1,
        1,
        wire_effect_scope(ManualWireControl::EFFECT),
        0,
        0,
        0,
        handle,
    );

    let mut bytes = [0u8; MANUAL_TOKEN_LEN];
    bytes[..MANUAL_TOKEN_NONCE_LEN].copy_from_slice(&[0xAB; MANUAL_TOKEN_NONCE_LEN]);
    bytes[MANUAL_TOKEN_NONCE_LEN..MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN]
        .copy_from_slice(&header);
    GenericCapToken::from_bytes(bytes)
}

pub(super) fn manual_wire_abort_ack_token(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
) -> GenericCapToken<ManualWireAbortAckControl> {
    manual_wire_abort_ack_token_for(
        sid,
        lane,
        peer,
        scope_id,
        epoch,
        sid.raw(),
        lane.as_wire() as u16,
    )
}

pub(super) fn manual_wire_abort_ack_token_with_handle(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
    handle_sid: u32,
    handle_lane: u16,
) -> GenericCapToken<ManualWireAbortAckControl> {
    manual_wire_abort_ack_token_for(sid, lane, peer, scope_id, epoch, handle_sid, handle_lane)
}

pub(super) fn manual_wire_abort_ack_token_for(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
    handle_sid: u32,
    handle_lane: u16,
) -> GenericCapToken<ManualWireAbortAckControl> {
    let handle = encode_manual_wire_abort_ack_handle(handle_sid, handle_lane);
    let header = encode_manual_cap_header(
        sid,
        lane,
        peer,
        ManualWireAbortAckControl::TAG,
        wire_effect_op(ManualWireAbortAckControl::EFFECT),
        1,
        1,
        wire_effect_scope(ManualWireAbortAckControl::EFFECT),
        0,
        scope_id,
        epoch,
        handle,
    );

    let mut bytes = [0u8; MANUAL_TOKEN_LEN];
    bytes[..MANUAL_TOKEN_NONCE_LEN].copy_from_slice(&[0xCD; MANUAL_TOKEN_NONCE_LEN]);
    bytes[MANUAL_TOKEN_NONCE_LEN..MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN]
        .copy_from_slice(&header);
    GenericCapToken::from_bytes(bytes)
}
