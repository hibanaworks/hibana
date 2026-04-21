//! Compile-time typed handle bag for VM execution contexts.
//!
//! `HandleBag<'ctx, Spec>` stores capability tokens according to a type-level
//! specification `Spec` (a cons-list built from [`crate::control::handle::spec::Nil`] /
//! [`crate::control::handle::spec::Cons`]). Each node holds exactly one
//! `GenericCapToken<K>`
//! together with the tail bag, enabling affine consumption without heap
//! allocation.
//!
//! # Guarantees
//! - **Compile-time accuracy** — only handles declared in `Spec` exist.
//! - **Affine consumption** — obtaining a head handle consumes it and returns
//!   the tail bag for subsequent operations.
//! - **Zero alloc** — storage is stack-based; no `Box` / heap usage.
//! - **Type safety** — incorrect resource kinds fail to compile.

use crate::control::cap::mint::{GenericCapToken, ResourceKind};
use crate::control::cap::typed_tokens::CapFrameToken;
use crate::control::handle::spec::{Cons, HandleSpecList, Nil};
use core::marker::PhantomData;

/// Typed handle bag parameterised by the specification list `Spec`.
pub(crate) struct HandleBag<'ctx, Spec>
where
    Spec: HandleSpecList + BagStorage<'ctx>,
{
    storage: Spec::Storage,
}

impl<'ctx> HandleBag<'ctx, Nil> {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self { storage: () }
    }
}

impl<'ctx> Default for HandleBag<'ctx, Nil> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'ctx, K, Tail> HandleBag<'ctx, Cons<K, Tail>>
where
    K: ResourceKind,
    Tail: HandleSpecList + BagStorage<'ctx>,
{
    /// Construct from an inbound frame token.
    #[inline]
    pub(crate) fn from_frame(tail: HandleBag<'ctx, Tail>, token: CapFrameToken<'ctx, K>) -> Self {
        Self {
            storage: Node {
                token: GenericCapToken::from_bytes(*token.bytes()),
                tail: tail.into_storage(),
                _marker: PhantomData,
            },
        }
    }

    /// Consume the head token and expose it to the closure.
    ///
    /// The closure receives ownership of the canonical token and the remaining
    /// bag. Returning the token is optional – affine discipline is enforced by
    /// the type system.
    #[inline]
    pub(crate) fn with_token<R>(
        self,
        f: impl FnOnce(GenericCapToken<K>, HandleBag<'ctx, Tail>) -> R,
    ) -> R {
        let Node { token, tail, .. } = self.storage;
        let tail_bag = HandleBag { storage: tail };
        f(token, tail_bag)
    }
}

impl<'ctx, Spec> HandleBag<'ctx, Spec>
where
    Spec: HandleSpecList + BagStorage<'ctx>,
{
    #[inline(always)]
    pub(crate) fn into_storage(self) -> Spec::Storage {
        self.storage
    }
}

// ---------------------------------------------------------------------------
// Internal storage machinery
// ---------------------------------------------------------------------------

pub(crate) trait BagStorage<'ctx>: HandleSpecList {
    type Storage: StorageLifetime<'ctx>;
}

pub(crate) trait StorageLifetime<'ctx> {}

impl<'ctx> BagStorage<'ctx> for Nil {
    type Storage = ();
}

impl<'ctx> StorageLifetime<'ctx> for () {}

pub(crate) struct Node<'ctx, K, Tail>
where
    K: ResourceKind,
    Tail: HandleSpecList + BagStorage<'ctx>,
{
    token: GenericCapToken<K>,
    tail: Tail::Storage,
    _marker: PhantomData<&'ctx ()>,
}

impl<'ctx, K, Tail> StorageLifetime<'ctx> for Node<'ctx, K, Tail>
where
    K: ResourceKind,
    Tail: HandleSpecList + BagStorage<'ctx>,
{
}

impl<'ctx, K, Tail> BagStorage<'ctx> for Cons<K, Tail>
where
    K: ResourceKind,
    Tail: HandleSpecList + BagStorage<'ctx>,
{
    type Storage = Node<'ctx, K, Tail>;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::mint::{
        CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TAG_LEN, CAP_TOKEN_LEN, CapHeader, CapShot,
        ControlResourceKind,
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
    fn empty_bag_is_zst() {
        assert_eq!(core::mem::size_of::<HandleBag<Nil>>(), 0);
    }

    #[test]
    fn single_handle_roundtrip() {
        let handle = LoopDecisionHandle {
            sid: 7,
            lane: 3,
            scope: ScopeId::route(1),
        };
        let bytes = make_test_bytes(&handle);
        let token = CapFrameToken::<LoopContinueKind>::new(&bytes);
        let bag = HandleBagSingle::from_frame(HandleBag::new(), token);

        let res = bag.with_token(|token, tail| {
            let view = token.as_view().expect("token must decode");
            assert_eq!(view.handle(), &handle);
            assert_eq!(core::mem::size_of_val(&tail), 0);
            11
        });
        assert_eq!(res, 11);
    }

    #[test]
    fn chained_handles_are_affine() {
        type Spec = Cons<LoopContinueKind, Cons<LoopContinueKind, Nil>>;

        let h1 = LoopDecisionHandle {
            sid: 10,
            lane: 1,
            scope: ScopeId::route(2),
        };
        let h2 = LoopDecisionHandle {
            sid: 11,
            lane: 2,
            scope: ScopeId::loop_scope(3),
        };
        let bytes1 = make_test_bytes(&h1);
        let bytes2 = make_test_bytes(&h2);
        let token1 = CapFrameToken::<LoopContinueKind>::new(&bytes1);
        let token2 = CapFrameToken::<LoopContinueKind>::new(&bytes2);

        let tail = HandleBagSingle::from_frame(HandleBag::<Nil>::new(), token1);
        let bag = HandleBag::<Spec>::from_frame(tail, token2);

        bag.with_token(|token2, tail_after| {
            let view2 = token2.as_view().expect("token2 must decode");
            assert_eq!(view2.handle(), &h2);
            tail_after.with_token(|token1, final_tail| {
                let view1 = token1.as_view().expect("token1 must decode");
                assert_eq!(view1.handle(), &h1);
                assert_eq!(core::mem::size_of_val(&final_tail), 0);
            });
        });
    }

    // Helper type to construct single-element bags in tests
    type HandleBagSingle<'ctx, K> = HandleBag<'ctx, Cons<K, Nil>>;
}
