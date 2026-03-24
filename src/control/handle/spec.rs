//! Compile-time handle specification lists.
//!
//! This module provides zero-cost type-level lists for tracking which
//! capability handles are available at any given program point.
//!
//! # Design Principles
//!
//! 1. **Compile-time only**: All types are ZSTs; no runtime overhead
//! 2. **Affine consumption**: Handles are consumed as they're used
//! 3. **No runtime errors**: Missing handles cause compile errors
//! 4. **No dynamic dispatch**: Every path is monomorphised on K
//!
//! # Type-Level Lists
//!
//! Handle sets are represented as cons-lists:
//! ```ignore
//! type EmptyHandles = Nil;
//! type OneHandle = Cons<LoopContinueKind, Nil>;
//! type TwoHandles = Cons<EndpointResource, Cons<LoopContinueKind, Nil>>;
//! ```
//!
//! These types are derived from typestate at compile time and baked into
//! the endpoint/kernel pipeline.

use crate::control::cap::mint::ResourceKind;
use core::marker::PhantomData;

/// Marker trait for type-level handle specification lists.
///
/// All implementors are zero-sized types (ZSTs) that exist only at
/// compile time. The type system uses these to track which handles
/// are available in a given context.
///
/// # Safety
///
/// This trait is sealed and cannot be implemented outside this module.
/// Only `Nil` and `Cons<K, Tail>` implement this trait.
pub(crate) trait HandleSpecList: private::Sealed {}

/// Empty handle specification list.
///
/// Represents a state where no handles are available.
/// Attempting to access any handle from a `HandleBag<Nil>` will
/// fail at compile time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Nil;

impl HandleSpecList for Nil {}

/// Non-empty handle specification list.
///
/// Represents a cons-cell containing a handle of type `K` and
/// a tail of remaining handles.
///
/// # Type Parameters
///
/// - `K`: The resource kind for the head handle
/// - `Tail`: The rest of the handle list (either `Nil` or another `Cons`)
///
/// # Example
///
/// ```ignore
/// type Handles = Cons<LoopContinueKind, Cons<EndpointResource, Nil>>;
/// //             ^~~~ head              ^~~~ tail
/// ```
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cons<K: ResourceKind, Tail: HandleSpecList> {
    _marker: PhantomData<(K, Tail)>,
}

impl<K: ResourceKind, Tail: HandleSpecList> HandleSpecList for Cons<K, Tail> {}

/// Sealed trait to prevent external implementations.
mod private {
    use super::*;

    pub(crate) trait Sealed {}
    impl Sealed for Nil {}
    impl<K: ResourceKind, Tail: HandleSpecList> Sealed for Cons<K, Tail> {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::mint::EndpointResource;
    use crate::control::cap::resource_kinds::LoopContinueKind;

    #[test]
    fn handle_spec_lists_are_zero_sized() {
        assert_eq!(core::mem::size_of::<Nil>(), 0);
        assert_eq!(core::mem::size_of::<Cons<LoopContinueKind, Nil>>(), 0);
        assert_eq!(
            core::mem::size_of::<Cons<EndpointResource, Cons<LoopContinueKind, Nil>>>(),
            0
        );
    }
}
