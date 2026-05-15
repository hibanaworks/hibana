//! Internal control-plane kernel behind [`crate::integration::SessionKit`].
//!
//! `hibana` exposes only two public faces: the app surface at the crate root
//! and the integration surface at [`crate::integration`]. This module houses the
//! kernel types that power the integration facade; it is not a third public face.

/// Runtime configuration types.
pub(crate) mod config;
/// Runtime constants and label universe helpers.
pub(crate) mod consts;
