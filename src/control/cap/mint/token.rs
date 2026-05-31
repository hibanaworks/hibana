//! Opaque capability token payload.

use core::{fmt, marker::PhantomData};

#[cfg(all(test, hibana_repo_tests))]
use crate::global::const_dsl::ControlScopeKind;
use crate::transport::wire::{CodecError, WireEncode};

use super::{
    CAP_CONTROL_HEADER_FIXED_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TOKEN_LEN,
    CapError, CapHeader, WireControlKind,
};
#[cfg(all(test, hibana_repo_tests))]
use super::{CapShot, ControlOp, ControlPath, EndpointHandle, EndpointResource};

#[inline]
#[cfg(all(test, hibana_repo_tests))]
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
#[cfg(all(test, hibana_repo_tests))]
fn decode_canonical_endpoint_identity(
    token: &GenericCapToken<EndpointResource>,
) -> Result<(CapHeader, EndpointHandle), CapError> {
    let header = token.control_header()?;
    if !is_canonical_endpoint_header(header) {
        return Err(CapError);
    }

    let handle = EndpointResource::decode_identity(token.handle_bytes()).map_err(|_| CapError)?;
    let matches_header =
        handle.sid == header.sid() && handle.lane == header.lane() && handle.role == header.role();
    let matches_encoding = EndpointResource::encode_identity(&handle) == token.handle_bytes();
    if !matches_header || !matches_encoding {
        return Err(CapError);
    }

    Ok((header, handle))
}

/// Opaque capability-token payload carried by control messages.
///
/// Protocol authors name this type in a `g::Msg<..., GenericCapToken<K>>`
/// payload when a protocol-owned wire token is supplied explicitly. Local
/// endpoint-owned controls use `()` as their payload and never expose a fake
/// all-zero token value. Descriptor metadata and token header details live under
/// the integration capability metadata bucket; ordinary choreography code
/// should only pass the token as an opaque payload.
#[repr(C)]
#[derive(PartialEq, Eq)]
pub struct GenericCapToken<K> {
    bytes: [u8; CAP_TOKEN_LEN],
    _marker: PhantomData<K>,
}

impl<K: WireControlKind> fmt::Debug for GenericCapToken<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericCapToken")
            .field("resource", &K::NAME)
            .field("encoded_len", &CAP_TOKEN_LEN)
            .finish()
    }
}

impl<K> Copy for GenericCapToken<K> {}

impl<K> Clone for GenericCapToken<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> GenericCapToken<K> {
    #[inline(always)]
    pub(crate) const fn from_raw_bytes(bytes: [u8; CAP_TOKEN_LEN]) -> Self {
        Self {
            bytes,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) const fn into_raw_bytes(self) -> [u8; CAP_TOKEN_LEN] {
        self.bytes
    }

    #[inline]
    fn header_slice(&self) -> &[u8; CAP_HEADER_LEN] {
        self.bytes[CAP_NONCE_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN]
            .try_into()
            .expect("CAP_HEADER_LEN is compile-time constant")
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
}

impl<K: WireControlKind> GenericCapToken<K> {
    #[inline(always)]
    pub const fn from_bytes(bytes: [u8; CAP_TOKEN_LEN]) -> Self {
        Self::from_raw_bytes(bytes)
    }

    #[inline(always)]
    pub const fn into_bytes(self) -> [u8; CAP_TOKEN_LEN] {
        self.into_raw_bytes()
    }
}

impl<K> GenericCapToken<K> {
    #[cfg(all(test, hibana_repo_tests))]
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

    pub(crate) fn handle_bytes(&self) -> [u8; CAP_HANDLE_LEN] {
        *self.handle_bytes_ref()
    }
}

#[cfg(all(test, hibana_repo_tests))]
impl GenericCapToken<EndpointResource> {
    #[inline]
    pub(crate) fn endpoint_header(&self) -> Result<CapHeader, CapError> {
        let (header, _handle) = decode_canonical_endpoint_identity(self)?;
        Ok(header)
    }

    #[inline]
    pub(crate) fn endpoint_identity(&self) -> Result<EndpointHandle, CapError> {
        decode_canonical_endpoint_identity(self).map(|(_, handle)| handle)
    }
}

impl<K: WireControlKind> WireEncode for GenericCapToken<K> {
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
