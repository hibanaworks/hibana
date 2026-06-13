//! Internal generativity helpers used to brand rendezvous instances and lanes.
//!
//! Rendezvous is the exclusive authority for issuing ownership witnesses.
//! We model this by giving every rendezvous a zero-sized brand token and handing
//! out `Guard<'brand>` projections to code that must prove it is operating
//! within that rendezvous instance. The rendezvous owner is the only runtime
//! path that issues these witnesses.

use core::marker::PhantomData;

/// Unique brand token carried by a rendezvous owner.
#[derive(Clone, Copy)]
pub(crate) struct Brand<'brand> {
    _marker: PhantomData<&'brand mut &'brand ()>,
}

/// Lightweight projection of a brand that can be stored inside data
/// structures without exposing the brand type itself.
#[derive(Clone, Copy)]
pub(crate) struct Guard<'brand> {
    _marker: PhantomData<&'brand mut &'brand ()>,
}

impl<'brand> Guard<'brand> {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Owner<'brand> {
    _brand: PhantomData<Guard<'brand>>,
}

impl<'brand> Owner<'brand> {
    #[inline]
    pub(crate) fn new(_brand: Guard<'brand>) -> Self {
        Self {
            _brand: PhantomData,
        }
    }
}
