use super::{ScopeId, StateIndex, TryFrom};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrontierKind {
    Route,
    Reentry,
    Parallel,
    PassiveObserver,
}

impl FrontierKind {
    #[inline]
    pub(crate) const fn as_audit_tag(self) -> u8 {
        match self {
            Self::Route => 1,
            Self::Reentry => 2,
            Self::Parallel => 3,
            Self::PassiveObserver => 4,
        }
    }

    #[inline]
    pub(crate) const fn bit(self) -> u8 {
        match self {
            Self::Route => 1 << 0,
            Self::Reentry => 1 << 1,
            Self::Parallel => 1 << 2,
            Self::PassiveObserver => 1 << 3,
        }
    }
}

#[inline]
pub(crate) fn checked_state_index(idx: usize) -> Option<StateIndex> {
    u16::try_from(idx).ok().map(StateIndex::new)
}

#[derive(Clone, Copy)]
pub(crate) struct LaneOfferState {
    pub(crate) scope: ScopeId,
    pub(crate) entry: StateIndex,
    pub(crate) parallel_root: ScopeId,
    pub(crate) frontier: FrontierKind,
    pub(crate) flags: u8,
}

impl LaneOfferState {
    pub(crate) const FLAG_CONTROLLER: u8 = 1;
    pub(crate) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(crate) const FLAG_INTRINSIC_READY: u8 = 1 << 2;
    pub(crate) const EMPTY: Self = Self {
        scope: ScopeId::none(),
        entry: StateIndex::ABSENT,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        flags: 0,
    };

    #[inline]
    pub(crate) fn is_controller(self) -> bool {
        (self.flags & Self::FLAG_CONTROLLER) != 0
    }

    #[inline]
    pub(crate) fn is_dynamic(self) -> bool {
        (self.flags & Self::FLAG_DYNAMIC) != 0
    }

    #[inline]
    pub(crate) fn intrinsic_ready(self) -> bool {
        (self.flags & Self::FLAG_INTRINSIC_READY) != 0
    }
}
