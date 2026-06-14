mod seal {
    pub trait Sealed {
        const ALLOWS_ZERO_LENGTH: bool;

        fn validate_payload<'a>(
            input: crate::transport::wire::Payload<'a>,
        ) -> Result<(), crate::transport::wire::CodecError>;

        fn zero_payload<'a>(
            scratch: &'a mut [u8],
        ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError>;

        fn decode_validated_payload<'a>(
            input: crate::transport::wire::Payload<'a>,
        ) -> <Self as super::Message>::Decoded<'a>
        where
            Self: super::Message;

        const ENCODE_PAYLOAD: unsafe fn(
            *const (),
            &mut [u8],
        )
            -> Result<usize, crate::transport::wire::CodecError>;
    }
}

pub(crate) use seal::Sealed as MessageRuntime;

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
{
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = P;
    type Decoded<'a> = <P as crate::transport::wire::WirePayload>::Decoded<'a>;
}

impl<const LOGICAL_LABEL: u8, P> seal::Sealed for crate::g::Msg<LOGICAL_LABEL, P>
where
    P: crate::transport::wire::WirePayload,
{
    const ALLOWS_ZERO_LENGTH: bool = <P as crate::transport::wire::WirePayload>::ALLOWS_ZERO_LENGTH;

    #[inline]
    fn validate_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> Result<(), crate::transport::wire::CodecError> {
        <P as crate::transport::wire::WirePayload>::validate_payload(input)
    }

    #[inline]
    fn zero_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<crate::transport::wire::Payload<'a>, crate::transport::wire::CodecError> {
        <P as crate::transport::wire::WirePayload>::zero_payload(scratch)
    }

    #[inline]
    fn decode_validated_payload<'a>(
        input: crate::transport::wire::Payload<'a>,
    ) -> <Self as Message>::Decoded<'a> {
        <P as crate::transport::wire::WirePayload>::decode_validated_payload(input)
    }

    const ENCODE_PAYLOAD: unsafe fn(
        *const (),
        &mut [u8],
    ) -> Result<usize, crate::transport::wire::CodecError> =
        crate::transport::wire::erased_encoder::<P>();
}
