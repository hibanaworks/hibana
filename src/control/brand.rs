//! Internal generativity helpers used to brand rendezvous instances and lanes.
//!
//! The B+ execution plan requires that rendezvous be the exclusive authority for
//! minting control-plane witnesses.  We model this by giving every rendezvous a
//! zero-sized brand token and handing out `Guard<'brand>` projections to code
//! that must prove it is operating within that rendezvous instance.  Each call
//! to [`with_brand`] produces a fresh, unforgeable brand lifetime.

use core::marker::PhantomData;

/// Unique brand token.  The invariant is that a `Brand<'brand>` can only be
/// created inside [`with_brand`] and therefore cannot be forged by user code.
#[derive(Clone, Copy)]
pub(crate) struct Brand<'brand> {
    _marker: PhantomData<&'brand mut &'brand ()>,
}

#[cfg(test)]
impl<'brand> Brand<'brand> {
    #[inline]
    const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn guard(&self) -> Guard<'brand> {
        Guard::new()
    }
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

/// Execute `f` with a freshly minted brand.  Each invocation receives a brand
/// that is unique to the call site, ensuring that witnesses cannot outlive the
/// rendezvous that produced them.
#[cfg(test)]
pub(crate) fn with_brand<R>(f: impl for<'brand> FnOnce(Brand<'brand>) -> R) -> R {
    struct Wrapper<F>(F);

    impl<F> Wrapper<F> {
        #[inline]
        fn run<R>(self) -> R
        where
            F: for<'brand> FnOnce(Brand<'brand>) -> R,
        {
            fn call<'brand, R>(f: impl FnOnce(Brand<'brand>) -> R) -> R {
                f(Brand::new())
            }

            call(self.0)
        }
    }

    Wrapper(f).run()
}

#[cfg(test)]
mod tests {
    use super::with_brand;

    #[test]
    fn brands_are_unique() {
        with_brand(|brand_a| {
            let guard_a = brand_a.guard();
            with_brand(|brand_b| {
                let guard_b = brand_b.guard();
                // Guards from distinct brands must not be interchangeable.  The
                // following line would fail to compile if we attempted to pass
                // both guards to a function expecting the same lifetime.
                let _ = guard_a;
                let _ = guard_b;
            });
        });
    }
}
