//! Rendezvous state tables.
//!
//! These tables manage session-owned route decisions.
//! All tables are !Send/!Sync and single-threaded under no_std.
//!
//! # Unsafe Owner Contract
//!
//! This module is the owner for rendezvous table backing storage. Unsafe blocks
//! here may initialize or rebind caller-provided slices, but must preserve the
//! table's lane range, initialized-entry, and single-writer invariants before
//! exposing safe table methods.

use core::{
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
    task::Poll,
};

use crate::{
    global::const_dsl::{ScopeId, ScopeKind},
    session::types::{Lane, SessionId},
};

const MAX_TRACKED_ROLES: usize = crate::g::ROLE_DOMAIN_SIZE as usize;
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
