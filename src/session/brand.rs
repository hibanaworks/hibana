//! Invariant storage lifetime marker for the pinned rendezvous owner.
//! Runtime identity and affine issuance are checked by live lease generations.

use core::marker::PhantomData;

/// Keeps the rendezvous's storage lifetime invariant.
#[derive(Clone, Copy)]
pub(crate) struct Brand<'brand> {
    _marker: PhantomData<&'brand mut &'brand ()>,
}
