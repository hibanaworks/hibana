//! Delegation primitives for the cursor endpoint surface.
//!
//! This module provides lightweight data types for delegation events, routing,
//! and splice coordination used by tap events and policy context.

/// Locator used by routers to resolve remote rendezvous handles.
#[derive(Clone, Copy, Debug, Default)]
pub struct Locator {
    /// Abstract identifier for the destination rendezvous (opaque for now).
    pub id: u32,
}

/// Enumeration of delegation events emitted to the tap ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelegEvent {
    /// Delegation begin with shot discipline and service identifier.
    DelegBegin {
        /// Shot discipline flag (0 = one-shot, 1 = many-shot).
        shot: u8,
        /// Service identifier provided by the router.
        svc: u32,
        /// Number of in-flight delegations observed when issuing the event.
        in_flight: u32,
    },
    /// Router policy selected a particular shard.
    RoutePick {
        /// Policy identifier.
        policy: u32,
        /// Chosen shard identifier.
        shard: u32,
    },
    /// Delegation splice completed between two lanes.
    DelegSplice {
        /// Source lane identifier.
        from: u8,
        /// Destination lane identifier.
        to: u8,
        /// Generation number associated with the splice.
        generation: u16,
    },
    /// Delegation aborted with a reason code.
    DelegAbort {
        /// Reason for the abort (opaque placeholder until the new routing layer lands).
        reason: DelegAbortReason,
    },
    /// Service-level objective breach detected by the router.
    SLOBreach {
        /// Observed latency in microseconds.
        lat: u32,
        /// Queue tail depth at the time of the breach.
        qtail: u32,
        /// Retry counter or auxiliary data.
        retry: u32,
    },
}

/// Abort reasons for delegation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelegAbortReason {
    /// Unspecified reason.
    Unspecified,
}

use crate::{
    control::{
        automaton::delegation::DelegatedPortWitness,
        cluster::{AttachError, SessionCluster},
        types::{LaneId as CpLaneId, RendezvousId as CpRendezvousId, SessionId as CpSessionId},
    },
    endpoint::CursorEndpoint,
    transport::Transport,
};

/// Pending delegated port claim obtained from `SessionCluster::delegate_claim`.
pub struct DelegatedPortClaim<'cluster, 'cfg, T, U, C, const MAX_RV: usize>
where
    T: Transport,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
    'cluster: 'cfg,
{
    cluster: &'cluster SessionCluster<'cfg, T, U, C, MAX_RV>,
    rv_id: CpRendezvousId,
    sid: CpSessionId,
    lane: CpLaneId,
    witness: Option<DelegatedPortWitness>,
}

impl<'cluster, 'cfg, T, U, C, const MAX_RV: usize>
    DelegatedPortClaim<'cluster, 'cfg, T, U, C, MAX_RV>
where
    T: Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    'cluster: 'cfg,
{
    pub(crate) fn new(
        cluster: &'cluster SessionCluster<'cfg, T, U, C, MAX_RV>,
        rv_id: CpRendezvousId,
        sid: CpSessionId,
        lane: CpLaneId,
        witness: DelegatedPortWitness,
    ) -> Self {
        Self {
            cluster,
            rv_id,
            sid,
            lane,
            witness: Some(witness),
        }
    }

    #[inline]
    pub fn sid(&self) -> CpSessionId {
        self.sid
    }

    #[inline]
    pub fn lane(&self) -> CpLaneId {
        self.lane
    }

    /// Consume the claim and attach a delegated cursor endpoint.
    pub fn attach_cursor<'lease, const ROLE: u8, LocalSteps, Mint>(
        mut self,
        program: &'static crate::g::RoleProgram<'static, ROLE, LocalSteps, Mint>,
    ) -> Result<
        CursorEndpoint<'cfg, ROLE, T, U, C, crate::control::cap::EpochInit, MAX_RV, Mint>,
        AttachError,
    >
    where
        'cfg: 'lease,
        'lease: 'cfg,
        Mint: crate::control::cap::MintConfigMarker,
    {
        let witness = self
            .witness
            .take()
            .expect("delegated port witness already consumed");
        match self
            .cluster
            .attach_delegated_cursor::<ROLE, LocalSteps, Mint>(
                self.rv_id, self.sid, self.lane, program, witness,
            ) {
            Ok(endpoint) => Ok(endpoint),
            Err(err) => {
                self.cluster
                    .revoke_delegated_witness(self.rv_id, self.sid, self.lane);
                Err(err)
            }
        }
    }
}

impl<'cluster, 'cfg, T, U, C, const MAX_RV: usize> Drop
    for DelegatedPortClaim<'cluster, 'cfg, T, U, C, MAX_RV>
where
    T: Transport,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
    'cluster: 'cfg,
{
    fn drop(&mut self) {
        if self.witness.is_some() {
            self.cluster
                .revoke_delegated_witness(self.rv_id, self.sid, self.lane);
            self.witness = None;
        }
    }
}
