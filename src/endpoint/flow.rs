//! Send pipeline tying descriptor metadata and transport emission into a single
//! affine value.

use core::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    endpoint::{SendResult, kernel},
    global::{ControlDesc, ControlPayloadKind, MessageSpec, SendableLabel},
    transport::wire::WireEncode,
};

pub struct Flow<'e, 'r, const ROLE: u8, M>
where
    M: MessageSpec + SendableLabel,
{
    endpoint: *mut super::Endpoint<'r, ROLE>,
    preview: kernel::SendPreview,
    desc: kernel::SendRuntimeDesc,
    _msg: PhantomData<(&'e mut super::Endpoint<'r, ROLE>, M)>,
}

pub(crate) trait ErasedSendInput<'a, M>: sealed::Sealed<M>
where
    M: MessageSpec + SendableLabel,
{
    fn into_payload(self) -> Option<&'a M::Payload>;
}

mod sealed {
    pub trait Sealed<M> {}
    impl<M> Sealed<M> for () {}
    impl<'a, M> Sealed<M> for &'a M::Payload where M: super::MessageSpec {}
}

struct RawSendFuture<'e, 'r, const ROLE: u8> {
    endpoint: *mut super::Endpoint<'r, ROLE>,
    completed: bool,
    _borrow: PhantomData<&'e mut crate::binding::BindingHandle<'r>>,
}

pub(crate) struct SendFuture<'e, 'r, const ROLE: u8> {
    raw: RawSendFuture<'e, 'r, ROLE>,
}

#[inline]
pub(crate) fn send_desc<M>() -> kernel::SendRuntimeSpec
where
    M: MessageSpec + SendableLabel,
    M::ControlKind: ControlPayloadKind,
{
    let control = <M as MessageSpec>::CONTROL.map(ControlDesc::from_static);
    let expects_control = <M::ControlKind as ControlPayloadKind>::IS_CONTROL;
    kernel::SendRuntimeSpec::new(
        <M as MessageSpec>::LOGICAL_LABEL,
        expects_control,
        control,
        <M::ControlKind as ControlPayloadKind>::ENCODE_CONTROL_HANDLE,
    )
}

impl<'e, 'r, const ROLE: u8, M> Flow<'e, 'r, ROLE, M>
where
    M: MessageSpec + SendableLabel,
{
    pub(crate) fn new(
        endpoint: *mut super::Endpoint<'r, ROLE>,
        preview: kernel::SendPreview,
        desc: kernel::SendRuntimeSpec,
    ) -> Self {
        let desc = desc.bind_frame_label(preview.frame_label());
        Self {
            endpoint,
            preview,
            desc,
            _msg: PhantomData,
        }
    }
}

impl<'e, 'r, const ROLE: u8, M> Flow<'e, 'r, ROLE, M>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
{
    #[inline]
    #[expect(
        private_bounds,
        reason = "send argument resolution is sealed to () and &Payload"
    )]
    pub fn send<'a, A>(
        self,
        arg: A,
    ) -> impl Future<Output = SendResult<()>> + 'a + use<'a, 'e, 'r, A, M, ROLE>
    where
        A: ErasedSendInput<'a, M>,
        M::Payload: 'a,
        M: 'a,
        A: 'a,
        'e: 'a,
        'r: 'a,
    {
        let payload = arg
            .into_payload()
            .map(kernel::RawSendPayload::from_typed::<M::Payload>);
        unsafe {
            (&mut *self.endpoint).init_public_send_state(self.desc, self.preview, payload);
        }
        SendFuture {
            raw: RawSendFuture::new(self.endpoint),
        }
    }
}

impl<'e, 'r, const ROLE: u8> RawSendFuture<'e, 'r, ROLE> {
    #[inline]
    fn new(endpoint: *mut super::Endpoint<'r, ROLE>) -> Self {
        Self {
            endpoint,
            completed: false,
            _borrow: PhantomData,
        }
    }

