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
    control::cap::mint::{AllowsCanonical, MintConfig, MintConfigMarker, ResourceKind},
    endpoint::{SendResult, control::ControlOutcome, kernel},
    global::{ControlHandling, ControlPayloadKind, MessageSpec, SendableLabel},
    transport::wire::WireEncode,
};

type EndpointBinding<'r> = BindingHandle<'r>;

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

struct SendFuture<'e, 'a, 'r, const ROLE: u8, M>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: ControlPayloadKind,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    'r: 'a,
{
    endpoint: *mut super::Endpoint<'r, ROLE>,
    desc: kernel::SendDesc,
    completed: bool,
    _borrow: PhantomData<&'e mut EndpointBinding<'r>>,
    _payload: PhantomData<&'a M::Payload>,
    _msg: PhantomData<M>,
}

#[inline]
pub(crate) fn send_desc<M>() -> kernel::SendDesc
where
    M: MessageSpec + SendableLabel,
    M::ControlKind: ControlPayloadKind,
{
    let handling = match <M::ControlKind as ControlPayloadKind>::HANDLING {
        ControlHandling::None => kernel::SendHandling::None,
        ControlHandling::Canonical => kernel::SendHandling::Canonical,
        ControlHandling::External => kernel::SendHandling::External {
            auto_mint_external:
                <<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind as ResourceKind>::AUTO_MINT_EXTERNAL,
        },
    };
    let expects_control = !matches!(
        <M::ControlKind as ControlPayloadKind>::HANDLING,
        ControlHandling::None
    );
    let resource_tag = if expects_control {
        Some(
            <<<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind as ResourceKind>::TAG,
        )
    } else {
        None
    };
    kernel::SendDesc::new(<M as MessageSpec>::LABEL, expects_control, handling, resource_tag)
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
    M::ControlKind: ControlPayloadKind,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    MintConfig: MintConfigMarker<Policy: AllowsCanonical>,
{
    #[inline]
    pub(crate) fn send<'a, A>(
        self,
        arg: A,
    ) -> impl Future<
        Output = SendResult<
            ControlOutcome<
                'r,
                <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
            >,
        >,
    > + 'a
    where
        A: FlowSendArg<'a, M>,
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
        SendFuture::<'e, 'a, 'r, ROLE, M> {
            endpoint,
            desc,
            completed: false,
            _borrow: PhantomData,
            _payload: PhantomData,
            _msg: PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Flow<'e, 'r, ROLE, M>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: ControlPayloadKind,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    MintConfig: MintConfigMarker<Policy: AllowsCanonical>,
{
    #[inline]
    pub fn send<'a, A>(
        self,
        arg: A,
    ) -> impl Future<
        Output = SendResult<
            ControlOutcome<
                'r,
                <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind,
            >,
        >,
    > + 'a
    where
        A: FlowSendArg<'a, M>,
        M::Payload: 'a,
        M: 'a,
        A: 'a,
        'e: 'a,
        'r: 'a,
    {
        self.inner.inner.send(arg)
    }
}

impl<'e, 'a, 'r, const ROLE: u8, M> Future for SendFuture<'e, 'a, 'r, ROLE, M>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: ControlPayloadKind,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    MintConfig: MintConfigMarker<Policy: AllowsCanonical>,
    'r: 'a,
{
    type Output = SendResult<
        ControlOutcome<'r, <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>,
    >;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let endpoint = unsafe { &mut *this.endpoint };
        match endpoint.poll_send(this.desc, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(outcome)) => {
                this.completed = true;
                Poll::Ready(Ok(match outcome {
                    kernel::SendControlOutcome::None => ControlOutcome::None,
                    kernel::SendControlOutcome::Canonical(token) => {
                        ControlOutcome::Canonical(token.into_typed())
                    }
                    kernel::SendControlOutcome::External(token) => {
                        ControlOutcome::External(token.into_generic())
                    }
                }))
            }
            Poll::Ready(Err(err)) => {
                this.completed = true;
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'a, 'r, const ROLE: u8, M> Drop for SendFuture<'e, 'a, 'r, ROLE, M>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: ControlPayloadKind,
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
    MintConfig: MintConfigMarker<Policy: AllowsCanonical>,
    M::ControlKind: ControlPayloadKind,
{
    #[inline(always)]
    fn into_payload(self) -> Option<&'a M::Payload> {
        const {
            let handling = <M::ControlKind as ControlPayloadKind>::HANDLING as u8;
            let is_canonical = handling == ControlHandling::Canonical as u8;
            let is_external = handling == ControlHandling::External as u8;
            let auto_mint = <<M::ControlKind as ControlPayloadKind>::ResourceKind as ResourceKind>::AUTO_MINT_EXTERNAL;
            assert!(
                is_canonical || (is_external && auto_mint),
                "Unit () can only be used with Canonical control or External control with AUTO_MINT_EXTERNAL"
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
            let handling = <M::ControlKind as ControlPayloadKind>::HANDLING as u8;
            assert!(
                handling == ControlHandling::External as u8
                    || handling == ControlHandling::None as u8,
                "Payload reference can only be used with External or None control messages"
            );
        }
        Some(self)
    }
}
