//! Rendezvous state tables.
//!
//! These tables manage route decisions and waiters.
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

use super::waiter::WaiterSlot;
use crate::{
    global::const_dsl::{ScopeId, ScopeKind},
    session::types::Lane,
    transport::FrameLabelMask,
};

const MAX_TRACKED_ROLES: usize = u16::BITS as usize;
#[inline]
const fn checked_add_usize(lhs: usize, rhs: usize) -> usize {
    if lhs > usize::MAX - rhs {
        crate::invariant();
    }
    lhs + rhs
}

#[inline]
const fn checked_mul_usize(lhs: usize, rhs: usize) -> usize {
    if lhs != 0 && rhs > usize::MAX / lhs {
        crate::invariant();
    }
    lhs * rhs
}

#[inline]
const fn checked_sub_usize(lhs: usize, rhs: usize) -> usize {
    if lhs < rhs {
        crate::invariant();
    }
    lhs - rhs
}

mod route_table;

pub(crate) use route_table::*;
