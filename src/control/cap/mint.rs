//! Capability minting and registered-token validation primitives.
//!
//! Hibana mints control tokens through const-first strategies baked into
//! `RoleProgram` and endpoint-owned local control send paths. Rendezvous-local
//! capability tables own nonce release and snapshot restore side effects.
//!
//! # Endpoint-Local Witnesses And Capability Authority
//!
//! Endpoint-local control progression is witnessed by rendezvous-scoped brands
//! and epoch markers. Endpoint-owned local control tokens register their nonce in
//! rendezvous-local state so send rollback, drop cleanup, and snapshot-aware
//! release are owned by the rendezvous. Explicit protocol-owned wire tokens are
//! descriptor/header validated; their authority is the projected control
//! descriptor and the protocol-owned wire-control kind contract. Endpoint-owned handle
//! minting is crate-owned; explicit wire controls never expose runtime mint
//! authority.
//!
//! ## Design Principles
//!
//! 1. **No global state**: Epoch is tracked via type-level witnesses, not global counters
//! 2. **Affine linearity**: endpoint state carries a rendezvous-scoped owner witness
//! 3. **Compile-time safety**: endpoint-owned epoch witnesses remain in the type system
//! 4. **AMPST compliance**: Integrates with cancellation termination (ECOOP'22)
//!
//! ## Usage Example
//!
//! Internally, the rendezvous core mints a rendezvous-scoped `Owner` witness
//! for the active endpoint. Application code never receives the brand directly;
//! the cursor endpoint stores the witness and exposes typed control operations.
//!
//! ## Integration with Endpoint
//!
//! The internal endpoint implementation stores `Owner<'rv, Step>` alongside
//! `EndpointEpoch<'rv, Table>`. Control plane operations verify epoch progression
//! through the `Step` type parameter, ensuring:
//!
//! - **Affine progression**: Each operation consumes `Endpoint<Step>` and produces
//!   `Endpoint<NextStep>`, making reuse impossible at compile time.
//! - **API simplicity**: Users work with `Endpoint` directly; witness mechanics are hidden
//!   in the `pub(crate)` implementation.
//!
//! The approach keeps witness bookkeeping internal: the rendezvous retains the
//! brand token and application code never handles witness machinery directly.
//!
//! # Wire Format
//!
//! Capability tokens are 56 bytes on the wire:
//! ```text
//! [16B nonce | 40B descriptor header]
//! descriptor header = fixed control metadata plus resource-owned handle bytes
//! ```
//!
//! The default runtime is trusted-domain registered-token state, not a keyed verifier.
//! Endpoint-owned token authority comes from a nonce entry minted by the same
//! rendezvous plus descriptor/header validation. Explicit wire-token authority
//! comes from descriptor/header validation and the protocol-owned wire-control
//! kind contract; it is not registered in `CapTable`.
//! Token bytes stop at the descriptor header; trailing extensions are outside
//! the capability authority model.
//!
//! # Usage Pattern
//!
//! ## SessionCluster-driven endpoint minting
//!
//! ```rust,ignore
//! let controller = rv
//!     .session(sid)
//!     .role(&CONTROLLER)
//!     .enter()?;
//! controller.flow::<CancelMsg>()?.send(&()).await?;
//! ```
//!
//! ## Custom Wire Control Example
//!
//! ```rust
//! use hibana::integration::cap::{GenericCapToken, WireControlEffect, WireControlKind};
//!
//! struct PageControl;
//!
//! impl WireControlKind for PageControl {
//!     const TAG: u8 = 1;
//!     const EFFECT: WireControlEffect = WireControlEffect::Fence;
//! }
//!
//! fn round_trip(token: GenericCapToken<PageControl>) -> GenericCapToken<PageControl> {
//!     // Explicit wire-control messages carry the opaque token bytes directly.
//!     GenericCapToken::from_bytes(token.into_bytes())
//! }
//! ```

mod epoch;
mod error;
mod header;
mod resource;
mod strategy;
mod token;

#[cfg(all(test, hibana_repo_tests))]
pub(crate) use crate::global::const_dsl::ControlScopeKind;
pub(crate) use epoch::*;
pub(crate) use epoch::{EndpointEpoch, Owner};
pub(crate) use error::CapError;
pub(crate) use header::{CapHeader, CapShot, ControlOp, ControlPath};
#[cfg(all(test, hibana_repo_tests))]
pub(crate) use resource::EndpointHandle;
#[cfg(all(test, hibana_repo_tests))]
pub(crate) use resource::EndpointResource;
pub(crate) use resource::LocalControlKind;
pub use resource::{WireControlEffect, WireControlKind};
pub(crate) use strategy::*;
pub use token::GenericCapToken;

/// Length of the nonce segment inside a capability token.
pub const CAP_NONCE_LEN: usize = 16;
/// Length of the header segment inside a capability token.
pub const CAP_HEADER_LEN: usize = 40;
/// Number of fixed bytes used by the descriptor-first control header codec.
///
/// Layout:
/// - version: 1
/// - sid: 4
/// - lane: 1
/// - role: 1
/// - tag: 1
/// - op: 1
/// - path: 1
/// - shot: 1
/// - scope_kind: 1
/// - flags: 1
/// - scope_id: 2
/// - epoch: 2
pub const CAP_CONTROL_HEADER_FIXED_LEN: usize = 17;
/// Number of bytes available for resource-specific handle encoding.
pub const CAP_HANDLE_LEN: usize = CAP_HEADER_LEN - CAP_CONTROL_HEADER_FIXED_LEN;
/// Total length of a capability token on the wire.
pub const CAP_TOKEN_LEN: usize = CAP_NONCE_LEN + CAP_HEADER_LEN;

// ============================================================================
// Default implementation (trusted-domain registered-token state)
// ============================================================================
//
// The default strategy is deliberately non-cryptographic. It is used when
// capability tokens stay inside a rendezvous-owned trust domain and release
// authority is the registered-token state, not a keyed authenticator. Cross-domain
// authentication belongs in a protocol/integration layer that can model and
// verify that trust boundary explicitly.

#[cfg(all(test, hibana_repo_tests))]
#[path = "mint_tests.rs"]
mod tests;
