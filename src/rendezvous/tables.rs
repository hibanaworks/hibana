//! Internal state tables for ra module.
//!
//! These tables manage generation counters, state snapshots, and routing policies.
//! All tables are !Send/!Sync and single-threaded under no_std.
//!
//! # Unsafe Owner Contract
//!
//! This module is the owner for rendezvous table backing storage. Unsafe blocks
//! here may initialize or rebind caller-provided slices, but must preserve the
//! table's lane range, initialized-entry, and single-writer invariants before
//! exposing safe table methods.

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    task::{Context, Poll},
};

use super::error::{GenError, GenerationRecord};
use super::waiter::WaiterSlot;
use crate::{
    control::{
        lease::map::ArrayMap,
        types::{Generation, Lane},
    },
    eff::EffIndex,
    global::const_dsl::{PolicyMode, ScopeId, ScopeKind},
    transport::FrameLabelMask,
};

const MAX_TRACKED_ROLES: usize = u16::BITS as usize;
#[cfg(test)]
const ROUTE_SLOTS: usize = crate::eff::meta::MAX_EFF_NODES;
const CONTROL_PLAN_SLOTS: usize = 128;

#[inline]
const fn lane_storage_align() -> usize {
    let u16_align = core::mem::align_of::<u16>();
    let u8_align = core::mem::align_of::<u8>();
    if u16_align > u8_align {
        u16_align
    } else {
        u8_align
    }
}

#[inline]
const fn align_up(value: usize, align: usize) -> usize {
    let mask = align.saturating_sub(1);
    (value + mask) & !mask
}

mod generation;
mod loop_table;
mod policy;
mod route_table;
#[cfg(test)]
mod route_tests;
mod snapshot;

pub(crate) use generation::*;
pub(crate) use loop_table::*;
pub(crate) use policy::*;
pub(crate) use route_table::*;
pub(crate) use snapshot::*;
