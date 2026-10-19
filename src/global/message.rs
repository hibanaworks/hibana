mod seal {
    pub trait Sealed {
        const SOURCE_PAYLOAD: crate::g::SourcePayload;
    }
}

/// Sealed through `Message`; ordinary codecs cannot declare an owned endpoint.
#[doc(hidden)]
pub trait PayloadSpec: Sized {
    const SOURCE: crate::g::SourcePayload;
    type SendArgument<'a, 'r>
    where
        Self: 'a,
        'r: 'a;
    type Received<'e, 'r>
    where
        'r: 'e;
    fn send<'a, 'e, 'r, M, const ROLE: u8>(
        endpoint: crate::endpoint::target::OperationTarget<'e, 'r, ROLE>,
        payload: Self::SendArgument<'a, 'r>,
    ) -> impl core::future::Future<Output = Result<(), crate::EndpointError>> + 'a
    where
        M: Message<Payload = Self> + 'a,
        Self: 'a,
        'e: 'a,
        'r: 'e;
    fn recv<'e, 'r, M, const ROLE: u8>(
        endpoint: crate::endpoint::target::OperationTarget<'e, 'r, ROLE>,
    ) -> impl core::future::Future<Output = Result<Self::Received<'e, 'r>, crate::EndpointError>> + 'e
    where
        M: Message<Payload = Self> + 'e,
        'r: 'e;
    fn branch_send<'a, 'e, 'r, M, const ROLE: u8>(
        branch: crate::RouteBranch<'e, 'r, ROLE>,
        payload: Self::SendArgument<'a, 'r>,
    ) -> impl core::future::Future<Output = Result<(), crate::EndpointError>> + 'a
    where
        M: Message<Payload = Self> + 'a,
        Self: 'a,
        'e: 'a,
        'r: 'e;
    fn branch_recv<'e, 'r, M, const ROLE: u8>(
        branch: crate::RouteBranch<'e, 'r, ROLE>,
    ) -> impl core::future::Future<Output = Result<Self::Received<'e, 'r>, crate::EndpointError>> + 'e
    where
        M: Message<Payload = Self> + 'e,
        'r: 'e;
}

/// Public message shape carried by `g::Msg`.
pub trait Message: seal::Sealed {
    /// Logical label associated with the choreography message.
    const LOGICAL_LABEL: u8;
    /// Payload type named by the choreography.
    type Payload: PayloadSpec;
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
pub(crate) const fn payload_kind<M: Message>() -> crate::eff::EventPayload {
    match source_payload::<M>() {
        crate::g::SourcePayload::Wire { schema } => crate::eff::EventPayload::Wire(schema),
        crate::g::SourcePayload::Endpoint(key) => crate::eff::EventPayload::Endpoint(key.role),
    }
}
