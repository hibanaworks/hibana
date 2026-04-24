//! Erased registered capability-token primitives.
//!
//! # Design Principles
//!
//! 1. **No dynamic dispatch**: Zero trait objects, zero downcasts
//! 2. **No duplication**: Token bytes exist exactly once
//! 3. **No leaks**: affine ownership + Drop releases registered authority
//! 4. **No public token surface**: app sends complete as `SendResult<()>`

use crate::{
    control::cap::mint::{CAP_NONCE_LEN, CAP_TOKEN_LEN},
    rendezvous::capability::{CapReleaseCtx, CapTable},
};
use core::marker::PhantomData;

pub(crate) struct RawRegisteredCapToken<'rv> {
    bytes: [u8; CAP_TOKEN_LEN],
    nonce: [u8; CAP_NONCE_LEN],
    release_ctx: Option<CapReleaseCtx>,
    _marker: PhantomData<&'rv CapTable>,
}

impl<'rv> RawRegisteredCapToken<'rv> {
    #[inline]
    pub(crate) fn from_registered_bytes(
        bytes: [u8; CAP_TOKEN_LEN],
        nonce: [u8; CAP_NONCE_LEN],
        release_ctx: CapReleaseCtx,
    ) -> Self {
        Self {
            bytes,
            nonce,
            release_ctx: Some(release_ctx),
            _marker: PhantomData,
        }
    }
}

impl<'rv> Drop for RawRegisteredCapToken<'rv> {
    fn drop(&mut self) {
        if let Some(release_ctx) = self.release_ctx.take() {
            release_ctx.release(&self.nonce);
        }

        self.bytes.fill(0);
        self.nonce.fill(0);
    }
}
