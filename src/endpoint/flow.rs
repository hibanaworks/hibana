//! Capability-oriented flow pipeline tying typestate metadata and transport
//! emission into a single affine value.

use core::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    binding::BindingHandle,
    control::cap::mint::{
        AllowsEndpointMint, ControlResourceKind, GenericCapToken, MintConfig, MintConfigMarker,
    },
    endpoint::{SendError, SendResult, kernel},
    global::{ControlDesc, ControlPayloadKind, MessageSpec, SendableLabel},
    transport::wire::WireEncode,
};

type EndpointBinding<'r> = BindingHandle<'r>;

pub trait SendOutcomeKind<'r>: ControlPayloadKind {
    type Output;

    fn finish_send(outcome: kernel::SendControlOutcome<'r>) -> SendResult<Self::Output>;
}

impl<'r> SendOutcomeKind<'r> for () {
    type Output = ();

    #[inline]
    fn finish_send(outcome: kernel::SendControlOutcome<'r>) -> SendResult<Self::Output> {
        match outcome {
            kernel::SendControlOutcome::None => Ok(()),
            _ => Err(SendError::PhaseInvariant),
        }
    }
}

impl<'r, K> SendOutcomeKind<'r> for K
where
    K: ControlResourceKind + 'r,
{
    type Output = GenericCapToken<K>;

    #[inline]
    fn finish_send(outcome: kernel::SendControlOutcome<'r>) -> SendResult<Self::Output> {
        match outcome {
            kernel::SendControlOutcome::None => Err(SendError::PhaseInvariant),
            kernel::SendControlOutcome::Registered(token) => {
                Ok(token.into_typed::<K>().into_handle())
            }
            kernel::SendControlOutcome::Emitted(token) => Ok(token.into_generic()),
        }
    }
}

/// Affine flow handle for a pending send transition.
pub(crate) struct CapFlow<'e, 'r, const ROLE: u8, M>
where
    M: MessageSpec + SendableLabel,
{
    endpoint: *mut super::Endpoint<'r, ROLE>,
    preview: kernel::SendPreview,
    desc: kernel::SendDesc,
    _msg: PhantomData<(&'e mut super::Endpoint<'r, ROLE>, M)>,
}

struct FlowInner<'e, 'r, const ROLE: u8, M>
where
    M: MessageSpec + SendableLabel,
{
    inner: CapFlow<'e, 'r, ROLE, M>,
}

pub struct Flow<'e, 'r, const ROLE: u8, M>
where
    M: MessageSpec + SendableLabel,
{
    inner: FlowInner<'e, 'r, ROLE, M>,
}

struct SendFuture<'e, 'a, 'r, const ROLE: u8, M, O>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: SendOutcomeKind<'r, Output = O>,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    'r: 'a,
{
    endpoint: *mut super::Endpoint<'r, ROLE>,
    desc: kernel::SendDesc,
    completed: bool,
    _borrow: PhantomData<&'e mut EndpointBinding<'r>>,
    _payload: PhantomData<&'a M::Payload>,
    _msg: PhantomData<M>,
    _output: PhantomData<O>,
}

#[inline]
pub(crate) fn send_desc<M>() -> kernel::SendDesc
where
    M: MessageSpec + SendableLabel,
    M::ControlKind: ControlPayloadKind,
{
    let control = <M as MessageSpec>::CONTROL.map(ControlDesc::from_static);
    let expects_control = <M::ControlKind as ControlPayloadKind>::IS_CONTROL;
    kernel::SendDesc::new(
        <M as MessageSpec>::LABEL,
        expects_control,
        control,
        <M::ControlKind as ControlPayloadKind>::ENCODE_CONTROL_HANDLE,
    )
}