    #[inline]
    fn poll_raw(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<SendResult<kernel::SendControlOutcome<'r>>> {
        let endpoint = unsafe { &mut *self.endpoint };
        match endpoint.poll_send(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(outcome)) => {
                self.completed = true;
                Poll::Ready(Ok(outcome))
            }
            Poll::Ready(Err(err)) => {
                self.completed = true;
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<'e, 'r, const ROLE: u8> Future for SendFuture<'e, 'r, ROLE> {
    type Output = SendResult<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        match this.raw.poll_raw(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(outcome)) => Poll::Ready(finish_send(outcome)),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }
}

impl<'e, 'r, const ROLE: u8> Drop for RawSendFuture<'e, 'r, ROLE> {
    fn drop(&mut self) {
        if !self.completed {
            unsafe {
                (&mut *self.endpoint).reset_public_send_state();
            }
        }
    }
}

#[inline(always)]
fn finish_send(outcome: kernel::SendControlOutcome<'_>) -> SendResult<()> {
    match outcome {
        kernel::SendControlOutcome::None => Ok(()),
        kernel::SendControlOutcome::Emitted(token) => {
            let _ = token.bytes();
            Ok(())
        }
        kernel::SendControlOutcome::Registered(token) => {
            drop(token);
            Ok(())
        }
    }
}

impl<'a, M> ErasedSendInput<'a, M> for ()
where
    M: MessageSpec + SendableLabel,
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
                "Unit () can only be used with local control or auto-minted wire control"
            );
        }
        None
    }
}

impl<'a, M> ErasedSendInput<'a, M> for &'a M::Payload
where
    M: MessageSpec + SendableLabel,
{
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
}

#[cfg(test)]
mod tests {
    use super::{SendFuture, finish_send};
    use crate::{
        control::cap::{
            mint::{
                CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN, CAP_TOKEN_LEN, CapHeader, CapShot,
                ControlResourceKind, ResourceKind,
            },
            resource_kinds::{LoopContinueKind, LoopDecisionHandle},
            typed_tokens::RawRegisteredCapToken,
        },
        endpoint::kernel::SendControlOutcome,
        global::const_dsl::ScopeId,
        rendezvous::{
            capability::{CapEntry, CapReleaseCtx, CapTable},
            tables::StateSnapshotTable,
        },
        substrate::ids::{Lane, SessionId},
    };
    use core::{cell::Cell, mem::size_of};
    use std::vec;

    type SendFut = SendFuture<'static, 'static, 0>;
    type SendFutAltRole = SendFuture<'static, 'static, 1>;

    fn cap_table() -> CapTable {
        const CAP_TABLE_SLOTS: usize = 64;
        let mut table = CapTable::empty();
        let storage = vec![Option::<CapEntry>::None; CAP_TABLE_SLOTS].into_boxed_slice();
        let ptr = std::boxed::Box::leak(storage).as_mut_ptr().cast::<u8>();
        unsafe {
            table.bind_from_storage(ptr, CAP_TABLE_SLOTS, 0);
        }
        table
    }

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
        const WORD: usize = size_of::<usize>();
        assert!(
            size_of::<SendFut>() <= 3 * WORD,
            "SendFuture must stay within the 3-word budget"
        );
    }

    #[test]
    fn send_future_layout_is_message_independent() {
        assert_eq!(size_of::<SendFut>(), size_of::<SendFutAltRole>());
    }

    #[test]
    fn registered_send_outcome_is_released_by_finish_send() {
        let table = cap_table();
        let lane = Lane::new(3);
        let sid = SessionId::new(42);
        let role = 0u8;
        let nonce = [0xAC; CAP_NONCE_LEN];
        let handle = LoopDecisionHandle {
            sid: sid.raw(),
            lane: lane.as_wire(),
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

        finish_send(SendControlOutcome::Registered(
            RawRegisteredCapToken::from_registered_bytes(
                bytes,
                nonce,
                CapReleaseCtx::new(&table, &snapshots, &revisions, lane),
            ),
        ))
        .expect("registered local control send");

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
            "finishing a registered send must release the registered capability"
        );
    }

    #[test]
    fn emitted_control_send_outcome_completes_erased_send() {
        let bytes = [0u8; CAP_TOKEN_LEN];
        finish_send(SendControlOutcome::Emitted(
            crate::endpoint::kernel::RawEmittedCapToken::new(bytes),
        ))
        .expect("wire-emitted control sends complete through erased output");
    }
}
