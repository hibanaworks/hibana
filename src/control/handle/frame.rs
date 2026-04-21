//! Typed control frame abstraction for the capability pipeline.
//!
//! `ControlFrame<'ctx, K>` is a tiny typed wrapper around raw token bytes used
//! by test-only helpers.
//!
//! # Design Principles
//!
//! 1. **Type-level resource tracking**: `K: ResourceKind` is always known at compile-time
//! 2. **No dynamic dispatch**: Zero runtime tag checks or downcasts
//! 3. **Affine consumption**: Frames must be consumed or explicitly dropped
//!
//! # Pipeline Flow
//!
//! ## Send Path
//! ```text
//! CapFlow::into_token::<K>()
//!   → CapFlowToken<K>
//!   → ControlFrame<'ctx, K>
//!   → inspect typed bytes in tests
//! ```
//!
use crate::{
    control::cap::mint::{CAP_TOKEN_LEN, GenericCapToken, ResourceKind},
    control::cap::typed_tokens::CapFlowToken,
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
            _marker: PhantomData,
        }
    }

    /// Interpret as `GenericCapToken<K>` for inspection.
    #[inline]
    pub(crate) fn as_generic(&self) -> GenericCapToken<K> {
        GenericCapToken::from_bytes(self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::mint::{
        CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN, CapHeader, CapShot, ControlResourceKind,
    };
    use crate::control::cap::resource_kinds::{LoopContinueKind, LoopDecisionHandle};
    use crate::global::const_dsl::ScopeId;
    use crate::substrate::{Lane, SessionId};

    fn make_test_bytes(handle: &LoopDecisionHandle) -> [u8; CAP_TOKEN_LEN] {
        let handle_bytes = LoopContinueKind::encode_handle(handle);

        let mut header = [0u8; CAP_HEADER_LEN];
        CapHeader::new(
            SessionId::new(handle.sid),
            Lane::new(handle.lane as u32),
            0,
            LoopContinueKind::TAG,
            LoopContinueKind::LABEL,
            LoopContinueKind::OP,
            LoopContinueKind::PATH,
            CapShot::One,
            LoopContinueKind::SCOPE,
            0,
            handle.scope.local_ordinal(),
            0,
            handle_bytes,
        )
        .encode(&mut header);

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
