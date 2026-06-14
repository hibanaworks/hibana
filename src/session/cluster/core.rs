//! SessionCluster - Local session coordination.
//!
//! This module implements SessionCluster, which coordinates multiple Rendezvous
//! instances for local session ownership.
//!
//! # Unsafe Owner Contract
//!
//! This module owns the in-place session cluster image. Unsafe blocks here may
//! initialize resident storage/resolver buckets and borrow their
//! `UnsafeCell` state, but must keep one mutable owner per closure, preserve
//! initialized-bucket ranges, and keep endpoint/lease generations coherent.

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;

use crate::session::lease::core::{LeaseError, RegisterRendezvousError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PublicEndpointStorageLayout {
    total_bytes: usize,
    total_align: usize,
    arena_offset: usize,
}

use core::fmt;

use super::error::{AttachError, ClusterError, ResourceScope};
use crate::eff::EffIndex;
use crate::global::compiled::images::{CompiledProgramRef, RoleImageSlice};
use crate::rendezvous::core::{EndpointLeaseId, LaneLease, Rendezvous};
use crate::rendezvous::error::RendezvousError;
use crate::session::types::{Lane, RendezvousId, SessionId};

struct EndpointInitArgs<
    'r,
    const ROLE: u8,
    T: crate::transport::Transport + 'r,
    C: crate::runtime_core::config::Clock,
    const MAX_RV: usize,
> {
    dst: *mut crate::endpoint::kernel::CursorEndpoint<'r, ROLE, T, C, MAX_RV>,
    arena_storage: *mut u8,
    rv_id: RendezvousId,
    sid: SessionId,
    role_image: RoleImageSlice<ROLE>,
    public_slot: EndpointLeaseId,
    public_generation: u32,
    public_ops: crate::endpoint::carrier::EndpointOps<'r>,
    public_slot_ownership: crate::endpoint::kernel::PublicSlotOwnership,
}
mod cluster_storage;
mod dynamic_resolvers;
mod endpoint_attach;
mod session_cluster_ops;
mod session_effect_steps;

pub(crate) use cluster_storage::*;
pub(crate) use dynamic_resolvers::*;
pub use dynamic_resolvers::{DecisionArm, DecisionResolution, ResolverError, ResolverRef};
pub(crate) use session_cluster_ops::*;

impl<'cfg, T, C, const MAX_RV: usize> Drop for SessionCluster<'cfg, T, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    C: crate::runtime_core::config::Clock + 'cfg,
{
    fn drop(&mut self) {
        // SAFETY: `core` is owned by `self` and we're in `drop`, so no aliases exist.
        let core = unsafe { &*self.storage_ref_ptr() };
        if core.active_leases.get() != 0 {
            crate::invariant();
        }
    }
}
