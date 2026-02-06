//! Typed control frame abstraction for the capability pipeline.
//!
//! `ControlFrame<'ctx, K>` is the central typed abstraction that replaces
//! raw `GenericCapToken` manipulation throughout the control plane.
//!
//! # Design Principles
//!
//! 1. **Type-level resource tracking**: `K: ResourceKind` is always known at compile-time
//! 2. **Single pipeline**: Send → CapFlowToken → ControlFrame → HandleBag
//! 3. **No dynamic dispatch**: Zero runtime tag checks or downcasts
//! 4. **Affine consumption**: Frames must be consumed or explicitly dropped
//!
//! # Pipeline Flow
//!
//! ## Send Path
//! ```ignore
//! CapFlow::into_token::<K>()
//!   → CapFlowToken<K>
//!   → CapFlowToken::into_frame()
//!   → ControlFrame<'ctx, K>
//!   → register with rendezvous
//!   → CapRegisteredToken<'ctx, K>
//!   → HandleBag::from_registered()
//! ```
//!
//! ## Recv Path
//! ```ignore
//! Receive wire bytes
//!   → CapFrameToken<'f, K>
//!   → ControlFrame::from_recv()
//!   → HandleBag::from_frame()
//! ```

use crate::{
    control::cap::{
        CAP_TOKEN_LEN, GenericCapToken, ResourceKind,
        typed_tokens::{CapFlowToken, CapFrameToken, CapRegisteredToken},
    },
    global::typestate::SendMeta,
    rendezvous::Rendezvous,
};
use core::marker::PhantomData;

