use core::marker::PhantomData;

/// Private marker used to keep runtime owners local-only.
///
/// `hibana`'s runtime is intentionally single-core, non-reentrant, and not
/// ISR-safe. Embedding owners carry this field so they cannot implement
/// `Send`/`Sync` accidentally.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LocalOnly(PhantomData<*mut ()>);

impl LocalOnly {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self(PhantomData)
    }
}
