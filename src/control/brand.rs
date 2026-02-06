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

impl<'brand> Brand<'brand> {
    #[inline]
    const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }

    /// Reconstruct a `Brand` from a `Guard`.
    ///
    /// **Safety**: This is safe because `Guard` is crate-private and can only
    /// be created from a valid `Brand`.
    #[inline]
    pub(crate) const fn from_guard(_guard: Guard<'brand>) -> Self {
        Self {
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn guard(&self) -> Guard<'brand> {
        Guard {
            _marker: PhantomData,
        }
    }

    /// Execute `f` with a short-lived lane witness (`&'ln Brand<'rv>`).
    ///
    /// This is the **only** way to mint a `LaneKey`, ensuring that lane witnesses
    /// cannot be forged and that lifetime `'ln` (lane) is always shorter than `'rv` (rendezvous).
    ///
    /// # Design Rationale
    ///
    /// The type system enforces `'rv: 'ln` (rendezvous outlives lane) by:
    /// 1. `Brand<'rv>` is owned by the rendezvous and lives for the entire session.
    /// 2. `&'ln Brand<'rv>` is generated transiently inside this higher-ranked closure.
    /// 3. `LaneKey<'rv>` carries only `'rv`, never the short-lived `'ln`.
    ///
    /// As a result `Forward` and `Owner` can be stored without the lane lifetime,
    /// addressing the classic E0521 borrow checker issue when returning values
    /// from closures.
    #[inline]
    pub(crate) fn with_lane<R>(
        &self,
        lane: crate::rendezvous::Lane,
        f: impl for<'ln> FnOnce(&'ln Brand<'brand>, crate::control::cap::LaneKey<'brand>) -> R,
    ) -> R {
        // Short-lived witness: &'ln Brand<'brand>
        // Note: 'ln is fresh for each invocation, ensuring it cannot escape
        fn call_inner<'brand, 'ln, R>(
            brand: &'ln Brand<'brand>,
            lane: crate::rendezvous::Lane,
            f: impl FnOnce(&'ln Brand<'brand>, crate::control::cap::LaneKey<'brand>) -> R,
        ) -> R {
            let lane_key = crate::control::cap::LaneKey::new(brand.guard(), lane);
            f(brand, lane_key)
        }

        call_inner(self, lane, f)
    }
}

/// Lightweight projection of a brand that can be stored inside data
/// structures without exposing the brand type itself.
#[derive(Clone, Copy)]
pub(crate) struct Guard<'brand> {
    _marker: PhantomData<&'brand mut &'brand ()>,
}

/// Execute `f` with a freshly minted brand.  Each invocation receives a brand
/// that is unique to the call site, ensuring that witnesses cannot outlive the
/// rendezvous that produced them.
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
