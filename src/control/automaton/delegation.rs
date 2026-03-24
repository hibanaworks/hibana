//! Delegation automatons — lease-first delegation minting.
//!
//! Provides [`ControlAutomaton`] implementations that drive capability minting
//! through `SessionCluster::drive`, ensuring canonical delegation tokens are
//! produced via `CapsFacet` rather than direct rendezvous access.

use core::marker::PhantomData;

use crate::{
    control::{
        cap::mint::{
            AllowsCanonical, CAP_FIXED_HEADER_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CapShot,
            GenericCapToken, MintConfigMarker, ResourceKind,
        },
        cluster::error::DelegationError,
        lease::{
            bundle::LeaseBundleFacet,
            core::{ControlAutomaton, ControlStep, DelegationSpec, RendezvousLease},
            graph::{LeaseGraph, LeaseSpec},
            planner::{LeaseFacetNeeds, LeaseSpecFacetNeeds, facets_caps_delegation},
        },
        types::{Lane, RendezvousId, SessionId},
    },
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

/// Maximum node capacity for [`DelegationLeaseSpec`].
pub(crate) const DELEGATION_LEASE_MAX_NODES: usize = 8;
/// Maximum child capacity for [`DelegationLeaseSpec`].
pub(crate) const DELEGATION_LEASE_MAX_CHILDREN: usize = 6;

const DELEGATION_FACET_NEEDS: LeaseFacetNeeds = facets_caps_delegation();

/// LeaseGraph specification for delegation orchestration.
pub(crate) struct DelegationLeaseSpec<T, U, C, E>(PhantomData<(T, U, C, E)>);

impl<T, U, C, E> LeaseSpec for DelegationLeaseSpec<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type NodeId = RendezvousId;
    type Facet = LeaseBundleFacet<T, U, C, E>;
    const MAX_NODES: usize = DELEGATION_LEASE_MAX_NODES;
    const MAX_CHILDREN: usize = DELEGATION_LEASE_MAX_CHILDREN;
}

impl<T, U, C, E> LeaseSpecFacetNeeds for DelegationLeaseSpec<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline(always)]
    fn facet_needs() -> LeaseFacetNeeds {
        DELEGATION_FACET_NEEDS
    }
}

/// Seed describing a canonical delegation mint operation.
pub(crate) struct DelegateMintSeed<K, Mint>
where
    K: ResourceKind,
    Mint: MintConfigMarker,
{
    pub(crate) sid: SessionId,
    pub(crate) lane: Lane,
    pub(crate) dest_role: u8,
    pub(crate) shot: CapShot,
    pub(crate) handle: K::Handle,
    pub(crate) mint: Mint,
}

/// Automaton that mints canonical delegation capabilities via `CapsFacet`.
pub(crate) struct DelegateMintAutomaton<K, Mint>(PhantomData<(K, Mint)>);

impl<T, U, C, E, K, Mint> ControlAutomaton<T, U, C, E> for DelegateMintAutomaton<K, Mint>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::mint::EpochTable,
    K: ResourceKind,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
{
    type Spec = DelegationSpec;
    type Seed = DelegateMintSeed<K, Mint>;
    type Output = GenericCapToken<K>;
    type Error = DelegationError;
    type GraphSpec = DelegationLeaseSpec<T, U, C, E>;

    fn run<'lease, 'lease_cfg>(
        _lease: &mut RendezvousLease<'lease, 'lease_cfg, T, U, C, E, Self::Spec>,
        _seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'lease_cfg: 'lease,
    {
        ControlStep::Abort(DelegationError::InvalidToken)
    }

    fn run_with_graph<'lease, 'lease_cfg, 'graph>(
        graph: &'graph mut LeaseGraph<'graph, DelegationLeaseSpec<T, U, C, E>>,
        root_lease: &mut RendezvousLease<'lease, 'lease_cfg, T, U, C, E, Self::Spec>,
        seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'lease_cfg: 'lease,
    {
        let DelegateMintSeed {
            sid,
            lane,
            dest_role,
            shot,
            handle,
            mint,
        } = seed;

        let strategy = mint.as_config().strategy();
        let nonce_seed = root_lease.with_rendezvous(|rv| {
            let caps = rv.caps_facet();
            caps.next_nonce_seed(rv)
        });
        let nonce = strategy.derive_nonce(nonce_seed);

        let handle_bytes = K::encode_handle(&handle);
        let caps_mask = K::caps_mask(&handle);

        root_lease.with_rendezvous(|rv| {
            let caps = rv.caps_facet();
            caps.mint_cap::<K>(rv, sid, lane, shot, dest_role, nonce, handle);
        });

        if let Some(result) = {
            let mut handle = graph.root_handle_mut();
            handle
                .context()
                .caps_mut()
                .map(|caps| caps.track_mint(nonce))
        } && result.is_err()
        {
            root_lease.with_rendezvous(|rv| rv.release_cap_by_nonce(&nonce));
            return ControlStep::Abort(DelegationError::Exhausted);
        }

        let mut header = [0u8; CAP_HEADER_LEN];
        header[0..4].copy_from_slice(&sid.raw().to_be_bytes());
        header[4] = lane.as_wire();
        header[5] = dest_role;
        header[6] = K::TAG;
        header[7] = shot.as_u8();
        header[8..10].copy_from_slice(&caps_mask.bits().to_be_bytes());
        header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
            .copy_from_slice(&handle_bytes);

        let tag = strategy.derive_tag(&nonce, &header);

        ControlStep::Complete(GenericCapToken::from_parts(nonce, header, tag))
    }
}
