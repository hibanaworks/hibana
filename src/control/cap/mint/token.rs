//! Opaque capability token and typed handle views.

use core::{fmt, marker::PhantomData};

use crate::global::const_dsl::{ControlScopeKind, ScopeId};
use crate::transport::wire::{CodecError, Payload, WireEncode, WirePayload};

use super::{
    CAP_CONTROL_HEADER_FIXED_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TOKEN_LEN,
    CapError, CapHeader, EndpointResource, ResourceKind,
};
#[cfg(test)]
use super::{CapShot, ControlOp, ControlPath, EndpointHandle};

#[inline]
#[cfg(test)]
pub(crate) const fn is_canonical_endpoint_header(header: CapHeader) -> bool {
    header.tag() == EndpointResource::TAG
        && matches!(header.op(), ControlOp::Fence)
        && matches!(header.path(), ControlPath::Local)
        && matches!(header.shot(), CapShot::One)
        && matches!(header.scope_kind(), ControlScopeKind::None)
        && header.flags() == 0
        && header.scope_id() == 0
        && header.epoch() == 0
}

#[inline]
#[cfg(test)]
fn decode_canonical_endpoint_identity(
    token: &GenericCapToken<EndpointResource>,
) -> Result<(CapHeader, EndpointHandle), CapError> {
    let header = token.control_header()?;
    if !is_canonical_endpoint_header(header) {
        return Err(CapError::Mismatch);
    }

    let mut handle =
        EndpointResource::decode_handle(token.handle_bytes()).map_err(|_| CapError::Mismatch)?;
    let matches_header =
        handle.sid == header.sid() && handle.lane == header.lane() && handle.role == header.role();
    let matches_encoding = EndpointResource::encode_handle(&handle) == token.handle_bytes();
    if !matches_header || !matches_encoding {
        EndpointResource::zeroize(&mut handle);
        return Err(CapError::Mismatch);
    }

    Ok((header, handle))
}

#[inline]
const fn scope_from_header(header: CapHeader) -> Option<ScopeId> {
    match header.scope_kind() {
        ControlScopeKind::Route => Some(ScopeId::route(header.scope_id())),
        ControlScopeKind::Loop => Some(ScopeId::loop_scope(header.scope_id())),
        _ => None,
    }
}

/// Typed view over a capability handle exposed to an external policy VM.
///
/// The view carries the original resource payload together with the structured
/// scope metadata recovered from the descriptor-first control header.
pub struct HandleView<'ctx, K: ResourceKind> {
    raw: &'ctx [u8; CAP_HANDLE_LEN],
    handle: K::Handle,
    scope: Option<ScopeId>,
}

impl<'ctx, K: ResourceKind> HandleView<'ctx, K> {
    #[inline]
    pub(crate) fn decode(
        raw: &'ctx [u8; CAP_HANDLE_LEN],
        scope: Option<ScopeId>,
    ) -> Result<Self, CapError> {
        let handle = K::decode_handle(*raw)?;
        Ok(Self { raw, handle, scope })
    }

    /// Borrow the encoded resource payload.
    #[inline]
    pub fn bytes(&self) -> &'ctx [u8; CAP_HANDLE_LEN] {
        self.raw
    }

    /// Borrow the decoded handle payload.
    #[inline]
    pub fn handle(&self) -> &K::Handle {
        &self.handle
    }

    /// Structured scope identifier encoded in this handle, when available.
    #[inline]
    pub fn scope(&self) -> Option<ScopeId> {
        self.scope
    }
}

impl<'ctx, K: ResourceKind> Drop for HandleView<'ctx, K> {
    fn drop(&mut self) {
        K::zeroize(&mut self.handle);
    }
}

/// Opaque capability-token payload carried by control messages.
///
/// Protocol authors name this type in a `g::Msg<..., GenericCapToken<K>, K>`
/// payload when a protocol-owned wire token is supplied explicitly. Local
/// endpoint-owned controls use `()` as their payload and never expose a fake
/// all-zero token value. Descriptor metadata and token header details live under
/// the integration capability metadata bucket; ordinary choreography code
/// should only pass the token as an opaque payload.
#[repr(C)]
#[derive(PartialEq, Eq)]
pub struct GenericCapToken<K: ResourceKind> {
    bytes: [u8; CAP_TOKEN_LEN],
    _marker: PhantomData<K>,
}

impl<K: ResourceKind> fmt::Debug for GenericCapToken<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericCapToken")
            .field("resource", &K::NAME)
            .field("encoded_len", &CAP_TOKEN_LEN)
            .finish()
    }
}

impl<K: ResourceKind> Copy for GenericCapToken<K> {}

