//! Frontier-selection helpers for `offer()`.
//!
//! # Unsafe Owner Contract
//!
//! This module owns frontier scratch slices borrowed from the endpoint runtime
//! image. Unsafe blocks here may form slices over that storage only after the
//! endpoint has initialized the matching capacity fields for the current
//! generation.

use core::{
    convert::TryFrom,
    mem,
    ops::{Deref, DerefMut, Index, IndexMut},
    slice,
};

use crate::global::const_dsl::ScopeId;
use crate::global::role_program::{LaneSet, LaneSetView, LaneWord};
use crate::global::typestate::{MAX_STATES, StateIndex, state_index_to_usize};

const FRONTIER_SLOT_MASK_BITS: usize = u8::BITS as usize;

use super::offer::CurrentScopeSelectionMeta;

#[path = "frontier/entry_sets.rs"]
mod entry_sets;
#[path = "frontier/kind.rs"]
mod kind;
#[path = "frontier/offer_entries.rs"]
mod offer_entries;
#[path = "frontier/scratch.rs"]
mod scratch;
#[path = "frontier/select.rs"]
mod select;
#[path = "frontier/snapshot.rs"]
mod snapshot;

pub(crate) use entry_sets::*;
pub(crate) use kind::*;
pub(crate) use offer_entries::*;
pub(crate) use scratch::*;
pub(crate) use select::*;
pub(crate) use snapshot::*;