impl<'e, 'r, const ROLE: u8, M> Flow<'e, 'r, ROLE, M>
where
    M: MessageSpec + SendableLabel,
{
    #[inline]
    pub(crate) fn from_cap_flow(inner: CapFlow<'e, 'r, ROLE, M>) -> Self {
        Self {
            inner: FlowInner { inner },
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> CapFlow<'e, 'r, ROLE, M>
where
    M: MessageSpec + SendableLabel,
{
    pub(crate) fn new(
        endpoint: *mut super::Endpoint<'r, ROLE>,
        preview: kernel::SendPreview,
        desc: kernel::SendDesc,
    ) -> Self {
        Self {
            endpoint,
            preview,
            desc,
            _msg: PhantomData,
        }
    }

    fn into_parts(
        self,
    ) -> (
        *mut super::Endpoint<'r, ROLE>,
        kernel::SendPreview,
        kernel::SendDesc,
    ) {
        (self.endpoint, self.preview, self.desc)
    }
}

impl<'e, 'r, const ROLE: u8, M> CapFlow<'e, 'r, ROLE, M>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: SendOutcomeKind<'r>,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    MintConfig: MintConfigMarker<Policy: AllowsEndpointMint>,
{
    #[inline]
    pub(crate) fn send<'a, A, O>(
        self,
        arg: A,
    ) -> impl Future<Output = SendResult<O>> + 'a
    where
        A: FlowSendArg<'a, M>,
        M::ControlKind: SendOutcomeKind<'r, Output = O>,
        O: 'a,
        M::Payload: 'a,
        M: 'a,
        A: 'a,
        'e: 'a,
        'r: 'a,
    {
        let (endpoint, preview, desc) = self.into_parts();
        let payload = arg
            .into_payload()
            .map(kernel::RawSendPayload::from_typed::<M::Payload>);
        unsafe {
            (&mut *endpoint).init_public_send_state(preview, payload);
        }
        SendFuture::<'e, 'a, 'r, ROLE, M, O> {
            endpoint,
            desc,
            completed: false,
            _borrow: PhantomData,
            _payload: PhantomData,
            _msg: PhantomData,
            _output: PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Flow<'e, 'r, ROLE, M>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: SendOutcomeKind<'r>,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    MintConfig: MintConfigMarker<Policy: AllowsEndpointMint>,
{
    #[inline]
    #[allow(private_bounds)]
    pub fn send<'a, A, O>(
        self,
        arg: A,
    ) -> impl Future<Output = SendResult<O>> + 'a
    where
        A: FlowSendArg<'a, M>,
        M::ControlKind: SendOutcomeKind<'r, Output = O>,
        O: 'a,
        M::Payload: 'a,
        M: 'a,
        A: 'a,
        'e: 'a,
        'r: 'a,
    {
        self.inner.inner.send(arg)
    }
}

impl<'e, 'a, 'r, const ROLE: u8, M, O> Future for SendFuture<'e, 'a, 'r, ROLE, M, O>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: SendOutcomeKind<'r, Output = O>,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    MintConfig: MintConfigMarker<Policy: AllowsEndpointMint>,
    'r: 'a,
{
    type Output = SendResult<O>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let endpoint = unsafe { &mut *this.endpoint };
        match endpoint.poll_send(this.desc, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(outcome)) => {
                this.completed = true;
                Poll::Ready(<M::ControlKind as SendOutcomeKind<'r>>::finish_send(outcome))
            }
            Poll::Ready(Err(err)) => {
                this.completed = true;
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'a, 'r, const ROLE: u8, M, O> Drop for SendFuture<'e, 'a, 'r, ROLE, M, O>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: SendOutcomeKind<'r, Output = O>,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    'r: 'a,
{
    fn drop(&mut self) {
        if !self.completed {
            unsafe {
                (&mut *self.endpoint).reset_public_send_state();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SendFuture;
    use core::mem::size_of;

    type SendFut = SendFuture<'static, 'static, 'static, 0, crate::g::Msg<7, ()>, ()>;

    #[test]
    fn send_future_stays_within_size_budget() {
        assert!(
            size_of::<SendFut>() <= 48,
            "SendFuture must stay within the localside size budget"
        );
    }
}

/// Sealed trait for type-level send argument resolution.
pub trait FlowSendArg<'a, M>
where
    M: MessageSpec + SendableLabel,
{
    fn into_payload(self) -> Option<&'a M::Payload>
    where
        Self: Sized;
}

impl<'a, M> FlowSendArg<'a, M> for ()
where
    M: MessageSpec + SendableLabel,
    MintConfig: MintConfigMarker<Policy: AllowsEndpointMint>,
    M::ControlKind: ControlPayloadKind,
{
    #[inline(always)]
    fn into_payload(self) -> Option<&'a M::Payload> {
        const {
            assert!(
                match <M as MessageSpec>::CONTROL {
                    Some(desc) => match desc.path() {
                        crate::control::cap::mint::ControlPath::Local => true,
                        crate::control::cap::mint::ControlPath::Wire => desc.auto_mint_wire(),
                    },
                    None => false,
                },
                "Unit () can only be used with local control or wire control with AUTO_MINT_WIRE"
            );
        }
        None
    }
}

impl<'a, M> FlowSendArg<'a, M> for &'a M::Payload
where
    M: MessageSpec + SendableLabel,
    M::ControlKind: ControlPayloadKind,
{
    #[inline(always)]
    fn into_payload(self) -> Option<&'a M::Payload> {
        const {
            assert!(
                !<M::ControlKind as ControlPayloadKind>::IS_CONTROL
                    || matches!(
                        <M as MessageSpec>::CONTROL,
                        Some(desc)
                            if matches!(
                                desc.path(),
                                crate::control::cap::mint::ControlPath::Wire
                            )
                    ),
                "Payload reference can only be used with wire control or data messages"
            );
        }
        Some(self)
    }
}
