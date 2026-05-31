use super::*;

pub(super) const MANUAL_WIRE_CONTROL_LOGICAL: u8 = 122;
pub(super) const MANUAL_WIRE_ABORT_ACK_LOGICAL: u8 = 123;
pub(super) const ABORT_ACK_ID: u16 = 0x0201;
pub(super) const MANUAL_TOKEN_NONCE_LEN: usize = 16;
pub(super) const MANUAL_TOKEN_HEADER_LEN: usize = 40;
pub(super) const MANUAL_TOKEN_LEN: usize = MANUAL_TOKEN_NONCE_LEN + MANUAL_TOKEN_HEADER_LEN;

fn encode_manual_cap_header(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    role: u8,
    tag: u8,
    op: ControlOp,
    path: ControlPath,
    shot: CapShot,
    scope_kind: ControlScopeKind,
    flags: u8,
    scope_id: u16,
    epoch: u16,
    handle: [u8; CAP_HANDLE_LEN],
) -> [u8; MANUAL_TOKEN_HEADER_LEN] {
    let mut header = [0u8; MANUAL_TOKEN_HEADER_LEN];
    header[0] = 1;
    header[1..5].copy_from_slice(&sid.raw().to_be_bytes());
    header[5] = lane.as_wire();
    header[6] = role;
    header[7] = tag;
    header[8] = op.as_u8();
    header[9] = path.as_u8();
    header[10] = shot.as_u8();
    header[11] = scope_kind as u8;
    header[12] = flags;
    header[13..15].copy_from_slice(&scope_id.to_be_bytes());
    header[15..17].copy_from_slice(&epoch.to_be_bytes());
    header[17..].copy_from_slice(&handle);
    header
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

impl ResourceKind for ManualWireControl {
    type Handle = (u32, u16);
    const TAG: u8 = 0x72;
    const NAME: &'static str = "ManualWireControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut out = [0u8; CAP_HANDLE_LEN];
        out[0..4].copy_from_slice(&handle.0.to_be_bytes());
        out[4..6].copy_from_slice(&handle.1.to_be_bytes());
        out
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok((
            u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            u16::from_be_bytes([data[4], data[5]]),
        ))
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ManualWireControl {
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = 0x0472;
    const SHOT: CapShot = CapShot::Many;
    const OP: ControlOp = ControlOp::Fence;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ManualWireAbortAckControl;

impl ResourceKind for ManualWireAbortAckControl {
    type Handle = (u32, u16);
    const TAG: u8 = 0x74;
    const NAME: &'static str = "ManualWireAbortAckControl";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut out = [0u8; CAP_HANDLE_LEN];
        out[0..4].copy_from_slice(&handle.0.to_le_bytes());
        out[4..6].copy_from_slice(&handle.1.to_le_bytes());
        out
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        Ok((
            u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            u16::from_le_bytes([data[4], data[5]]),
        ))
    }

    fn zeroize(_handle: &mut Self::Handle) {}
}

impl ControlResourceKind for ManualWireAbortAckControl {
    const SCOPE: ControlScopeKind = ControlScopeKind::Abort;
    const PATH: ControlPath = ControlPath::Wire;
    const TAP_ID: u16 = ABORT_ACK_ID;
    const SHOT: CapShot = CapShot::Many;
    const OP: ControlOp = ControlOp::AbortAck;
}

pub(super) fn manual_wire_token(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
) -> GenericCapToken<ManualWireControl> {
    let handle = ManualWireControl::encode_handle(&(sid.raw(), lane.as_wire() as u16));
    let header = encode_manual_cap_header(
        sid,
        lane,
        peer,
        ManualWireControl::TAG,
        ManualWireControl::OP,
        ManualWireControl::PATH,
        ManualWireControl::SHOT,
        ManualWireControl::SCOPE,
        0,
        ScopeId::generic(0).local_ordinal(),
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
    manual_wire_abort_ack_token_for::<ManualWireAbortAckControl>(
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
    manual_wire_abort_ack_token_for::<ManualWireAbortAckControl>(
        sid,
        lane,
        peer,
        scope_id,
        epoch,
        handle_sid,
        handle_lane,
    )
}

pub(super) fn manual_wire_abort_ack_token_for<K>(
    sid: SessionId,
    lane: hibana::integration::ids::Lane,
    peer: u8,
    scope_id: u16,
    epoch: u16,
    handle_sid: u32,
    handle_lane: u16,
) -> GenericCapToken<K>
where
    K: ControlResourceKind + ResourceKind<Handle = (u32, u16)>,
{
    let handle = K::encode_handle(&(handle_sid, handle_lane));
    let header = encode_manual_cap_header(
        sid,
        lane,
        peer,
        K::TAG,
        K::OP,
        K::PATH,
        K::SHOT,
        K::SCOPE,
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
