//! Crate-private carrier markers and alias owners for internal endpoint packs.

use core::marker::PhantomData;

pub(crate) struct SessionCfg<K>(pub(crate) PhantomData<fn() -> K>);

pub(crate) struct EndpointCfg<K, Mint, B>(pub(crate) PhantomData<fn() -> (K, Mint, B)>);

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

    unsafe fn stash_route_branch_preview<'r, const ROLE: u8, Mint>(
        kit: &'r Self,
        slot: u8,
        generation: u32,
        branch: Self::KernelRouteBranch<
            'r,
            ROLE,
            crate::control::cap::mint::EpochTbl,
            Mint,
            crate::binding::BindingHandle<'r>,
        >,
    ) where
        Self: 'r,
        Mint: crate::control::cap::mint::MintConfigMarker;
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

    #[inline]
    unsafe fn stash_route_branch_preview<'r, const ROLE: u8, Mint>(
        kit: &'r Self,
        slot: u8,
        generation: u32,
        branch: Self::KernelRouteBranch<
            'r,
            ROLE,
            crate::control::cap::mint::EpochTbl,
            Mint,
            crate::binding::BindingHandle<'r>,
        >,
    ) where
        Self: 'r,
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        let Some(endpoint) = (unsafe {
            crate::substrate::public_endpoint_access::<ROLE, T, U, C, MAX_RV, Mint>(
                kit, slot, generation,
            )
        }) else {
            return;
        };
        unsafe {
            (&mut *endpoint).stash_pending_branch_preview(branch);
        }
    }
}