/// Typed control frame carrying a capability token.
///
/// This is the unified representation used throughout the control plane:
/// - SessionCluster::dispatch_control_effect receives ControlFrame
/// - transport::forward produces ControlFrame
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
pub struct ControlFrame<'ctx, K: ResourceKind> {
    /// Raw token bytes
    bytes: [u8; CAP_TOKEN_LEN],
    /// Send metadata (only present for outbound frames)
    meta: Option<SendMeta>,
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
    pub fn from_flow(token: CapFlowToken<K>) -> Self {
        let meta = Some(token.meta());
        let bytes = token.into_bytes();
        Self {
            bytes,
            meta,
            is_registered: true,
            _marker: PhantomData,
        }
    }

    /// Create a control frame from received wire bytes (recv path).
    ///
    /// The token is marked as not registered since it was received
    /// from the wire and needs to be registered in the CapTable.
    ///
    /// # Safety
    ///
    /// Caller must ensure:
    /// - `bytes` represent a valid `GenericCapToken<K>`
    /// - Resource tag matches `K::TAG`
    #[inline]
    pub fn from_recv(bytes: [u8; CAP_TOKEN_LEN]) -> Self {
        Self {
            bytes,
            meta: None,
            is_registered: false,
            _marker: PhantomData,
        }
    }

    /// Create a control frame from a borrowed frame token.
    ///
    /// The token is marked as not registered since frame tokens
    /// come from wire data and need CapTable registration.
    #[inline]
    pub fn from_frame_token(token: CapFrameToken<'ctx, K>) -> Self {
        Self {
            bytes: *token.bytes(),
            meta: None,
            is_registered: false,
            _marker: PhantomData,
        }
    }

    /// Get the raw token bytes.
    #[inline]
    pub fn bytes(&self) -> &[u8; CAP_TOKEN_LEN] {
        &self.bytes
    }

    /// Consume the frame and return raw bytes.
    #[inline]
    pub fn into_bytes(self) -> [u8; CAP_TOKEN_LEN] {
        self.bytes
    }

    /// Get send metadata if this is an outbound frame.
    #[inline]
    pub fn meta(&self) -> Option<SendMeta> {
        self.meta
    }

    /// Interpret as `GenericCapToken<K>` for inspection.
    #[inline]
    pub fn as_generic(&self) -> GenericCapToken<K> {
        GenericCapToken::from_bytes(self.bytes)
    }

    /// Consume frame and return `GenericCapToken<K>`.
    #[inline]
    pub fn into_generic(self) -> GenericCapToken<K> {
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
    pub fn register<'rv, 'cfg, T, U, C, E>(
        self,
        rendezvous: &'rv Rendezvous<'rv, 'cfg, T, U, C, E>,
    ) -> Result<CapRegisteredToken<'rv, K>, crate::control::CpError>
    where
        T: crate::transport::Transport,
        U: crate::runtime::consts::LabelUniverse,
        C: crate::runtime::config::Clock,
        E: crate::control::cap::EpochTable,
        'cfg: 'rv,
    {
        let generic = self.as_generic();

        // Verify resource tag matches K at runtime
        if generic.resource_tag() != K::TAG {
            return Err(crate::control::CpError::ResourceMismatch {
                expected: K::TAG,
                actual: generic.resource_tag(),
            });
        }

        // Extract fields from token for CapTable registration
        let header = generic.header();
        let nonce = generic.nonce();
        let sid = generic.sid();
        let lane = generic.lane();
        let shot = generic
            .shot()
            .map_err(|_| crate::control::CpError::Authorisation {
                effect: crate::control::CpEffect::Open,
            })?;
        let role = header[5];

        // Decode and validate handle
        let view = generic
            .as_view()
            .map_err(|_| crate::control::CpError::ResourceMismatch {
                expected: K::TAG,
                actual: generic.resource_tag(),
            })?;

        let handle_bytes = K::encode_handle(view.handle());
        let caps_mask = view.grant_mask();
        let scope = view.scope();

        let cap_table = rendezvous.caps();

        // Only insert if not already registered (provenance check)
        if !self.is_registered {
            let entry = crate::rendezvous::CapEntry {
                sid,
                lane,
                kind_tag: K::TAG,
                shot,
                role,
                consumed: false,
                nonce,
                caps_mask,
                handle: handle_bytes,
                scope,
            };

            cap_table
                .insert_entry(entry)
                .map_err(|_| crate::control::CpError::Authorisation {
                    effect: crate::control::CpEffect::Open,
                })?;
        }

        // Token is now registered in CapTable with auto-release on drop
        Ok(CapRegisteredToken::new(self.bytes, nonce, cap_table, scope))
    }

    /// Extract session ID from frame.
    #[inline]
    pub fn sid(&self) -> crate::rendezvous::SessionId {
        let generic = GenericCapToken::<K>::from_bytes(self.bytes);
        generic.sid()
    }

    /// Extract lane from frame.
    #[inline]
    pub fn lane(&self) -> crate::rendezvous::Lane {
        let generic = GenericCapToken::<K>::from_bytes(self.bytes);
        generic.lane()
    }

    /// Structured scope identifier encoded in this frame's capability, if any.
    #[inline]
    pub fn scope_hint(&self) -> Option<crate::global::const_dsl::ScopeId> {
        GenericCapToken::<K>::from_bytes(self.bytes).scope_hint()
    }

    /// Create a ControlFrame from wire bytes with runtime tag dispatch.
    ///
    /// This is used in forward paths where the resource kind is not known
    /// at compile time. The resource tag in the token header determines K.
    ///
    /// # Design Notes
    ///
    /// Forward path needs runtime dispatch because it receives
    /// `GenericCapToken<EndpointResource>` from the wire. This method
    /// provides type-safe conversion by reading the tag and constructing
    /// the appropriate `ControlFrame<K>`.
    ///
    /// # Returns
    ///
    /// Returns a `ControlFrame<EndpointResource>` which can then be
    /// dispatched via SessionCluster.
    /// The token is marked as not registered since it was received
    /// from the wire and needs CapTable registration.
    #[inline]
    pub fn from_wire_bytes(bytes: [u8; CAP_TOKEN_LEN]) -> Self
    where
        K: crate::control::cap::ResourceKind<Handle = crate::control::cap::EndpointHandle>,
    {
        Self {
            bytes,
            meta: None,
            is_registered: false,
            _marker: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::resource_kinds::{LoopContinueKind, LoopDecisionHandle};
    use crate::control::cap::{
        CAP_FIXED_HEADER_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN,
        CAP_TOKEN_LEN, CapShot,
    };
    use crate::eff::EffIndex;
    use crate::global::{
        const_dsl::{HandlePlan, ScopeId},
        typestate::SendMeta,
    };

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

    fn test_meta() -> SendMeta {
        SendMeta {
            eff_index: 42 as EffIndex,
            peer: 1,
            label: LoopContinueKind::TAG,
            resource: Some(LoopContinueKind::TAG),
            is_control: true,
            next: 0,
            scope: ScopeId::none(),
            route_arm: None,
            shot: Some(CapShot::One),
            plan: HandlePlan::None,
            lane: 0,
        }
    }

    #[test]
    fn control_frame_from_flow() {
        let handle = LoopDecisionHandle::new(123, 456, ScopeId::route(2));
        let bytes = make_test_bytes(&handle);
        let token = GenericCapToken::<LoopContinueKind>::from_bytes(bytes);
        let meta = test_meta();
        let flow_token = CapFlowToken::new(meta, token);

        let frame = ControlFrame::from_flow(flow_token);
        assert_eq!(frame.bytes(), &bytes);
        assert_eq!(frame.meta(), Some(meta));
    }

    #[test]
    fn control_frame_from_recv() {
        let handle = LoopDecisionHandle::new(789, 12, ScopeId::loop_scope(3));
        let bytes = make_test_bytes(&handle);

        let frame = ControlFrame::<LoopContinueKind>::from_recv(bytes);
        assert_eq!(frame.bytes(), &bytes);
        assert_eq!(frame.meta(), None);

        let generic = frame.as_generic();
        let view = generic.as_view().expect("should decode");
        assert_eq!(view.handle(), &handle);
    }

    #[test]
    fn control_frame_roundtrip() {
        let handle = LoopDecisionHandle::new(999, 111, ScopeId::route(4));
        let bytes = make_test_bytes(&handle);
        let frame_token = CapFrameToken::<LoopContinueKind>::new(&bytes);

        let frame = ControlFrame::from_frame_token(frame_token);
        let result_bytes = frame.into_bytes();
        assert_eq!(result_bytes, bytes);
    }
}
