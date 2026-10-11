mod seal {
    pub trait Sealed {}
}

/// Public message shape carried by `g::Msg`.
pub trait Message: seal::Sealed {
    /// Logical label associated with the choreography message.
    const LOGICAL_LABEL: u8;
    /// Payload type named by the choreography.
    type Payload;
}

impl<const LOGICAL_LABEL: u8, P> Message for crate::g::Msg<LOGICAL_LABEL, P> {
    const LOGICAL_LABEL: u8 = LOGICAL_LABEL;
    type Payload = P;
}

impl<const LOGICAL_LABEL: u8, P> seal::Sealed for crate::g::Msg<LOGICAL_LABEL, P> {}

#[inline(always)]
pub(crate) const fn payload_schema<M: Message>() -> u32
where
    M::Payload: crate::transport::wire::WirePayload,
{
    <M::Payload as crate::transport::wire::WirePayload>::SCHEMA_ID
}
