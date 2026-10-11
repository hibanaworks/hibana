//! Runtime kernel behind [`crate::runtime::SessionKit`].
//!
//! `hibana` exposes only two public faces: the app surface at the crate root
//! and the runtime surface at [`crate::runtime`]. This module houses the kernel
//! types that power the runtime facade; it is not a third public face.

/// Runtime constants.
pub(crate) mod consts;
/// Checked arithmetic shared by resident and scratch layout owners.
pub(crate) mod layout;
/// Runtime resource substrate.
pub(crate) mod resources;
mod unique_match;

pub(crate) use unique_match::{UniqueMatch, UniqueMatchFailure};
