//! Send-path helpers for `flow().send()`.

#[cfg(test)]
use core::marker::PhantomData;

use super::core::CursorEndpoint;
use crate::{
    binding::BindingSlot,
    control::cap::mint::{EpochTbl, MintConfigMarker},
    endpoint::{SendResult, flow::CapFlow},
    global::{MessageSpec, SendableLabel},
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};
#[cfg(test)]
use crate::{
    control::cap::mint::EpochTable,
    endpoint::control::ControlOutcome,
    global::{ControlPayloadKind, typestate::SendMeta},
    transport::wire::WireEncode,
};

impl<'r, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, EpochTbl, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    pub(crate) fn flow_for_kit<'cfg, M>(
        self,
    ) -> SendResult<
        CapFlow<'r, ROLE, M, crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>, Mint, B>,
    >
    where
        M: MessageSpec + SendableLabel,
        T: 'cfg,
        U: 'cfg,
        C: 'cfg,
        'cfg: 'r,
    {
        let (endpoint, meta) = self.prepare_flow::<M>()?;
        Ok(CapFlow::new(endpoint, meta))
    }
}

#[cfg(test)]
pub(crate) struct TestCapFlow<
    'r,
    const ROLE: u8,
    M,
    T,
    U,
    C,
    E,
    const MAX_RV: usize,
    Mint,
    B: BindingSlot,
> where
    M: MessageSpec + SendableLabel,
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    endpoint: CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
    meta: SendMeta,
    _msg: PhantomData<M>,
}

#[cfg(test)]
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
    pub(crate) fn flow<M>(self) -> SendResult<TestCapFlow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>>
    where
        M: MessageSpec + SendableLabel,
    {
        let (endpoint, meta) = self.prepare_flow::<M>()?;
        Ok(TestCapFlow {
            endpoint,
            meta,
            _msg: PhantomData,
        })
    }
}

#[cfg(test)]
impl<'r, const ROLE: u8, M, T, U, C, E, const MAX_RV: usize, Mint, B>
    TestCapFlow<'r, ROLE, M, T, U, C, E, MAX_RV, Mint, B>
where
    M: MessageSpec + SendableLabel,
    M::Payload: WireEncode,
    M::ControlKind: super::core::CanonicalTokenProvider<'r, ROLE, T, U, C, E, Mint, MAX_RV, M, B>,
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: BindingSlot,
{
    pub(crate) async fn send<'a, A>(
        self,
        arg: A,
    ) -> SendResult<(
        CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>,
        ControlOutcome<'r, <<M as MessageSpec>::ControlKind as ControlPayloadKind>::ResourceKind>,
    )>
    where
        A: crate::endpoint::flow::FlowSendArg<'a, M, Mint>,
        M::Payload: 'a,
    {
        self.endpoint
            .send_with_meta::<M>(&self.meta, arg.into_payload())
            .await
    }
}
