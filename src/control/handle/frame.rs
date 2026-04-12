//! Typed control frame abstraction for the capability pipeline.
//!
//! `ControlFrame<'ctx, K>` is the central typed abstraction that replaces
//! raw `GenericCapToken` manipulation throughout the control plane.
//!
//! # Design Principles
//!
//! 1. **Type-level resource tracking**: `K: ResourceKind` is always known at compile-time
//! 2. **Single pipeline**: Send → CapFlowToken → ControlFrame → registration
//! 3. **No dynamic dispatch**: Zero runtime tag checks or downcasts
//! 4. **Affine consumption**: Frames must be consumed or explicitly dropped
//!
//! # Pipeline Flow
//!
//! ## Send Path
//! ```text
//! CapFlow::into_token::<K>()
//!   → CapFlowToken<K>
//!   → CapFlowToken::into_frame()
//!   → ControlFrame<'ctx, K>
//!   → register with rendezvous
//!   → CapRegisteredToken<'ctx, K>
//! ```
//!
use crate::{
    control::cap::mint::{CAP_TOKEN_LEN, GenericCapToken, ResourceKind},
    control::cap::typed_tokens::{CapFlowToken, CapRegisteredToken},
    rendezvous::{capability::CapEntry, core::Rendezvous},
};
use core::marker::PhantomData;

/// Typed control frame carrying a capability token.
///
/// This is the unified representation used throughout the control plane:
/// - SessionCluster::dispatch_control_effect receives ControlFrame
/// - epf::Kernel processes ControlFrame via HandleBag
///
/// # Provenance Tracking
///
/// The `is_registered` flag tracks whether this frame's token has already been
/// registered in the CapTable:
/// - `true` (from_flow): Token was minted locally and registered during minting.
///   Calling `register()` will skip CapTable insertion to avoid double-registration.
/// - `false` (from_recv): Token was received from wire and needs registration.
///
/// # Invariants
///
/// - The token bytes always represent a valid `GenericCapToken<K>`
/// - Resource tag in header matches `K::TAG`
/// - Lifetime `'ctx` ties frame to its execution context
#[derive(Debug)]
pub(crate) struct ControlFrame<'ctx, K: ResourceKind> {
    /// Raw token bytes
    bytes: [u8; CAP_TOKEN_LEN],
    /// Whether this token is already registered in CapTable.
    /// true = from_flow (minted locally, already registered)
    /// false = from_recv (received from wire, needs registration)
    is_registered: bool,
    /// Lifetime marker
    _marker: PhantomData<&'ctx K>,
}

impl<'ctx, K: ResourceKind> ControlFrame<'ctx, K> {
    /// Create a control frame from a flow token (send path).
    ///
    /// This consumes the `CapFlowToken` and captures its metadata.
    /// The token is marked as already registered since flow tokens
    /// are minted locally and registered during minting.
    #[inline]
    pub(crate) fn from_flow(token: CapFlowToken<K>) -> Self {
        let bytes = token.into_bytes();
        Self {
            bytes,
            is_registered: true,
            _marker: PhantomData,
        }
    }

    /// Interpret as `GenericCapToken<K>` for inspection.
    #[inline]
    pub(crate) fn as_generic(&self) -> GenericCapToken<K> {
        GenericCapToken::from_bytes(self.bytes)
    }

