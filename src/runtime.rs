//! Control-plane facade built on top of the SessionCluster kernel.
//!
//! Applications interact with the strongly typed [`SessionCluster`] defined
//! here. It exposes rendezvous registration, effect evaluation, and endpoint
//! execution. Internally it delegates CpEffect handling to the kernel in
//! `control::cluster`, but no compatibility layer is kept – this is the canonical
//! public API.

/// Runtime configuration types.
pub mod config;
/// Runtime constants and label universe helpers.
pub mod consts;
/// Management protocol surface.
pub mod mgmt;

use crate::control::cluster::SessionCluster as KernelCluster;

/// Typed control-plane cluster that owns Rendezvous instances.
///
/// SessionCluster takes ownership of Rendezvous, ensuring proper RAII:
/// - Drop order: SessionCluster → Rendezvous → LaneLease
/// - No self-referential lifetime issues
/// - Type-level proof of affine resource management
///
/// `MAX_RV` bounds the number of rendezvous instances (local + remote).
pub type SessionCluster<'cfg, T, U, C, const MAX_RV: usize> = KernelCluster<'cfg, T, U, C, MAX_RV>;

pub use crate::control::cluster::SpliceOperands;
