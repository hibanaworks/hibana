use super::{MAX_STATES, ScopeId, StateIndex};
use crate::global::const_dsl::ScopeKind;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrontierKind {
    Route,
    Reentry,
    Parallel,
    PassiveObserver,
}

impl FrontierKind {
    pub(crate) const ALL_BITS: u8 = (1 << 4) - 1;

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
    if idx < MAX_STATES {
        Some(StateIndex::new(idx as u16))
    } else {
        None
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OfferEntryKey {
    scope: ScopeId,
    entry: StateIndex,
}

impl OfferEntryKey {
    pub(crate) const EMPTY: Self = Self {
        scope: ScopeId::none(),
        entry: StateIndex::ABSENT,
    };

    #[inline]
    pub(crate) const fn new(scope: ScopeId, entry: StateIndex) -> Option<Self> {
        if !matches!(scope.kind(), Some(ScopeKind::Route)) || entry.is_absent() {
            None
        } else {
            Some(Self { scope, entry })
        }
    }

    #[inline]
    pub(crate) fn from_index(scope: ScopeId, entry_idx: usize) -> Option<Self> {
        Self::new(scope, checked_state_index(entry_idx)?)
    }

    #[inline]
    pub(crate) const fn is_absent(self) -> bool {
        self.scope.is_none() || self.entry.is_absent()
    }

    #[inline]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope
    }

    #[inline]
    pub(crate) const fn entry(self) -> StateIndex {
        self.entry
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub(crate) const fn key(self) -> Option<OfferEntryKey> {
        OfferEntryKey::new(self.scope, self.entry)
    }

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
