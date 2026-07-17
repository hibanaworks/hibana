//! Frontier-selection helpers for `offer()`.
//!
//! # Unsafe Owner Contract
//!
//! This module owns frontier scratch slices borrowed from the endpoint runtime
//! image. Unsafe blocks here may form slices over that storage only after the
//! endpoint has initialized the matching capacity fields for the current
//! generation.

use core::{mem, slice};

use crate::global::const_dsl::ScopeId;
use crate::global::typestate::{MAX_STATES, StateIndex, state_index_to_usize};

mod active_offer_entry;
mod entry_sets;
mod kind;
mod observation;
mod offer_entries;
mod scratch;
mod select;
mod snapshot;

pub(crate) use active_offer_entry::*;
pub(crate) use entry_sets::*;
pub(crate) use kind::*;
pub(crate) use observation::*;
pub(crate) use offer_entries::*;
pub(crate) use scratch::*;
pub(crate) use select::*;
pub(crate) use snapshot::*;
