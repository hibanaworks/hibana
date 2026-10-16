mod seal {
    pub trait Sealed {
        const SOURCE_PAYLOAD: crate::g::SourcePayload;
    }
}

/// Sealed through `Message`; ordinary codecs cannot declare an owned endpoint.
trait PayloadSpec {
    const SOURCE: crate::g::SourcePayload;
}

impl<P> PayloadSpec for P
where
    P: crate::transport::wire::WireEncode + crate::transport::wire::WirePayload,
{
    const SOURCE: crate::g::SourcePayload = crate::g::SourcePayload::Wire {
        schema: P::SCHEMA_ID,
    };
}

impl<const ROLE: u8> PayloadSpec for crate::Endpoint<'_, ROLE> {
    const SOURCE: crate::g::SourcePayload =
        crate::g::SourcePayload::Endpoint(crate::g::EndpointKey {
            component: 0,
            role: ROLE,
        });
}

/// Public message shape carried by `g::Msg`.
pub trait Message: seal::Sealed {
    /// Logical label associated with the choreography message.
    const LOGICAL_LABEL: u8;
    /// Payload type named by the choreography.
    type Payload;
}

impl<const LOGICAL_LABEL: u8, P: PayloadSpec> Message for crate::g::Msg<LOGICAL_LABEL, P> {
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = P;
}

impl<const LOGICAL_LABEL: u8, P: PayloadSpec> seal::Sealed for crate::g::Msg<LOGICAL_LABEL, P> {
    const SOURCE_PAYLOAD: crate::g::SourcePayload = P::SOURCE;
}

#[inline(always)]
pub(crate) const fn source_payload<M: Message>() -> crate::g::SourcePayload {
    M::SOURCE_PAYLOAD
}

#[inline(always)]
pub(crate) const fn payload_schema<M: Message>() -> u32
where
    M::Payload: crate::transport::wire::WirePayload,
{
    <M::Payload as crate::transport::wire::WirePayload>::SCHEMA_ID
}
