//! Crate-private carrier markers and alias owners for internal endpoint packs.

use core::marker::PhantomData;

use crate::{control::types::RendezvousId, rendezvous::core::EndpointLeaseId};

pub(crate) struct SessionCfg<K>(pub(crate) PhantomData<fn() -> K>);

pub(crate) struct EndpointCfg<K, Mint, B>(pub(crate) PhantomData<fn() -> (K, Mint, B)>);

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct PackedEndpointHandle(u32);

impl PackedEndpointHandle {
    #[inline]
    pub(crate) fn new(rv: RendezvousId, slot: EndpointLeaseId) -> Self {
        Self(((rv.raw() as u32) << 16) | (u16::from(slot) as u32))
    }

    #[inline]
    pub(crate) fn rendezvous(self) -> RendezvousId {
        RendezvousId::new((self.0 >> 16) as u16)
    }

    #[inline]
    pub(crate) fn slot(self) -> EndpointLeaseId {
        EndpointLeaseId::from(self.0 as u16)
    }
}

pub(crate) trait SessionKitFamily {
    type Transport;
    type LabelUniverse;
    type Clock;

    type KernelSessionCluster<'cfg>
    where
        Self: 'cfg;

    type KernelCursorEndpoint<'r, const ROLE: u8, E, Mint, B>
    where
        Self: 'r,
        E: crate::control::cap::mint::EpochTable + 'r,
        Mint: crate::control::cap::mint::MintConfigMarker,
        B: crate::binding::BindingSlot + 'r;

    type KernelRouteBranch<'r, const ROLE: u8, E, Mint, B>
    where
        Self: 'r,
        E: crate::control::cap::mint::EpochTable + 'r,
        Mint: crate::control::cap::mint::MintConfigMarker,
        B: crate::binding::BindingSlot + 'r;
}

pub(crate) type KernelCursorEndpoint<'r, const ROLE: u8, K, E, Mint, B> =
    <K as SessionKitFamily>::KernelCursorEndpoint<'r, ROLE, E, Mint, B>;
pub(crate) type KernelRouteBranch<'r, const ROLE: u8, K, E, Mint, B> =
    <K as SessionKitFamily>::KernelRouteBranch<'r, ROLE, E, Mint, B>;

impl<'cfg, T, U, C, const MAX_RV: usize> SessionKitFamily
    for crate::substrate::SessionKit<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    type Transport = T;
    type LabelUniverse = U;
    type Clock = C;

    type KernelSessionCluster<'lease>
        = crate::control::cluster::core::SessionCluster<'lease, T, U, C, MAX_RV>
    where
        Self: 'lease;

    type KernelCursorEndpoint<'r, const ROLE: u8, E, Mint, B>
        = crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
    where
        Self: 'r,
        E: crate::control::cap::mint::EpochTable + 'r,
        Mint: crate::control::cap::mint::MintConfigMarker,
        B: crate::binding::BindingSlot + 'r;

    type KernelRouteBranch<'r, const ROLE: u8, E, Mint, B>
        = crate::endpoint::kernel::RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
    where
        Self: 'r,
        E: crate::control::cap::mint::EpochTable + 'r,
        Mint: crate::control::cap::mint::MintConfigMarker,
        B: crate::binding::BindingSlot + 'r;
}
