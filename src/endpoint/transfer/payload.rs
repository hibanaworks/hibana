use crate::endpoint::{
    futures::{BranchRecvFuture, RecvFuture},
    send::SendFuture,
};
use crate::{
    Endpoint, EndpointError, RouteBranch,
    global::{Message, PayloadSpec},
    transport::wire::{WireEncode, WirePayload},
};
use core::future::Future;

impl<P: WireEncode + WirePayload> PayloadSpec for P {
    const SOURCE: crate::g::SourcePayload = crate::g::SourcePayload::Wire {
        schema: P::SCHEMA_ID,
    };
    type SendArgument<'a, 'r>
        = &'a P
    where
        P: 'a,
        'r: 'a;
    type Received<'e, 'r>
        = P::Decoded<'e>
    where
        'r: 'e;

    fn send<'a, 'e, 'r, M, const ROLE: u8>(
        endpoint: &'e mut Endpoint<'r, ROLE>,
        payload: &'a P,
    ) -> impl Future<Output = Result<(), EndpointError>> + 'a
    where
        M: Message<Payload = Self> + 'a,
        Self: 'a,
        'e: 'a,
        'r: 'e,
    {
        let endpoint = core::ptr::from_mut(endpoint);
        let logical_label = <M as crate::g::Message>::LOGICAL_LABEL;
        let payload_kind = crate::global::payload_kind::<M>();
        crate::endpoint::send::SendFuture::pending_direct(
            endpoint,
            logical_label,
            payload_kind,
            payload,
        )
    }
    fn recv<'e, 'r, M, const ROLE: u8>(
        endpoint: &'e mut Endpoint<'r, ROLE>,
    ) -> impl Future<Output = Result<Self::Received<'e, 'r>, EndpointError>> + 'e
    where
        M: Message<Payload = Self> + 'e,
        'r: 'e,
    {
        RecvFuture::<'e, 'r, ROLE, M>::new(endpoint)
    }
    fn branch_send<'a, 'e, 'r, M, const ROLE: u8>(
        branch: RouteBranch<'e, 'r, ROLE>,
        payload: &'a P,
    ) -> impl Future<Output = Result<(), EndpointError>> + 'a
    where
        M: Message<Payload = Self> + 'a,
        Self: 'a,
        'e: 'a,
        'r: 'e,
    {
        let branch = core::mem::ManuallyDrop::new(branch);
        let endpoint = branch.endpoint;
        let logical_label = <M as crate::g::Message>::LOGICAL_LABEL;
        let payload_kind = crate::global::payload_kind::<M>();
        let result = /* SAFETY: the consumed branch owns the in-flight kernel borrow until the returned future completes or drops. */ unsafe {
            (&mut *endpoint).begin_public_send_state(logical_label, payload_kind)
        };
        if let Err(error) = result {
            return SendFuture::ready_error(error);
        }
        SendFuture::pending_armed(endpoint, payload)
    }
    fn branch_recv<'e, 'r, M, const ROLE: u8>(
        branch: RouteBranch<'e, 'r, ROLE>,
    ) -> impl Future<Output = Result<Self::Received<'e, 'r>, EndpointError>> + 'e
    where
        M: Message<Payload = Self> + 'e,
        'r: 'e,
    {
        BranchRecvFuture::<'e, 'r, ROLE, M>::new(branch)
    }
}

impl<const RESOURCE: u8> PayloadSpec for Endpoint<'_, RESOURCE> {
    const SOURCE: crate::g::SourcePayload =
        crate::g::SourcePayload::Endpoint(crate::g::EndpointKey {
            component: 0,
            role: RESOURCE,
        });
    type SendArgument<'a, 'r>
        = Endpoint<'r, RESOURCE>
    where
        Self: 'a,
        'r: 'a;
    type Received<'e, 'r>
        = Endpoint<'r, RESOURCE>
    where
        'r: 'e;

    fn send<'a, 'e, 'r, M, const ROLE: u8>(
        endpoint: &'e mut Endpoint<'r, ROLE>,
        payload: Endpoint<'r, RESOURCE>,
    ) -> impl Future<Output = Result<(), EndpointError>> + 'a
    where
        M: Message<Payload = Self> + 'a,
        Self: 'a,
        'e: 'a,
        'r: 'e,
    {
        super::futures::OwnedSendFuture::direct(endpoint, M::LOGICAL_LABEL, payload)
    }
    fn recv<'e, 'r, M, const ROLE: u8>(
        endpoint: &'e mut Endpoint<'r, ROLE>,
    ) -> impl Future<Output = Result<Self::Received<'e, 'r>, EndpointError>> + 'e
    where
        M: Message<Payload = Self> + 'e,
        'r: 'e,
    {
        super::futures::OwnedRecvFuture::<ROLE, RESOURCE>::direct(endpoint, M::LOGICAL_LABEL)
    }
    fn branch_send<'a, 'e, 'r, M, const ROLE: u8>(
        branch: RouteBranch<'e, 'r, ROLE>,
        payload: Endpoint<'r, RESOURCE>,
    ) -> impl Future<Output = Result<(), EndpointError>> + 'a
    where
        M: Message<Payload = Self> + 'a,
        Self: 'a,
        'e: 'a,
        'r: 'e,
    {
        super::futures::OwnedSendFuture::branch(branch, M::LOGICAL_LABEL, payload)
    }
    fn branch_recv<'e, 'r, M, const ROLE: u8>(
        branch: RouteBranch<'e, 'r, ROLE>,
    ) -> impl Future<Output = Result<Self::Received<'e, 'r>, EndpointError>> + 'e
    where
        M: Message<Payload = Self> + 'e,
        'r: 'e,
    {
        super::futures::OwnedRecvFuture::<ROLE, RESOURCE>::branch(branch, M::LOGICAL_LABEL)
    }
}
