use crate::control::cap::mint::LocalControlKind;

use super::{
    CONTROL_PAYLOAD_LOCAL_UNIT, CONTROL_PAYLOAD_WIRE_UNIT, ControlMsgLowering, MessageControlSpec,
    StaticControlDesc, const_dsl,
};

type ControlHandleEncoder = fn(
    crate::integration::ids::SessionId,
    u8,
    u64,
) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN];

mod seal {
    pub trait Sealed {
        const ACCEPTS_EMPTY_PAYLOAD: bool;

        fn validate_payload<'a>(
            input: crate::transport::wire::Payload<'a>,
        ) -> Result<(), crate::transport::wire::CodecError>;

        fn synthetic_payload<'a>(
            scratch: &'a mut [u8],
        ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError>;

        fn decode_validated_payload<'a>(
            input: crate::transport::wire::Payload<'a>,
        ) -> <Self as super::Message>::Decoded<'a>
        where
            Self: super::Message;

        const CONTROL: Option<(u8, u8, u8, u16, u8, u8)>;
        const CONTROL_PAYLOAD: bool;
        const CONTROL_PAYLOAD_KIND: u8;
        const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder;
        const ENCODE_CONTROL_HANDLE: Option<super::ControlHandleEncoder>;
    }
}

pub(crate) use seal::Sealed as MessageRuntime;

pub(crate) fn encode_local_control_handle_for<K>(
    sid: crate::integration::ids::SessionId,
    lane: crate::control::types::Lane,
    scope_raw: u64,
) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN]
where
    K: LocalControlKind,
{
    let scope = const_dsl::ScopeId::decode_raw(scope_raw)
        .expect("local control scope ids are projected by hibana");
    K::encode_local_handle(sid, lane, scope)
}

pub(crate) fn encode_local_control_handle_wire_for<K>(
    sid: crate::integration::ids::SessionId,
    lane_wire: u8,
    scope_raw: u64,
) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN]
where
    K: LocalControlKind,
{
    encode_local_control_handle_for::<K>(
        sid,
        crate::control::types::Lane::new(lane_wire as u32),
        scope_raw,
    )
}

const _: StaticControlDesc = StaticControlDesc::of_local::<crate::g::control::LoopContinue>();
const _: ControlHandleEncoder =
    encode_local_control_handle_wire_for::<crate::g::control::LoopContinue>;

#[inline]
fn validate_control_msg_payload<K>(
    input: crate::transport::wire::Payload<'_>,
) -> Result<(), crate::transport::wire::CodecError>
where
    K: ControlMsgLowering,
{
    match K::CONTROL_PAYLOAD_KIND {
        CONTROL_PAYLOAD_LOCAL_UNIT => {
            <() as crate::transport::wire::WirePayload>::validate_payload(input)
        }
        CONTROL_PAYLOAD_WIRE_UNIT => crate::transport::wire::require_exact_len(
            input.as_bytes().len(),
            crate::control::cap::mint::CAP_TOKEN_LEN,
            "ControlMsg wire token",
        ),
        _ => Err(crate::transport::wire::CodecError::Invalid(
            "ControlMsg payload family",
        )),
    }
}

/// Public message shape carried by `g::Msg`.
pub trait Message: seal::Sealed {
    /// Logical label associated with the choreography message.
    const LOGICAL_LABEL: u8;
    /// Payload type transmitted on the wire.
    type Payload;
    /// Decoded payload view returned by `recv()` / `decode()`.
    type Decoded<'a>;
}

impl<const LOGICAL_LABEL: u8, P> Message for crate::g::Msg<LOGICAL_LABEL, P>
where
    P: crate::transport::wire::WirePayload,
    Self: seal::Sealed,
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = P;
    type Decoded<'a> = <P as crate::transport::wire::WirePayload>::Decoded<'a>;
}

