//! Test-only send-path wrappers built on the erased kernel state machine.

#[cfg(test)]
use crate::{
    binding::BindingSlot,
    control::cap::mint::{AllowsCanonical, EpochTable, MintConfigMarker},
    endpoint::{
        SendResult,
        flow::{FlowSendArg, send_desc},
    },
    global::{ControlPayloadKind, MessageSpec, SendableLabel, typestate::SendMeta},
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{Transport, wire::WireEncode},
};

#[cfg(test)]
impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    super::core::CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker<Policy: AllowsCanonical>,
    B: BindingSlot + 'r,
{
    pub(crate) fn send_direct<'a, M, A>(
        &'a mut self,
        arg: A,
    ) -> impl core::future::Future<Output = SendResult<()>> + 'a
    where
        M: MessageSpec + SendableLabel + 'a,
        M::Payload: WireEncode + 'a,
        M::ControlKind: ControlPayloadKind,
        A: FlowSendArg<'a, M> + 'a,
        'r: 'a,
    {
        let desc = send_desc::<M>();
        let mut preview = Some(self.preview_flow::<M>());
        let mut payload = arg
            .into_payload()
            .map(super::lane_port::RawSendPayload::from_typed::<M::Payload>);
        let mut state = None;

        core::future::poll_fn(move |cx| {
            if state.is_none() {
                let preview = preview
                    .take()
                    .expect("send_direct future polled after completion");
                let preview = match preview {
                    Ok(preview) => preview,
                    Err(err) => return core::task::Poll::Ready(Err(err)),
                };
                let (meta, preview_cursor_index) = preview.into_parts();
                state = Some(super::core::SendState::Init {
                    meta,
                    preview_cursor_index: Some(preview_cursor_index),
                    payload: payload.take(),
                });
            }

            match self.poll_send_state(
                desc,
                state
                    .as_mut()
                    .expect("send_direct state must exist while polling"),
                cx,
            ) {
                core::task::Poll::Pending => core::task::Poll::Pending,
                core::task::Poll::Ready(Ok(_)) => core::task::Poll::Ready(Ok(())),
                core::task::Poll::Ready(Err(err)) => core::task::Poll::Ready(Err(err)),
            }
        })
    }

    pub(crate) fn send_with_meta_in_place<'a, M>(
        &'a mut self,
        meta: SendMeta,
        payload: Option<&'a M::Payload>,
    ) -> impl core::future::Future<Output = SendResult<()>> + 'a
    where
        M: MessageSpec + SendableLabel + 'a,
        M::Payload: WireEncode + 'a,
        M::ControlKind: ControlPayloadKind,
        'r: 'a,
    {
        let desc = send_desc::<M>();
        let mut state = super::core::SendState::Init {
            meta,
            preview_cursor_index: None,
            payload: payload.map(super::lane_port::RawSendPayload::from_typed::<M::Payload>),
        };

        core::future::poll_fn(move |cx| match self.poll_send_state(desc, &mut state, cx) {
            core::task::Poll::Pending => core::task::Poll::Pending,
            core::task::Poll::Ready(Ok(_)) => core::task::Poll::Ready(Ok(())),
            core::task::Poll::Ready(Err(err)) => core::task::Poll::Ready(Err(err)),
        })
    }
}
