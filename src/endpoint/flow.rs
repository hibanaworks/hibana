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
    control::cap::{mint::ControlResourceKind, typed_tokens::CapRegisteredToken},
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
    type Output = CapRegisteredToken<'r, K>;

    #[inline]
    fn finish_send(outcome: kernel::SendControlOutcome<'r>) -> SendResult<Self::Output> {
        match outcome {
            kernel::SendControlOutcome::None => Err(SendError::PhaseInvariant),
            kernel::SendControlOutcome::Registered(token) => Ok(token.into_typed::<K>()),
            kernel::SendControlOutcome::Emitted(_) => Err(SendError::PhaseInvariant),
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

struct SendFuture<'e, 'a, 'r, const ROLE: u8, M, A>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: SendOutcomeKind<'r>,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    A: FlowSendArg<'a, M>,
    'r: 'a,
{
    endpoint: *mut super::Endpoint<'r, ROLE>,
    desc: kernel::SendDesc,
    completed: bool,
    _borrow: PhantomData<&'e mut EndpointBinding<'r>>,
    _payload: PhantomData<&'a M::Payload>,
    _msg: PhantomData<M>,
    _arg: PhantomData<A>,
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
{
    #[inline]
    pub(crate) fn send<'a, A>(self, arg: A) -> impl Future<Output = SendResult<A::Output<'r>>> + 'a
    where
        A: FlowSendArg<'a, M>,
        M::ControlKind: SendOutcomeKind<'r>,
        A::Output<'r>: 'a,
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
        SendFuture::<'e, 'a, 'r, ROLE, M, A> {
            endpoint,
            desc,
            completed: false,
            _borrow: PhantomData,
            _payload: PhantomData,
            _msg: PhantomData,
            _arg: PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Flow<'e, 'r, ROLE, M>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: SendOutcomeKind<'r>,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
{
    #[inline]
    pub fn send<'a, A>(self, arg: A) -> impl Future<Output = SendResult<A::Output<'r>>> + 'a
    where
        A: FlowSendArg<'a, M>,
        M::ControlKind: SendOutcomeKind<'r>,
        A::Output<'r>: 'a,
        M::Payload: 'a,
        M: 'a,
        A: 'a,
        'e: 'a,
        'r: 'a,
    {
        self.inner.inner.send(arg)
    }
}

impl<'e, 'a, 'r, const ROLE: u8, M, A> Future for SendFuture<'e, 'a, 'r, ROLE, M, A>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: SendOutcomeKind<'r>,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    A: FlowSendArg<'a, M>,
    'r: 'a,
{
    type Output = SendResult<A::Output<'r>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let endpoint = unsafe { &mut *this.endpoint };
        match endpoint.poll_send(this.desc, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(outcome)) => {
                this.completed = true;
                Poll::Ready(<A as FlowSendArg<'a, M>>::finish_send::<'r>(outcome))
            }
            Poll::Ready(Err(err)) => {
                this.completed = true;
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'a, 'r, const ROLE: u8, M, A> Drop for SendFuture<'e, 'a, 'r, ROLE, M, A>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: SendOutcomeKind<'r>,
    <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    A: FlowSendArg<'a, M>,
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
    use super::{SendFuture, SendOutcomeKind};
    use crate::{
        control::cap::{
            mint::{
                CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN, CAP_TOKEN_LEN, CapHeader, CapShot,
                ControlResourceKind, ResourceKind,
            },
            resource_kinds::{LoopContinueKind, LoopDecisionHandle},
            typed_tokens::{RawRegisteredCapToken, RegisteredTokenParts},
        },
        endpoint::kernel::SendControlOutcome,
        global::const_dsl::ScopeId,
        rendezvous::{
            capability::{CapEntry, CapReleaseCtx, CapTable},
            tables::StateSnapshotTable,
        },
        substrate::{Lane, SessionId},
    };
    use core::{cell::Cell, mem::size_of};
    use std::vec;

    type SendFut = SendFuture<'static, 'static, 'static, 0, crate::g::Msg<7, ()>, ()>;

    fn make_test_token_bytes(
        nonce: [u8; CAP_NONCE_LEN],
        handle: &LoopDecisionHandle,
    ) -> [u8; CAP_TOKEN_LEN] {
        let handle_bytes = LoopContinueKind::encode_handle(handle);
        let mut header = [0u8; CAP_HEADER_LEN];
        CapHeader::new(
            SessionId::new(handle.sid),
            Lane::new(handle.lane as u32),
            0,
            LoopContinueKind::TAG,
            LoopContinueKind::LABEL,
            LoopContinueKind::OP,
            LoopContinueKind::PATH,
            CapShot::Many,
            LoopContinueKind::SCOPE,
            0,
            handle.scope.local_ordinal(),
            0,
            handle_bytes,
        )
        .encode(&mut header);

        let mut bytes = [0u8; CAP_TOKEN_LEN];
        bytes[..CAP_NONCE_LEN].copy_from_slice(&nonce);
        bytes[CAP_NONCE_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN].copy_from_slice(&header);
        bytes[CAP_NONCE_LEN + CAP_HEADER_LEN..].copy_from_slice(&[0u8; CAP_TAG_LEN]);
        bytes
    }

    #[test]
    fn send_future_stays_within_size_budget() {
        assert!(
            size_of::<SendFut>() <= 48,
            "SendFuture must stay within the localside size budget"
        );
    }

    #[test]
    fn dropping_registered_send_outcome_releases_capability() {
        let table = CapTable::new();
        let lane = Lane::new(3);
        let sid = SessionId::new(42);
        let role = 0u8;
        let nonce = [0xAC; CAP_NONCE_LEN];
        let handle = LoopDecisionHandle {
            sid: sid.raw(),
            lane: lane.raw() as u16,
            scope: ScopeId::loop_scope(2),
        };
        let bytes = make_test_token_bytes(nonce, &handle);

        table
            .insert_entry(CapEntry {
                sid,
                lane_raw: lane.as_wire(),
                kind_tag: LoopContinueKind::TAG,
                shot_state: CapShot::Many.as_u8(),
                role,
                mint_revision: 1,
                consumed_revision: 0,
                released_revision: 0,
                nonce,
                handle: LoopContinueKind::encode_handle(&handle),
            })
            .expect("insert succeeds");

        let mut snapshot_storage = vec![0u8; StateSnapshotTable::storage_bytes(1)];
        let mut snapshots = StateSnapshotTable::empty();
        unsafe {
            snapshots.bind_from_storage(snapshot_storage.as_mut_ptr(), lane.raw(), 1);
        }
        let revisions = Cell::new(0u64);

        let outcome =
            <LoopContinueKind as SendOutcomeKind<'_>>::finish_send(SendControlOutcome::Registered(
                RawRegisteredCapToken::from_parts(RegisteredTokenParts::from_registered_bytes(
                    bytes,
                    nonce,
                    CapReleaseCtx::new(&table, &snapshots, &revisions, lane),
                )),
            ))
            .expect("registered local control send");
        drop(outcome);

        assert!(
            table
                .claim_by_nonce(
                    &nonce,
                    sid,
                    lane,
                    LoopContinueKind::TAG,
                    role,
                    CapShot::Many,
                    2,
                )
                .is_err(),
            "dropping the send outcome must release the registered capability"
        );
    }

    #[test]
    fn emitted_control_send_outcome_is_phase_invariant() {
        let bytes = [0u8; CAP_TOKEN_LEN];
        let err =
            <LoopContinueKind as SendOutcomeKind<'_>>::finish_send(SendControlOutcome::Emitted(
                crate::endpoint::kernel::RawCapFlowToken::from_bytes(bytes),
            ))
            .expect_err("wire-emitted control sends must resolve through registered owner output");

        assert!(matches!(err, crate::endpoint::SendError::PhaseInvariant));
    }
}

/// Sealed trait for type-level send argument resolution.
pub trait FlowSendArg<'a, M>
where
    M: MessageSpec + SendableLabel,
{
    type Output<'r>
    where
        M::ControlKind: SendOutcomeKind<'r>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
        'r: 'a;

    fn into_payload(self) -> Option<&'a M::Payload>
    where
        Self: Sized;

    fn finish_send<'r>(outcome: kernel::SendControlOutcome<'r>) -> SendResult<Self::Output<'r>>
    where
        M::ControlKind: SendOutcomeKind<'r>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
        'r: 'a;
}