impl<const LOGICAL_LABEL: u8, P> seal::Sealed for crate::g::Msg<LOGICAL_LABEL, P>
where
    P: crate::transport::wire::WirePayload,
    crate::g::Msg<LOGICAL_LABEL, P>: MessageControlSpec,
{
    const ACCEPTS_EMPTY_PAYLOAD: bool =
        <P as crate::transport::wire::WirePayload>::ACCEPTS_EMPTY_PAYLOAD;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        <P as crate::transport::wire::WirePayload>::validate_payload(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        <P as crate::transport::wire::WirePayload>::synthetic_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        <P as crate::transport::wire::WirePayload>::decode_validated_payload(input)
    }

    const CONTROL: Option<(u8, u8, u8, u16, u8, u8)> = match <Self as MessageControlSpec>::CONTROL {
        Some(desc) => Some(desc.runtime_tuple()),
        None => None,
    };
    const CONTROL_PAYLOAD: bool = <Self as MessageControlSpec>::CONTROL_PAYLOAD;
    const CONTROL_PAYLOAD_KIND: u8 = <Self as MessageControlSpec>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<P>();
    const ENCODE_CONTROL_HANDLE: Option<ControlHandleEncoder> =
        <Self as MessageControlSpec>::ENCODE_CONTROL_HANDLE;
}

#[inline]
fn synthetic_unit_payload<'a>(
    scratch: &'a mut [u8],
) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
    <() as crate::transport::wire::WirePayload>::synthetic_payload(scratch)
}

#[inline]
fn decode_unit_payload<'a>(_input: crate::transport::wire::Payload<'a>) {}

impl<const LOGICAL_LABEL: u8> Message
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::LoopContinue>
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = ();
    type Decoded<'a> = ();
}

impl<const LOGICAL_LABEL: u8> seal::Sealed
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::LoopContinue>
{
    const ACCEPTS_EMPTY_PAYLOAD: bool =
        <crate::g::control::LoopContinue as ControlMsgLowering>::CONTROL_PAYLOAD_KIND
            == CONTROL_PAYLOAD_LOCAL_UNIT;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        validate_control_msg_payload::<crate::g::control::LoopContinue>(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        synthetic_unit_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        decode_unit_payload(input)
    }

    const CONTROL: Option<(u8, u8, u8, u16, u8, u8)> =
        Some(<crate::g::control::LoopContinue as ControlMsgLowering>::CONTROL.runtime_tuple());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 =
        <crate::g::control::LoopContinue as ControlMsgLowering>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<()>();
    const ENCODE_CONTROL_HANDLE: Option<ControlHandleEncoder> =
        <crate::g::control::LoopContinue as ControlMsgLowering>::ENCODE_CONTROL_HANDLE;
}

impl<const LOGICAL_LABEL: u8> Message
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::LoopBreak>
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = ();
    type Decoded<'a> = ();
}

impl<const LOGICAL_LABEL: u8> seal::Sealed
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::LoopBreak>
{
    const ACCEPTS_EMPTY_PAYLOAD: bool =
        <crate::g::control::LoopBreak as ControlMsgLowering>::CONTROL_PAYLOAD_KIND
            == CONTROL_PAYLOAD_LOCAL_UNIT;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        validate_control_msg_payload::<crate::g::control::LoopBreak>(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        synthetic_unit_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        decode_unit_payload(input)
    }

    const CONTROL: Option<(u8, u8, u8, u16, u8, u8)> =
        Some(<crate::g::control::LoopBreak as ControlMsgLowering>::CONTROL.runtime_tuple());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 =
        <crate::g::control::LoopBreak as ControlMsgLowering>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<()>();
    const ENCODE_CONTROL_HANDLE: Option<ControlHandleEncoder> =
        <crate::g::control::LoopBreak as ControlMsgLowering>::ENCODE_CONTROL_HANDLE;
}

impl<const LOGICAL_LABEL: u8> Message
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::StateSnapshot>
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = ();
    type Decoded<'a> = ();
}

