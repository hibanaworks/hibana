//! Internal control-plane kernel behind [`crate::substrate::SessionKit`].
//!
//! `hibana` exposes only two public faces: the app surface at the crate root
//! and the substrate surface at [`crate::substrate`]. This module houses the
//! kernel types that power the substrate facade; it is not a third public face.

/// Runtime configuration types.
pub(crate) mod config;
/// Runtime constants and label universe helpers.
pub(crate) mod consts;
/// Management protocol surface.
pub(crate) mod mgmt;
