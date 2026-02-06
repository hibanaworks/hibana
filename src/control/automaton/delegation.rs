//! Delegation automatons — lease-first delegation minting.
//!
//! Provides [`ControlAutomaton`] implementations that drive capability minting
//! through `SessionCluster::drive`, ensuring canonical delegation tokens are
//! produced via `CapsFacet` rather than direct rendezvous access.

use core::{marker::PhantomData, ptr::NonNull};

use crate::{
    control::{
        cap::{
            AllowsCanonical, CAP_FIXED_HEADER_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CapShot,
            CapToken, EndpointResource, GenericCapToken, MintConfigMarker, ResourceKind,
            VerifiedCap,
        },
        cluster::error::DelegationError,
        lease::{
            ControlAutomaton, ControlStep, DelegationSpec, RendezvousLease,
            bundle::LeaseBundleFacet,
            graph::{LeaseGraph, LeaseSpec},
            map::ArrayMap,
            planner::{FacetCapsDelegation, LeaseFacetNeeds, LeaseSpecFacetNeeds},
        },
        types::{LaneId as CpLaneId, RendezvousId, SessionId as CpSessionId},
    },
    rendezvous::{CapError, Lane, SessionId},
    runtime::{config::Clock, consts::LabelUniverse},
    transport::Transport,
};

#[derive(Debug, Default)]
pub struct DelegationGraphContext {
    slots: Option<NonNull<DelegatedPortTable>>,
    key: Option<DelegatedPortKey>,
    staged_claim: Option<ClaimStage>,
}

/// Maximum number of delegated ports tracked concurrently.
pub(crate) const MAX_DELEGATED_PORTS: usize = 32;

/// Key identifying a delegated port claim within the control core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DelegatedPortKey {
    pub rendezvous: RendezvousId,
    pub sid: CpSessionId,
    pub lane: CpLaneId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClaimStage {
    Inserted,
    Updated,
}

impl DelegatedPortKey {
    pub const fn new(rendezvous: RendezvousId, sid: CpSessionId, lane: CpLaneId) -> Self {
        Self {
            rendezvous,
            sid,
            lane,
        }
    }
}

/// Storage slot for a delegated port claim.
pub(crate) struct DelegatedPortSlot {
    cap: Option<VerifiedCap<EndpointResource>>,
    issued: bool,
}

impl DelegatedPortSlot {
    #[inline]
    pub(crate) fn pending(cap: VerifiedCap<EndpointResource>) -> Self {
        Self {
            cap: Some(cap),
            issued: false,
        }
    }

    #[inline]
    pub(crate) fn set(
        &mut self,
        cap: VerifiedCap<EndpointResource>,
    ) -> Result<(), DelegationError> {
        if self.cap.is_some() {
            return Err(DelegationError::AlreadyClaimed);
        }
        self.cap = Some(cap);
        self.issued = false;
        Ok(())
    }

    #[inline]
    pub(crate) fn issue(&mut self) -> Option<DelegatedPortWitness> {
        if self.cap.is_some() && !self.issued {
            self.issued = true;
            Some(DelegatedPortWitness::new())
        } else {
            None
        }
    }

    #[inline]
    pub(crate) fn claim(&mut self) -> Result<DelegatedPortClaimGuard, DelegationError> {
        if !self.issued {
            return Err(DelegationError::InvalidToken);
        }
        self.issued = false;
        let cap = self.cap.take().ok_or(DelegationError::InvalidToken)?;
        Ok(DelegatedPortClaimGuard {
            slot: NonNull::from(self),
            cap: Some(cap),
            committed: false,
        })
    }

    #[inline]
    pub(crate) fn has_pending(&self) -> bool {
        self.cap.is_some() && !self.issued
    }

    #[inline]
    pub(crate) fn revoke(&mut self) {
        if self.cap.is_some() {
            self.issued = false;
        }
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        self.cap = None;
        self.issued = false;
    }

