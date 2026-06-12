//! SessionCluster - Distributed control-plane coordination.
//!
//! This module implements SessionCluster, which coordinates multiple Rendezvous
//! instances for local distributed session management.
//!
//! # Unsafe Owner Contract
//!
//! This module owns the in-place session cluster image. Unsafe blocks here may
//! initialize resident control/resolver buckets and borrow their
//! `UnsafeCell` state, but must keep one mutable owner per closure, preserve
//! initialized-bucket ranges, and keep endpoint/lease generations coherent.

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;

use crate::control::automaton::distributed::{
    DistributedTopology, DistributedTopologyInv, TopologyAck, TopologyIntent,
};
use crate::control::cap::atomic_codecs::{
    SessionLaneHandle, TopologyHandle, decode_session_lane_handle,
};
use crate::control::cap::mint::CapHeader;
use crate::control::cap::mint::{CAP_TOKEN_LEN, ControlOp, ControlToken, MintConfigMarker};
use crate::control::cluster::effects::EffectEnvelopeRef;
use crate::control::cluster::error::{StateRestoreError, TxAbortError, TxCommitError};
use crate::control::lease::core::{FullSpec, LeaseError, RegisterRendezvousError};
use crate::global::ControlDesc;
use crate::global::const_dsl::ControlScopeKind;
use crate::rendezvous::TopologySessionState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PublicEndpointStorageLayout {
    total_bytes: usize,
    total_align: usize,
    header_bytes: usize,
    port_slots_bytes: usize,
    guard_slots_bytes: usize,
    header_padding_bytes: usize,
    arena_offset: usize,
    arena_bytes: usize,
    arena_align: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TopologyDescriptor {
    operands: TopologyOperands,
}

impl TopologyDescriptor {
    #[inline]
    pub(crate) fn decode_for(
        operation: ControlOp,
        bytes: [u8; crate::control::cap::mint::CAP_HANDLE_LEN],
    ) -> Result<Self, CpError> {
        let handle = TopologyHandle::decode(bytes).map_err(|_| CpError::Authorisation {
            operation: operation as u8,
        })?;
        let src_lane = Lane::try_new(u32::from(handle.src_lane)).ok_or(CpError::Authorisation {
            operation: operation as u8,
        })?;
        let dst_lane = Lane::try_new(u32::from(handle.dst_lane)).ok_or(CpError::Authorisation {
            operation: operation as u8,
        })?;
        if handle.src_rv == 0 || handle.dst_rv == 0 || handle.src_rv == handle.dst_rv {
            return Err(CpError::Authorisation {
                operation: operation as u8,
            });
        }
        let operands = TopologyOperands {
            src_rv: RendezvousId::new(handle.src_rv),
            dst_rv: RendezvousId::new(handle.dst_rv),
            src_lane,
            dst_lane,
            old_gen: Generation::new(handle.old_gen),
            new_gen: Generation::new(handle.new_gen),
            seq_tx: handle.seq_tx,
            seq_rx: handle.seq_rx,
        };
        Ok(Self { operands })
    }

    #[inline]
    pub(crate) const fn operands(self) -> TopologyOperands {
        self.operands
    }
}

#[inline]
fn validate_topology_rendezvous_pair(
    src_rv: RendezvousId,
    dst_rv: RendezvousId,
    operation: ControlOp,
) -> Result<(), CpError> {
    if src_rv.raw() == 0 || dst_rv.raw() == 0 || src_rv == dst_rv {
        return Err(CpError::Authorisation {
            operation: operation as u8,
        });
    }
    Ok(())
}

use core::{fmt, panic::Location};

use super::error::{AttachError, CpError, ResourceScope, TopologyError};
use crate::control::automaton::txn::{InAcked, InBegin};
use crate::control::types::{Generation, Lane, RendezvousId, SessionId};
use crate::eff::EffIndex;
use crate::global::{
    compiled::images::{CompiledProgramRef, RoleImageSlice},
    const_dsl::ResolverMode,
};
use crate::rendezvous::core::{EndpointLeaseId, LaneLease, Rendezvous};
use crate::rendezvous::error::RendezvousError;

type ClusterCursorEndpoint<'r, const ROLE: u8, T, U, C, const MAX_RV: usize, Mint> =
    crate::endpoint::kernel::CursorEndpoint<
        'r,
        ROLE,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
    >;

struct EndpointInitArgs<
    'r,
    const ROLE: u8,
    T: crate::transport::Transport + 'r,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    const MAX_RV: usize,
    Mint: crate::control::cap::mint::MintConfigMarker,
> {
    dst: *mut ClusterCursorEndpoint<'r, ROLE, T, U, C, MAX_RV, Mint>,
    arena_storage: *mut u8,
    rv_id: RendezvousId,
    sid: SessionId,
    role_image: RoleImageSlice<ROLE>,
    public_slot: EndpointLeaseId,
    public_generation: u32,
    public_ops: crate::endpoint::carrier::EndpointOps<'r>,
    public_slot_owned: bool,
    mint: Mint,
}
mod cluster_storage;
mod command_types;
mod descriptor_controls;
mod dynamic_resolvers;
mod endpoint_attach;
mod session_cluster_ops;
mod session_effect_init;
mod session_effect_steps;
mod topology_state;

pub(crate) use cluster_storage::*;
pub(crate) use command_types::*;
pub(crate) use descriptor_controls::{DescriptorPublicationAuthority, DescriptorTerminal};
pub(crate) use dynamic_resolvers::*;
pub use dynamic_resolvers::{DecisionArm, DecisionResolution, ResolverError, ResolverRef};
pub(crate) use session_cluster_ops::*;
pub(crate) use topology_state::*;

impl<'cfg, T, U, C, const MAX_RV: usize> Drop for SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    fn drop(&mut self) {
        // SAFETY: `core` is owned by `self` and we're in `drop`, so no aliases exist.
        let core = unsafe { &*self.control_ref_ptr() };
        if core.active_leases.get() != 0 {
            crate::invariant();
        }
    }
}

#[cfg(all(test, hibana_repo_tests))]
#[path = "core/tests.rs"]
mod tests;
