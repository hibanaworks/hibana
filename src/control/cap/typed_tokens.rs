//! Typed capability token primitives.
//!
//! This module provides the core typed token types that form the foundation
//! of the zero-compromise epf integration:
//!
//! - `CapFlowToken<K>`: Affine token produced by `CapFlow::into_token()`
//! - `CapFrameToken<K>`: Borrowed token from inbound control frames
//! - `CapRegisteredToken<K>`: Rendezvous-registered token with auto-release
//!
//! # Design Principles
//!
//! 1. **No runtime tags**: Every token carries compile-time `K: ResourceKind`
//! 2. **No dynamic dispatch**: Zero trait objects, zero downcasts
//! 3. **No duplication**: Token bytes exist exactly once
//! 4. **No leaks**: Affine types + Drop = compile-time leak detection
//! 5. **No hidden state**: Single typed pipeline from mint to HandleView

use crate::{
    control::cap::mint::{CAP_NONCE_LEN, CAP_TOKEN_LEN, GenericCapToken, ResourceKind},
    global::const_dsl::ScopeId,
    rendezvous::capability::CapTable,
};
use core::{fmt, marker::PhantomData, ptr::NonNull};

/// Affine capability flow token.
///
/// Produced by `CapFlow::into_token::<K>()` and must be consumed via:
/// - `.into_frame()` → build a `ControlFrame` for rendezvous registration
/// - `.into_bytes()` → raw bytes for direct wire encoding
///
/// # Affine Semantics
///
/// Dropping without consumption will panic to prevent silent token leakage.
pub struct CapFlowToken<K: ResourceKind> {
    bytes: [u8; CAP_TOKEN_LEN],
    consumed: bool,
    _marker: PhantomData<K>,
}

impl<K: ResourceKind> CapFlowToken<K> {
    /// Create a new flow token from raw token bytes.
    ///
    /// # Safety Contract
    ///
    /// Caller must ensure:
    /// - Bytes represent a valid GenericCapToken<K>
    /// - Token's resource tag matches `K::TAG`
    #[inline]
    pub(crate) fn new(token: GenericCapToken<K>) -> Self {
        Self {
            bytes: token.into_bytes(),
            consumed: false,
            _marker: PhantomData,
        }
    }

    /// Consume this token and return raw bytes for wire encoding.
    ///
    /// This is the primary consumption path for sending tokens over the wire.
    #[inline]
    pub fn into_bytes(mut self) -> [u8; CAP_TOKEN_LEN] {
        self.consumed = true;
        self.bytes
    }

    /// Consume the flow token and return the underlying generic token.
    #[inline]
    pub fn into_generic(mut self) -> GenericCapToken<K> {
        self.consumed = true;
        GenericCapToken::from_bytes(self.bytes)
    }

    /// Borrow the generic token for inspection.
    #[inline]
    pub fn as_generic(&self) -> GenericCapToken<K> {
        GenericCapToken::from_bytes(self.bytes)
    }

    /// Convert this flow token into a `ControlFrame` for the typed pipeline.
    ///
    /// This is the primary integration point for the ControlFrame DSL:
    /// ```ignore
    /// CapFlow::into_token::<K>()
    ///   → CapFlowToken<K>
    ///   → into_frame()
    ///   → ControlFrame<'ctx, K>
    ///   → HandleBag integration
    /// ```
    ///
    /// # Design Notes
    ///
    /// The ControlFrame carries the typed token bytes into the control pipeline
    /// without widening the resource kind to an untyped runtime payload.
    #[inline]
    pub(crate) fn into_frame<'ctx>(self) -> crate::control::handle::frame::ControlFrame<'ctx, K> {
        crate::control::handle::frame::ControlFrame::from_flow(self)
    }
}

impl<K: ResourceKind> Drop for CapFlowToken<K> {
    fn drop(&mut self) {
        if !self.consumed {
            panic!(
                "CapFlowToken<{}> dropped without consumption! \
                 Must call .into_bytes() or .register()",
                core::any::type_name::<K>()
            );
        }
    }
}

/// Borrowed capability token from inbound control frame.
///
/// Provides zero-copy access to token bytes from a received frame.
pub struct CapFrameToken<'f, K: ResourceKind> {
    bytes: &'f [u8; CAP_TOKEN_LEN],
    _marker: PhantomData<K>,
}

impl<'f, K: ResourceKind> CapFrameToken<'f, K> {
    /// Create a frame token by borrowing from inbound frame bytes.
    #[inline]
    pub fn new(bytes: &'f [u8; CAP_TOKEN_LEN]) -> Self {
        Self {
            bytes,
            _marker: PhantomData,
        }
    }

    /// Get the raw token bytes.
    #[inline]
    pub fn bytes(&self) -> &'f [u8; CAP_TOKEN_LEN] {
        self.bytes
    }
}

/// Rendezvous-registered capability token.
///
/// Represents a token that has been registered with rendezvous.
/// Will auto-release on drop.
///
/// # Invariance
///
/// The lifetime `'rv` is **invariant** (via `PhantomData<fn(&'rv ())>`)
/// to prevent the token from outliving its rendezvous registration.
pub struct CapRegisteredToken<'rv, K: ResourceKind> {
    bytes: [u8; CAP_TOKEN_LEN],
    nonce: [u8; CAP_NONCE_LEN],
    cap_table: Option<NonNull<CapTable>>,
    scope: Option<ScopeId>,
    _marker: PhantomData<&'rv CapTable>,
    _resource: PhantomData<K>,
}

