//! Runtime kernel behind [`crate::runtime::SessionKit`].
//!
//! `hibana` exposes only two public faces: the app surface at the crate root
//! and the runtime surface at [`crate::runtime`]. This module houses the kernel
//! types that power the runtime facade; it is not a third public face.

/// Runtime configuration types.
pub(crate) mod config;
/// Runtime constants.
pub(crate) mod consts;
