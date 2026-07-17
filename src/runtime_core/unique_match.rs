#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UniqueMatch<T> {
    None,
    One(T),
    Ambiguous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UniqueMatchFailure {
    None,
    Ambiguous,
}

impl<T> UniqueMatch<T>
where
    T: Copy + PartialEq,
{
    pub(crate) const NONE: Self = Self::None;

    #[inline(always)]
    pub(crate) fn add(self, candidate: T) -> Self {
        match self {
            Self::None => Self::One(candidate),
            Self::One(existing) if existing == candidate => self,
            Self::One(_) | Self::Ambiguous => Self::Ambiguous,
        }
    }

    #[inline(always)]
    pub(crate) const fn is_ambiguous(self) -> bool {
        matches!(self, Self::Ambiguous)
    }

    #[inline(always)]
    pub(crate) fn finish(self) -> Result<T, UniqueMatchFailure> {
        match self {
            Self::None => Err(UniqueMatchFailure::None),
            Self::One(candidate) => Ok(candidate),
            Self::Ambiguous => Err(UniqueMatchFailure::Ambiguous),
        }
    }

    #[inline(always)]
    pub(crate) fn into_option(self) -> Option<T> {
        self.finish().ok()
    }
}

#[cfg(kani)]
mod kani;
#[cfg(all(test, hibana_repo_tests))]
mod tests;