impl<const LOGICAL_LABEL: u8> seal::Sealed
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::StateSnapshot>
{
    const ACCEPTS_EMPTY_PAYLOAD: bool =
        <crate::g::control::StateSnapshot as ControlMsgLowering>::CONTROL_PAYLOAD_KIND
            == CONTROL_PAYLOAD_LOCAL_UNIT;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        validate_control_msg_payload::<crate::g::control::StateSnapshot>(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        synthetic_unit_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        decode_unit_payload(input)
    }

    const CONTROL: Option<(u8, u8, u8, u16, u8, u8)> =
        Some(<crate::g::control::StateSnapshot as ControlMsgLowering>::CONTROL.runtime_tuple());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 =
        <crate::g::control::StateSnapshot as ControlMsgLowering>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<()>();
    const ENCODE_CONTROL_HANDLE: Option<ControlHandleEncoder> =
        <crate::g::control::StateSnapshot as ControlMsgLowering>::ENCODE_CONTROL_HANDLE;
}

impl<const LOGICAL_LABEL: u8> Message
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::StateRestore>
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = ();
    type Decoded<'a> = ();
}

impl<const LOGICAL_LABEL: u8> seal::Sealed
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::StateRestore>
{
    const ACCEPTS_EMPTY_PAYLOAD: bool =
        <crate::g::control::StateRestore as ControlMsgLowering>::CONTROL_PAYLOAD_KIND
            == CONTROL_PAYLOAD_LOCAL_UNIT;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        validate_control_msg_payload::<crate::g::control::StateRestore>(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        synthetic_unit_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        decode_unit_payload(input)
    }

    const CONTROL: Option<(u8, u8, u8, u16, u8, u8)> =
        Some(<crate::g::control::StateRestore as ControlMsgLowering>::CONTROL.runtime_tuple());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 =
        <crate::g::control::StateRestore as ControlMsgLowering>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<()>();
    const ENCODE_CONTROL_HANDLE: Option<ControlHandleEncoder> =
        <crate::g::control::StateRestore as ControlMsgLowering>::ENCODE_CONTROL_HANDLE;
}

impl<const LOGICAL_LABEL: u8> Message
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TxnCommit>
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = ();
    type Decoded<'a> = ();
}

impl<const LOGICAL_LABEL: u8> seal::Sealed
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TxnCommit>
{
    const ACCEPTS_EMPTY_PAYLOAD: bool =
        <crate::g::control::TxnCommit as ControlMsgLowering>::CONTROL_PAYLOAD_KIND
            == CONTROL_PAYLOAD_LOCAL_UNIT;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        validate_control_msg_payload::<crate::g::control::TxnCommit>(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        synthetic_unit_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        decode_unit_payload(input)
    }

    const CONTROL: Option<(u8, u8, u8, u16, u8, u8)> =
        Some(<crate::g::control::TxnCommit as ControlMsgLowering>::CONTROL.runtime_tuple());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 =
        <crate::g::control::TxnCommit as ControlMsgLowering>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<()>();
    const ENCODE_CONTROL_HANDLE: Option<ControlHandleEncoder> =
        <crate::g::control::TxnCommit as ControlMsgLowering>::ENCODE_CONTROL_HANDLE;
}

impl<const LOGICAL_LABEL: u8> Message
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TxnAbort>
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = ();
    type Decoded<'a> = ();
}

impl<const LOGICAL_LABEL: u8> seal::Sealed
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TxnAbort>
{
    const ACCEPTS_EMPTY_PAYLOAD: bool =
        <crate::g::control::TxnAbort as ControlMsgLowering>::CONTROL_PAYLOAD_KIND
            == CONTROL_PAYLOAD_LOCAL_UNIT;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        validate_control_msg_payload::<crate::g::control::TxnAbort>(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        synthetic_unit_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        decode_unit_payload(input)
    }

    const CONTROL: Option<(u8, u8, u8, u16, u8, u8)> =
        Some(<crate::g::control::TxnAbort as ControlMsgLowering>::CONTROL.runtime_tuple());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 =
        <crate::g::control::TxnAbort as ControlMsgLowering>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<()>();
    const ENCODE_CONTROL_HANDLE: Option<ControlHandleEncoder> =
        <crate::g::control::TxnAbort as ControlMsgLowering>::ENCODE_CONTROL_HANDLE;
}

