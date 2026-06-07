use crate::control::cap::mint::{LocalControlKind, WireControlKind};

use super::{MessageControlSpec, StaticControlDesc, const_dsl};

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

        const CONTROL: Option<super::StaticControlDesc>;
        const CONTROL_PAYLOAD: bool;
        const CONTROL_PAYLOAD_KIND: u8;
        const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder;
        const ENCODE_CONTROL_HANDLE: Option<
            fn(
                crate::integration::ids::SessionId,
                u8,
                u64,
            ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
        >;
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

const _: StaticControlDesc =
    StaticControlDesc::of_local::<crate::control::cap::resource_kinds::LoopContinueKind>();
const _: fn(
    crate::integration::ids::SessionId,
    u8,
    u64,
) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN] =
    encode_local_control_handle_wire_for::<crate::control::cap::resource_kinds::LoopContinueKind>;

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

    const CONTROL: Option<StaticControlDesc> = <Self as MessageControlSpec>::CONTROL;
    const CONTROL_PAYLOAD: bool = <Self as MessageControlSpec>::CONTROL_PAYLOAD;
    const CONTROL_PAYLOAD_KIND: u8 = <Self as MessageControlSpec>::CONTROL_PAYLOAD_KIND;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<P>();
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::integration::ids::SessionId,
            u8,
            u64,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    > = <Self as MessageControlSpec>::ENCODE_CONTROL_HANDLE;
}

impl<const LOGICAL_LABEL: u8, K> Message
    for crate::g::Msg<LOGICAL_LABEL, crate::control::cap::mint::GenericCapToken<K>>
where
    K: WireControlKind,
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = crate::control::cap::mint::GenericCapToken<K>;
    type Decoded<'a> = crate::control::cap::mint::GenericCapToken<K>;
}

impl<const LOGICAL_LABEL: u8, K> seal::Sealed
    for crate::g::Msg<LOGICAL_LABEL, crate::control::cap::mint::GenericCapToken<K>>
where
    K: WireControlKind,
{
    const ACCEPTS_EMPTY_PAYLOAD: bool = false;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        crate::transport::wire::require_exact_len(
            input.as_bytes().len(),
            crate::control::cap::mint::CAP_TOKEN_LEN,
            "GenericCapToken payload",
        )
    }

    #[inline]
    fn synthetic_payload<'a>(
        _scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        Err(crate::transport::wire::CodecError::Invalid(
            "GenericCapToken synthetic payload",
        ))
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        let mut bytes = [0u8; crate::control::cap::mint::CAP_TOKEN_LEN];
        bytes.copy_from_slice(input.as_bytes());
        crate::control::cap::mint::GenericCapToken::from_raw_bytes(bytes)
    }

    const CONTROL: Option<StaticControlDesc> = Some(StaticControlDesc::of_wire::<K>());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 = 2;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<crate::control::cap::mint::GenericCapToken<K>>();
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::integration::ids::SessionId,
            u8,
            u64,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    > = None;
}

impl<const LOGICAL_LABEL: u8, K> Message for crate::g::ControlMsg<LOGICAL_LABEL, K>
where
    K: LocalControlKind,
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = ();
    type Decoded<'a> = ();
}

impl<const LOGICAL_LABEL: u8, K> seal::Sealed for crate::g::ControlMsg<LOGICAL_LABEL, K>
where
    K: LocalControlKind,
{
    const ACCEPTS_EMPTY_PAYLOAD: bool = true;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        <() as crate::transport::wire::WirePayload>::validate_payload(input)
    }

    #[inline]
    fn synthetic_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        <() as crate::transport::wire::WirePayload>::synthetic_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        <() as crate::transport::wire::WirePayload>::decode_validated_payload(input)
    }

    const CONTROL: Option<StaticControlDesc> = Some(StaticControlDesc::of_local::<K>());
    const CONTROL_PAYLOAD: bool = true;
    const CONTROL_PAYLOAD_KIND: u8 = 1;
    const ENCODE_PAYLOAD: crate::transport::wire::ErasedEncoder =
        crate::transport::wire::erased_encoder::<()>();
    const ENCODE_CONTROL_HANDLE: Option<
        fn(
            crate::integration::ids::SessionId,
            u8,
            u64,
        ) -> [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    > = Some(encode_local_control_handle_wire_for::<K>);
}
