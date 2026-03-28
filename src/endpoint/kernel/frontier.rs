//! Frontier-selection helpers for `offer()`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrontierKind {
    Route,
    Loop,
    Parallel,
    PassiveObserver,
}

impl FrontierKind {
    #[inline]
    pub(super) const fn as_audit_tag(self) -> u8 {
        match self {
            Self::Route => 1,
            Self::Loop => 2,
            Self::Parallel => 3,
            Self::PassiveObserver => 4,
        }
    }

    #[inline]
    pub(super) const fn bit(self) -> u8 {
        match self {
            Self::Route => 1 << 0,
            Self::Loop => 1 << 1,
            Self::Parallel => 1 << 2,
            Self::PassiveObserver => 1 << 3,
        }
    }
}