    #[inline]
    fn restore(&mut self, cap: VerifiedCap<EndpointResource>) {
        debug_assert!(
            self.cap.is_none(),
            "delegated claim restore clobbers pending cap"
        );
        self.cap = Some(cap);
        self.issued = false;
    }
}

pub(crate) type DelegatedPortTable =
    ArrayMap<DelegatedPortKey, DelegatedPortSlot, MAX_DELEGATED_PORTS>;

pub(crate) struct DelegatedPortClaimGuard {
    slot: NonNull<DelegatedPortSlot>,
    cap: Option<VerifiedCap<EndpointResource>>,
    committed: bool,
}

impl DelegatedPortClaimGuard {
    #[inline]
    pub fn verified(&self) -> &VerifiedCap<EndpointResource> {
        self.cap
            .as_ref()
            .expect("delegated port claim guard missing cap")
    }

    #[inline]
    pub fn into_verified(mut self) -> VerifiedCap<EndpointResource> {
        self.committed = true;
        self.cap
            .take()
            .expect("delegated port claim guard missing cap")
    }

    #[inline]
    pub fn commit(mut self) {
        self.committed = true;
        self.cap.take();
    }
}

impl Drop for DelegatedPortClaimGuard {
    fn drop(&mut self) {
        if !self.committed
            && let Some(cap) = self.cap.take()
        {
            unsafe { self.slot.as_mut().restore(cap) };
        }
    }
}

impl DelegationGraphContext {
    pub fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn with_table(slots: NonNull<DelegatedPortTable>) -> Self {
        Self {
            slots: Some(slots),
            key: None,
            staged_claim: None,
        }
    }

    pub(crate) fn for_claim(slots: NonNull<DelegatedPortTable>, key: DelegatedPortKey) -> Self {
        Self {
            slots: Some(slots),
            key: Some(key),
            staged_claim: None,
        }
    }

    pub fn store_claim(
        &mut self,
        cap: VerifiedCap<EndpointResource>,
    ) -> Result<(), DelegationError> {
        let mut slots_ptr = self.slots.ok_or(DelegationError::InvalidToken)?;
        let key = self.key.ok_or(DelegationError::InvalidToken)?;
        let slots = unsafe { slots_ptr.as_mut() };

        if let Some(entry) = slots.get_mut(&key) {
            entry.set(cap)?;
            self.staged_claim = Some(ClaimStage::Updated);
            return Ok(());
        }

        if slots.is_full() {
            drop(cap);
            return Err(DelegationError::Exhausted);
        }

        match slots.insert(key, DelegatedPortSlot::pending(cap)) {
            Ok(()) => {
                self.staged_claim = Some(ClaimStage::Inserted);
                Ok(())
            }
            Err(slot) => {
                drop(slot);
                Err(DelegationError::Exhausted)
            }
        }
    }

    pub(crate) fn reset(&mut self) {
        self.slots = None;
        self.key = None;
        self.staged_claim = None;
    }

    pub(crate) fn rollback(&mut self) {
        if let (Some(stage), Some(mut slots_ptr), Some(key)) =
            (self.staged_claim, self.slots, self.key)
        {
            let slots = unsafe { slots_ptr.as_mut() };
            match stage {
                ClaimStage::Inserted => {
                    let _ = slots.remove(&key);
                }
                ClaimStage::Updated => {
                    if let Some(entry) = slots.get_mut(&key) {
                        entry.clear();
                    }
                }
            }
        }
        self.reset();
    }
}

/// Maximum node capacity for [`DelegationLeaseSpec`].
pub const DELEGATION_LEASE_MAX_NODES: usize = 8;
/// Maximum child capacity for [`DelegationLeaseSpec`].
pub const DELEGATION_LEASE_MAX_CHILDREN: usize = 6;

const DELEGATION_FACET_NEEDS: LeaseFacetNeeds = FacetCapsDelegation::NEEDS;

/// LeaseGraph specification for delegation orchestration.
pub struct DelegationLeaseSpec<T, U, C, E>(PhantomData<(T, U, C, E)>);