impl<K: ResourceKind> Clone for GenericCapToken<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: ResourceKind> GenericCapToken<K> {
    #[inline(always)]
    pub const fn from_bytes(bytes: [u8; CAP_TOKEN_LEN]) -> Self {
        Self {
            bytes,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub const fn into_bytes(self) -> [u8; CAP_TOKEN_LEN] {
        self.bytes
    }

    #[inline]
    fn header_slice(&self) -> &[u8; CAP_HEADER_LEN] {
        self.bytes[CAP_NONCE_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN]
            .try_into()
            .expect("CAP_HEADER_LEN is compile-time constant")
    }

    #[cfg(test)]
    pub(crate) fn raw_header(&self) -> [u8; CAP_HEADER_LEN] {
        let mut header = [0u8; CAP_HEADER_LEN];
        header.copy_from_slice(self.header_slice());
        header
    }

    #[inline]
    pub(crate) fn control_header(&self) -> Result<CapHeader, CapError> {
        let mut header = [0u8; CAP_HEADER_LEN];
        header.copy_from_slice(self.header_slice());
        CapHeader::decode(header)
    }

    #[inline]
    fn typed_header(&self) -> Result<CapHeader, CapError> {
        let header = self.control_header()?;
        if header.tag() != K::TAG {
            return Err(CapError::Mismatch);
        }
        Ok(header)
    }

    /// Extract the structured scope identifier encoded in the handle, if any.
    ///
    /// Header, tag, and handle decode failures are returned instead of being
    /// collapsed into `None`, which is reserved for valid tokens without
    /// structured scope metadata.
    pub fn scope(&self) -> Result<Option<ScopeId>, CapError> {
        self.as_view().map(|view| view.scope())
    }

    pub(crate) fn handle_bytes(&self) -> [u8; CAP_HANDLE_LEN] {
        *self.handle_bytes_ref()
    }

    /// Get a reference to the handle bytes within the token.
    ///
    /// This is a zero-copy operation that returns a slice reference
    /// to the handle payload embedded in the token header.
    #[inline(always)]
    pub(crate) fn handle_bytes_ref(&self) -> &[u8; CAP_HANDLE_LEN] {
        self.header_slice()
            [CAP_CONTROL_HEADER_FIXED_LEN..CAP_CONTROL_HEADER_FIXED_LEN + CAP_HANDLE_LEN]
            .try_into()
            .expect("CAP_HANDLE_LEN is compile-time constant")
    }

    #[cfg(test)]
    pub(crate) fn decode_handle(&self) -> Result<K::Handle, CapError> {
        self.typed_header()?;
        K::decode_handle(self.handle_bytes())
    }

    /// Extract a HandleView from this token.
    ///
    /// This provides zero-copy access to the embedded handle and its capabilities.
    /// The HandleView lifetime is bounded by the token's lifetime.
    ///
    /// # Type Safety
    ///
    /// The type parameter selects the expected [`ResourceKind`]; the wire header
    /// tag is validated before exposing the typed view. The returned
    /// `HandleView` cannot outlive the token.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn inspect(token: GenericCapToken<LoopContinueKind>) -> Result<(), CapError> {
    ///     let view = token.as_view()?;
    ///     let scope = view.scope();
    ///     let _ = scope;
    ///     Ok(())
    /// }
    /// ```
    pub fn as_view(&self) -> Result<HandleView<'_, K>, CapError> {
        let header = self.typed_header()?;
        HandleView::decode(self.handle_bytes_ref(), scope_from_header(header))
    }
}

impl GenericCapToken<EndpointResource> {
    #[cfg(test)]
    #[inline]
    pub(crate) fn endpoint_header(&self) -> Result<CapHeader, CapError> {
        let (header, mut handle) = decode_canonical_endpoint_identity(self)?;
        EndpointResource::zeroize(&mut handle);
        Ok(header)
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn endpoint_identity(&self) -> Result<EndpointHandle, CapError> {
        decode_canonical_endpoint_identity(self).map(|(_, handle)| handle)
    }
}

impl<K: ResourceKind> WireEncode for GenericCapToken<K> {
    fn encoded_len(&self) -> Option<usize> {
        Some(CAP_TOKEN_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < CAP_TOKEN_LEN {
            return Err(CodecError::Truncated);
        }
        out[0..CAP_TOKEN_LEN].copy_from_slice(&self.bytes);
        Ok(CAP_TOKEN_LEN)
    }
}

impl<K: ResourceKind> WirePayload for GenericCapToken<K> {
    type Decoded<'a> = Self;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        let bytes_in = input.as_bytes();
        if bytes_in.len() < CAP_TOKEN_LEN {
            return Err(CodecError::Truncated);
        }
        if bytes_in.len() != CAP_TOKEN_LEN {
            return Err(CodecError::Invalid("trailing bytes after GenericCapToken"));
        }
        Ok(())
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        let bytes_in = input.as_bytes();
        let mut bytes = [0u8; CAP_TOKEN_LEN];
        bytes.copy_from_slice(bytes_in);
        Self {
            bytes,
            _marker: PhantomData,
        }
    }
}
