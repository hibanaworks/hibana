//! Opaque capability token payload.

use core::{fmt, marker::PhantomData};

use crate::transport::wire::{CodecError, WireEncode};

use super::{
    CAP_CONTROL_HEADER_FIXED_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TOKEN_LEN,
    CapError, CapHeader, WireControlKind,
};

/// Opaque capability-token payload carried by control messages.
///
/// Opaque capability-token payload carried by internal control messages.
///
/// Local endpoint-owned controls use `()` as their payload and never expose a
/// fake all-zero token value. Descriptor metadata and token header details are
/// internal transport/control evidence, not public choreography vocabulary.
#[repr(C)]
#[derive(PartialEq, Eq)]
pub(crate) struct GenericCapToken<K> {
    bytes: [u8; CAP_TOKEN_LEN],
    _marker: PhantomData<K>,
}

impl<K: WireControlKind> fmt::Debug for GenericCapToken<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericCapToken")
            .field("tag", &K::TAG)
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

impl<K> GenericCapToken<K> {
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