impl<const LOGICAL_LABEL: u8> Message
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TopologyBegin>
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = ();
    type Decoded<'a> = ();
}

impl<const LOGICAL_LABEL: u8> seal::Sealed
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TopologyBegin>
{
    const ACCEPTS_EMPTY_PAYLOAD: bool =
        <crate::g::control::TopologyBegin as ControlMsgLowering>::CONTROL_PAYLOAD_KIND
            == CONTROL_PAYLOAD_LOCAL_UNIT;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        validate_control_msg_payload::<crate::g::control::TopologyBegin>(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        synthetic_unit_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        decode_unit_payload(input)
    }

    const CONTROL: Option<(u8, u8, u8, u16, u8, u8)> =
        Some(<crate::g::control::TopologyBegin as ControlMsgLowering>::CONTROL.runtime_tuple());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 =
        <crate::g::control::TopologyBegin as ControlMsgLowering>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<()>();
    const ENCODE_CONTROL_HANDLE: Option<ControlHandleEncoder> =
        <crate::g::control::TopologyBegin as ControlMsgLowering>::ENCODE_CONTROL_HANDLE;
}

impl<const LOGICAL_LABEL: u8> Message
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TopologyAck>
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = ();
    type Decoded<'a> = ();
}

impl<const LOGICAL_LABEL: u8> seal::Sealed
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TopologyAck>
{
    const ACCEPTS_EMPTY_PAYLOAD: bool =
        <crate::g::control::TopologyAck as ControlMsgLowering>::CONTROL_PAYLOAD_KIND
            == CONTROL_PAYLOAD_LOCAL_UNIT;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        validate_control_msg_payload::<crate::g::control::TopologyAck>(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        synthetic_unit_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        decode_unit_payload(input)
    }

    const CONTROL: Option<(u8, u8, u8, u16, u8, u8)> =
        Some(<crate::g::control::TopologyAck as ControlMsgLowering>::CONTROL.runtime_tuple());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 =
        <crate::g::control::TopologyAck as ControlMsgLowering>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<()>();
    const ENCODE_CONTROL_HANDLE: Option<ControlHandleEncoder> =
        <crate::g::control::TopologyAck as ControlMsgLowering>::ENCODE_CONTROL_HANDLE;
}

impl<const LOGICAL_LABEL: u8> Message
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TopologyCommit>
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = ();
    type Decoded<'a> = ();
}

impl<const LOGICAL_LABEL: u8> seal::Sealed
    for crate::g::ControlMsg<LOGICAL_LABEL, crate::g::control::TopologyCommit>
{
    const ACCEPTS_EMPTY_PAYLOAD: bool =
        <crate::g::control::TopologyCommit as ControlMsgLowering>::CONTROL_PAYLOAD_KIND
            == CONTROL_PAYLOAD_LOCAL_UNIT;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        validate_control_msg_payload::<crate::g::control::TopologyCommit>(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        synthetic_unit_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        decode_unit_payload(input)
    }

    const CONTROL: Option<(u8, u8, u8, u16, u8, u8)> =
        Some(<crate::g::control::TopologyCommit as ControlMsgLowering>::CONTROL.runtime_tuple());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 =
        <crate::g::control::TopologyCommit as ControlMsgLowering>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<()>();
    const ENCODE_CONTROL_HANDLE: Option<ControlHandleEncoder> =
        <crate::g::control::TopologyCommit as ControlMsgLowering>::ENCODE_CONTROL_HANDLE;
}