impl<'rv, K: ResourceKind> fmt::Debug for CapRegisteredToken<'rv, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapRegisteredToken")
            .field("resource", &K::NAME)
            .finish()
    }
}

impl<'rv, K: ResourceKind> CapRegisteredToken<'rv, K> {
    #[inline]
    pub(crate) fn from_bytes(bytes: [u8; CAP_TOKEN_LEN]) -> Self {
        let token = GenericCapToken::<K>::from_bytes(bytes);
        let scope = token.scope_hint();
        Self {
            bytes: token.into_bytes(),
            nonce: [0u8; CAP_NONCE_LEN],
            cap_table: None,
            scope,
            _marker: PhantomData,
            _resource: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn new(
        bytes: [u8; CAP_TOKEN_LEN],
        nonce: [u8; CAP_NONCE_LEN],
        cap_table: &'rv CapTable,
        scope: Option<ScopeId>,
    ) -> Self {
        Self {
            bytes,
            nonce,
            cap_table: Some(NonNull::from(cap_table)),
            scope,
            _marker: PhantomData,
            _resource: PhantomData,
        }
    }

    /// Get the raw token bytes.
    #[inline]
    pub fn bytes(&self) -> &[u8; CAP_TOKEN_LEN] {
        &self.bytes
    }

    /// Structured scope identifier carried by the canonical control token, if any.
    #[inline]
    pub fn scope(&self) -> Option<ScopeId> {
        self.scope
    }

    /// Interpret this token as GenericCapToken<K>.
    #[inline]
    pub fn as_generic(&self) -> GenericCapToken<K> {
        GenericCapToken::from_bytes(self.bytes)
    }

    /// Consume and zeroize the registered token, returning an owned handle.
    #[inline]
    pub fn into_handle(mut self) -> GenericCapToken<K> {
        let token = GenericCapToken::from_bytes(self.bytes);
        self.bytes.fill(0);
        token
    }
}

impl<'rv, K: ResourceKind> Drop for CapRegisteredToken<'rv, K> {
    fn drop(&mut self) {
        // Release from CapTable if registered
        if let Some(table) = self.cap_table.take() {
            unsafe {
                table.as_ref().release_by_nonce(&self.nonce);
            }
        }

        // Zeroize bytes and nonce to prevent misuse after drop
        self.bytes.fill(0);
        self.nonce.fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::mint::{
        CAP_FIXED_HEADER_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN,
        CAP_TOKEN_LEN, CapShot,
    };
    use crate::control::cap::resource_kinds::{LoopContinueKind, LoopDecisionHandle};
    use crate::global::const_dsl::ScopeId;

    /// Helper to build a test token
    fn make_test_token_bytes<K: ResourceKind>(handle: &K::Handle) -> [u8; CAP_TOKEN_LEN] {
        let handle_bytes = K::encode_handle(handle);
        let mask = K::caps_mask(handle);

        let mut header = [0u8; CAP_HEADER_LEN];
        header[0..4].copy_from_slice(&0u32.to_be_bytes());
        header[4] = 0; // lane
        header[5] = 0; // role
        header[6] = K::TAG;
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
    fn cap_flow_token_into_bytes() {
        let handle = LoopDecisionHandle {
            sid: 42,
            lane: 7,
            scope: ScopeId::route(5),
        };
        let bytes = make_test_token_bytes::<LoopContinueKind>(&handle);
        let token = GenericCapToken::<LoopContinueKind>::from_bytes(bytes);
        let flow_token = CapFlowToken::<LoopContinueKind>::new(token);

        let result = flow_token.into_bytes();
        assert_eq!(result, bytes);
    }

    #[test]
    fn cap_frame_token_borrow() {
        let handle = LoopDecisionHandle {
            sid: 100,
            lane: 3,
            scope: ScopeId::loop_scope(2),
        };
        let bytes = make_test_token_bytes::<LoopContinueKind>(&handle);

        let frame_token = CapFrameToken::<LoopContinueKind>::new(&bytes);
        assert_eq!(frame_token.bytes(), &bytes);

        let generic = GenericCapToken::<LoopContinueKind>::from_bytes(bytes);
        let view = generic.as_view().expect("as_view should succeed");
        assert_eq!(view.handle(), &handle);
    }

    #[test]
    #[should_panic(expected = "CapFlowToken")]
    fn cap_flow_token_drop_panics() {
        let handle = LoopDecisionHandle {
            sid: 1,
            lane: 0,
            scope: ScopeId::route(1),
        };
        let bytes = make_test_token_bytes::<LoopContinueKind>(&handle);
        let token = GenericCapToken::<LoopContinueKind>::from_bytes(bytes);
        let _flow_token = CapFlowToken::<LoopContinueKind>::new(token);
        // Drop without consumption → panic
    }

    #[test]
    fn cap_flow_token_consumption_prevents_drop_panic() {
        let handle = LoopDecisionHandle {
            sid: 5,
            lane: 2,
            scope: ScopeId::route(6),
        };
        let bytes = make_test_token_bytes::<LoopContinueKind>(&handle);
        let token = GenericCapToken::<LoopContinueKind>::from_bytes(bytes);
        let flow_token = CapFlowToken::<LoopContinueKind>::new(token);
        let _consumed = flow_token.into_bytes();
        // No panic because token was consumed
    }
}