impl<T, U, C, E> LeaseSpec for DelegationLeaseSpec<T, U, C, E>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
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
    E: crate::control::cap::EpochTable,
{
    const FACET_NEEDS: LeaseFacetNeeds = DELEGATION_FACET_NEEDS;
}

/// Seed describing a canonical delegation mint operation.
pub struct DelegateMintSeed<K, Mint>
where
    K: ResourceKind,
    Mint: MintConfigMarker,
{
    pub sid: SessionId,
    pub lane: Lane,
    pub dest_role: u8,
    pub shot: CapShot,
    pub handle: K::Handle,
    pub mint: Mint,
}

/// Automaton that mints canonical delegation capabilities via `CapsFacet`.
pub struct DelegateMintAutomaton<K, Mint>(PhantomData<(K, Mint)>);

impl<T, U, C, E, K, Mint> ControlAutomaton<T, U, C, E> for DelegateMintAutomaton<K, Mint>
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
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

/// Seed describing a delegation claim operation.
pub struct DelegateClaimSeed {
    pub token: CapToken,
}

/// Zero-sized witness produced when a delegated port claim succeeds.
#[derive(Debug)]
pub struct DelegatedPortWitness {
    _private: (),
}

impl DelegatedPortWitness {
    #[inline]
    pub(crate) fn new() -> Self {
        Self { _private: () }
    }
}

/// Automaton that claims a delegation capability via `CapsFacet`.
pub struct DelegateClaimAutomaton;

impl<T, U, C, E> ControlAutomaton<T, U, C, E> for DelegateClaimAutomaton
where
    T: Transport,
    U: LabelUniverse,
    C: Clock,
    E: crate::control::cap::EpochTable,
{
    type Spec = DelegationSpec;
    type Seed = DelegateClaimSeed;
    type Output = DelegatedPortWitness;
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
        let token = seed.token;

        let verified = match root_lease.with_rendezvous(|rv| {
            let caps = rv.caps_facet();
            caps.claim_cap(rv, &token)
        }) {
            Ok(cap) => cap,
            Err(err) => return ControlStep::Abort(map_cap_error(err)),
        };

        let mut handle = graph.root_handle_mut();
        let outcome = {
            let ctx = handle.context();
            match ctx.delegation() {
                Some(delegation) => delegation.store_claim(verified),
                None => Err(DelegationError::InvalidToken),
            }
        };

        if let Err(err) = outcome {
            return ControlStep::Abort(err);
        }

        ControlStep::Complete(DelegatedPortWitness::new())
    }
}

fn map_cap_error(err: CapError) -> DelegationError {
    match err {
        CapError::UnknownToken | CapError::WrongSessionOrLane => DelegationError::InvalidToken,
        CapError::Exhausted => DelegationError::Exhausted,
        CapError::Mismatch => DelegationError::ShotMismatch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::{CapShot, CapsMask, EndpointHandle, EndpointResource, VerifiedCap};
    use crate::control::types::{LaneId, RendezvousId, SessionId as CpSessionId};
    use crate::rendezvous::{Lane, SessionId};

    #[test]
    fn context_with_table_records_pending_claim() {
        let mut table = DelegatedPortTable::new();
        let key = DelegatedPortKey::new(RendezvousId::new(7), CpSessionId::new(3), LaneId::new(1));
        let slots_ptr = NonNull::from(&mut table);
        let mut context = DelegationGraphContext::for_claim(slots_ptr, key);

        let sid = SessionId::new(3);
        let lane = Lane::new(1);
        let handle = EndpointHandle::new(sid, lane, 2);
        let cap = VerifiedCap::<EndpointResource>::new(
            sid,
            lane,
            2,
            CapShot::One,
            CapsMask::allow_all(),
            handle,
            None,
        );

        context.store_claim(cap).expect("store claim succeeds");

        let slots = unsafe { slots_ptr.as_ref() };
        let entry = slots.get(&key).expect("claims table entry populated");
        assert!(
            entry.has_pending(),
            "delegated port slot should track pending claim"
        );
    }
}