    /// Register this frame with rendezvous and return a registered token.
    ///
    /// This integrates the control frame into the rendezvous capability table,
    /// enabling HandleBag construction and VM execution.
    ///
    /// # Provenance-Aware Registration
    ///
    /// - If `is_registered == true` (from_flow): Token was already registered
    ///   during minting. Skip CapTable insertion to avoid double-registration.
    /// - If `is_registered == false` (from_recv): Token came from wire and
    ///   needs CapTable registration.
    ///
    /// # Design Notes
    ///
    /// The typed pipeline ensures:
    /// 1. Token bytes are validated during ControlFrame construction
    /// 2. Resource tag K is compile-time verified
    /// 3. Rendezvous tracks the registration for auto-release on drop
    ///
    /// # Implementation
    ///
    /// This method validates the token against the rendezvous CapTable.
    /// The returned `CapRegisteredToken` will auto-release on drop.
    #[inline]
    pub(crate) fn register<'rv, 'cfg, T, U, C, E>(
        self,
        rendezvous: &'rv Rendezvous<'rv, 'cfg, T, U, C, E>,
    ) -> Result<CapRegisteredToken<'rv, K>, crate::control::cluster::error::CpError>
    where
        T: crate::transport::Transport,
        U: crate::runtime::consts::LabelUniverse,
        C: crate::runtime::config::Clock,
        E: crate::control::cap::mint::EpochTable,
        'cfg: 'rv,
    {
        let generic = self.as_generic();

        // Verify resource tag matches K at runtime
        if generic.resource_tag() != K::TAG {
            return Err(crate::control::cluster::error::CpError::ResourceMismatch {
                expected: K::TAG,
                actual: generic.resource_tag(),
            });
        }

        // Extract fields from token for CapTable registration
        let header = generic.header();
        let nonce = generic.nonce();
        let sid = generic.sid();
        let lane = generic.lane();
        let shot =
            generic
                .shot()
                .map_err(|_| crate::control::cluster::error::CpError::Authorisation {
                    effect: crate::control::cluster::effects::CpEffect::Open,
                })?;
        let role = header[5];

        // Decode and validate handle
        let view = generic.as_view().map_err(|_| {
            crate::control::cluster::error::CpError::ResourceMismatch {
                expected: K::TAG,
                actual: generic.resource_tag(),
            }
        })?;

        let handle_bytes = K::encode_handle(view.handle());
        let cap_table = rendezvous.caps();

        // Only insert if not already registered (provenance check)
        if !self.is_registered {
            let entry = CapEntry {
                sid,
                lane_raw: lane.as_wire(),
                kind_tag: K::TAG,
                shot_state: shot.as_u8(),
                role,
                nonce,
                handle: handle_bytes,
            };

            cap_table.insert_entry(entry).map_err(|_| {
                crate::control::cluster::error::CpError::Authorisation {
                    effect: crate::control::cluster::effects::CpEffect::Open,
                }
            })?;
        }

        // Token is now registered in CapTable with auto-release on drop.
        Ok(CapRegisteredToken::new(
            self.bytes,
            nonce,
            cap_table,
            view.scope(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::mint::{
        CAP_FIXED_HEADER_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN, CapShot,
    };
    use crate::control::cap::resource_kinds::{LoopContinueKind, LoopDecisionHandle};
    use crate::global::const_dsl::ScopeId;

    fn make_test_bytes(handle: &LoopDecisionHandle) -> [u8; CAP_TOKEN_LEN] {
        let handle_bytes = LoopContinueKind::encode_handle(handle);
        let mask = LoopContinueKind::caps_mask(handle);

        let mut header = [0u8; CAP_HEADER_LEN];
        header[0..4].copy_from_slice(&0u32.to_be_bytes());
        header[4] = 0;
        header[5] = 0;
        header[6] = LoopContinueKind::TAG;
        header[7] = CapShot::One.as_u8();
        header[8..10].copy_from_slice(&mask.bits().to_be_bytes());
        header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
            .copy_from_slice(&handle_bytes);

        let mut bytes = [0u8; CAP_TOKEN_LEN];
        bytes[..CAP_NONCE_LEN].copy_from_slice(&[0u8; CAP_NONCE_LEN]);
        bytes[CAP_NONCE_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN].copy_from_slice(&header);
        bytes[CAP_NONCE_LEN + CAP_HEADER_LEN..].copy_from_slice(&[0u8; CAP_TAG_LEN]);
        bytes
    }

    #[test]
    fn control_frame_from_flow() {
        let handle = LoopDecisionHandle {
            sid: 123,
            lane: 456,
            scope: ScopeId::route(2),
        };
        let bytes = make_test_bytes(&handle);
        let token = GenericCapToken::<LoopContinueKind>::from_bytes(bytes);
        let flow_token = CapFlowToken::new(token);

        let frame = ControlFrame::from_flow(flow_token);
        let generic = frame.as_generic();
        assert_eq!(generic.into_bytes(), bytes);
        let generic = GenericCapToken::<LoopContinueKind>::from_bytes(bytes);
        let view = generic.as_view().expect("should decode");
        assert_eq!(view.handle(), &handle);
    }
}