impl<'a, M> FlowSendArg<'a, M> for ()
where
    M: MessageSpec + SendableLabel,
    M::ControlKind: ControlPayloadKind,
{
    type Output<'r>
        = <M::ControlKind as SendOutcomeKind<'r>>::Output
    where
        M::ControlKind: SendOutcomeKind<'r>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
        'r: 'a;

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
                "Unit () can only be used with local control or auto-minted wire control"
            );
        }
        None
    }

    #[inline(always)]
    fn finish_send<'r>(outcome: kernel::SendControlOutcome<'r>) -> SendResult<Self::Output<'r>>
    where
        M::ControlKind: SendOutcomeKind<'r>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
        'r: 'a,
    {
        <M::ControlKind as SendOutcomeKind<'r>>::finish_send(outcome)
    }
}

impl<'a, M> FlowSendArg<'a, M> for &'a M::Payload
where
    M: MessageSpec + SendableLabel,
    M::ControlKind: ControlPayloadKind,
{
    type Output<'r>
        = ()
    where
        M::ControlKind: SendOutcomeKind<'r>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
        'r: 'a;

    #[inline(always)]
    fn into_payload(self) -> Option<&'a M::Payload> {
        const {
            assert!(
                match <M as MessageSpec>::CONTROL {
                    None => true,
                    Some(desc) =>
                        matches!(desc.path(), crate::control::cap::mint::ControlPath::Wire),
                },
                "Payload reference can only be used with data messages or wire control tokens"
            );
        }
        Some(self)
    }

    #[inline(always)]
    fn finish_send<'r>(outcome: kernel::SendControlOutcome<'r>) -> SendResult<Self::Output<'r>>
    where
        M::ControlKind: SendOutcomeKind<'r>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
        'r: 'a,
    {
        match outcome {
            kernel::SendControlOutcome::None | kernel::SendControlOutcome::Emitted(_) => Ok(()),
            _ => Err(SendError::PhaseInvariant),
        }
    }
}
