use super::{ScopeId, StateIndex, TryFrom};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrontierKind {
    Route,
    Loop,
    Parallel,
    PassiveObserver,
}

impl FrontierKind {
    #[inline]
    pub(crate) const fn as_audit_tag(self) -> u8 {
        match self {
            Self::Route => 1,
            Self::Loop => 2,
            Self::Parallel => 3,
            Self::PassiveObserver => 4,
        }
    }

    #[inline]
    pub(crate) const fn bit(self) -> u8 {
        match self {
            Self::Route => 1 << 0,
            Self::Loop => 1 << 1,
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
    pub(crate) static_ready: bool,
    pub(crate) flags: u8,
}

impl LaneOfferState {
    pub(crate) const FLAG_CONTROLLER: u8 = 1;
    pub(crate) const FLAG_DYNAMIC: u8 = 1 << 1;
    pub(crate) const EMPTY: Self = Self {
        scope: ScopeId::none(),
        entry: StateIndex::MAX,
        parallel_root: ScopeId::none(),
        frontier: FrontierKind::Route,
        static_ready: false,
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
    pub(crate) fn static_ready(self) -> bool {
        self.static_ready
    }
}
