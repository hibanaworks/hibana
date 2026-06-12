//! Opaque capability token payload.

use core::fmt;

use super::{
    CAP_CONTROL_HEADER_FIXED_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TOKEN_LEN,
    CapError, CapHeader,
};

/// Opaque descriptor token bytes used only after Hibana has selected a control path.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ControlToken {
    bytes: [u8; CAP_TOKEN_LEN],
}

impl fmt::Debug for ControlToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControlToken")
            .field("encoded_len", &CAP_TOKEN_LEN)
            .finish()
    }
}

impl ControlToken {
    #[inline(always)]
    pub(crate) const fn from_raw_bytes(bytes: [u8; CAP_TOKEN_LEN]) -> Self {
        Self { bytes }
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
