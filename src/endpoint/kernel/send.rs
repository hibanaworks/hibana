//! Send-path helpers for `flow().send()`.

use super::core::CursorEndpoint;
use crate::{
    binding::{BindingHandle, BindingSlot},
    control::cap::mint::EpochTable,
    control::cap::mint::{EpochTbl, MintConfigMarker},
    endpoint::{SendResult, flow::CapFlow},
    global::{MessageSpec, SendableLabel},
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};
#[cfg(test)]
use crate::{
    endpoint::control::ControlOutcome, endpoint::flow::FlowSendArg, global::ControlPayloadKind,
    transport::wire::WireEncode,
};

impl<'r, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, EpochTbl, MAX_RV, Mint, BindingHandle<'r>>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    Mint: MintConfigMarker,
{
    pub(crate) fn flow_for_kit<'cfg, M>(
        &mut self,
    ) -> SendResult<
        CapFlow<'_, 'r, ROLE, M, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint>,
    >
    where
        M: MessageSpec + SendableLabel,
        T: 'cfg,
        U: 'cfg,
        C: 'cfg,
        'cfg: 'r,
    {
        let preview = self.preview_flow_meta::<M>()?;
        Ok(CapFlow::new(core::ptr::from_mut(self), preview))
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    #[cfg(test)]
    pub(crate) async fn send_direct<'a, M, A>(
        &mut self,
        arg: A,
    ) -> SendResult<
        ControlOutcome<'r, <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>,
    >
    where
        M: MessageSpec + SendableLabel,
        M::Payload: WireEncode + 'a,
        M::ControlKind:
            super::core::CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
        A: FlowSendArg<'a, M, Mint>,
        <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind: 'r,
    {
        let preview = self.preview_flow_meta::<M>()?;
        self.send_with_preview_in_place::<M>(preview, arg.into_payload())
            .await
    }
}
