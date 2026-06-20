/// Structured scope taxonomy used by the global DSL to tag composite
/// fragments such as routes, rolled reentry regions, or parallel lanes.
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeKind {
    /// Scope representing a routing decision (`g::route`).
    Route = 0,
    /// Scope representing a rolled reentry region (`Program::roll`).
    Roll = 1,
    /// Scope representing a parallel lane (`g::par`).
    Parallel = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeEvent {
    Enter,
    Split,
    Exit,
}

/// Encoded scope identifier carried by lowering, route tables, resolver sites,
/// and endpoint evidence.
///
/// `u16::MAX` is the absent sentinel. Present scopes use the packed layout
/// `reserved:1 | kind:2 | local:13`; the reserved bit is always zero.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeId(u16);

impl ScopeId {
    const ABSENT_RAW: u16 = u16::MAX;
    const RESERVED_BIT: u16 = 0x8000;
    const KIND_SHIFT: u16 = 13;
    const KIND_MASK: u16 = 0b11;
    const LOCAL_MASK: u16 = 0x1fff;

    pub(crate) const MAX_LOCAL_ORDINAL: u16 = Self::LOCAL_MASK;
    pub(crate) const LOCAL_CAPACITY: u16 = Self::MAX_LOCAL_ORDINAL + 1;

    pub(crate) const fn new(kind: ScopeKind, local: u16) -> Self {
        if local > Self::LOCAL_MASK {
            panic!("scope ordinal overflow");
        }
        let raw = ((kind as u16) << Self::KIND_SHIFT) | local;
        if (raw & Self::RESERVED_BIT) != 0 {
            panic!("scope reserved bit set");
        }
        Self(raw)
    }

    pub(crate) const fn none() -> Self {
        Self(Self::ABSENT_RAW)
    }

    pub(crate) const fn is_none(self) -> bool {
        self.0 == Self::ABSENT_RAW
    }

    pub(crate) const fn raw(self) -> u16 {
        self.0
    }

    pub(crate) const fn same(self, other: Self) -> bool {
        self.0 == other.0
    }

    pub(crate) const fn from_raw(raw: u16) -> Self {
        if raw == Self::ABSENT_RAW {
            return Self::none();
        }
        if (raw & Self::RESERVED_BIT) != 0 {
            crate::invariant();
        }
        match (raw >> Self::KIND_SHIFT) & Self::KIND_MASK {
            0..=2 => Self(raw),
            _ => crate::invariant(),
        }
    }

    pub(crate) const fn kind(self) -> Option<ScopeKind> {
        if self.is_none() {
            return None;
        }
        match (self.0 >> Self::KIND_SHIFT) & Self::KIND_MASK {
            0 => Some(ScopeKind::Route),
            1 => Some(ScopeKind::Roll),
            2 => Some(ScopeKind::Parallel),
            _ => crate::invariant(),
        }
    }

    pub(crate) const fn local_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        self.0 & Self::LOCAL_MASK
    }

    pub(crate) const fn add_ordinal(self, delta: u16) -> Self {
        if self.is_none() {
            return Self::none();
        }
        let ordinal = self.local_ordinal();
        let sum = ordinal as u32 + delta as u32;
        if sum > Self::LOCAL_MASK as u32 {
            panic!("scope ordinal overflow");
        }
        match self.kind() {
            Some(kind) => Self::new(kind, sum as u16),
            None => Self::none(),
        }
    }

    pub(crate) const fn route(ordinal: u16) -> Self {
        Self::new(ScopeKind::Route, ordinal)
    }

    pub(crate) const fn roll_scope(ordinal: u16) -> Self {
        Self::new(ScopeKind::Roll, ordinal)
    }

    pub(crate) const fn parallel(ordinal: u16) -> Self {
        Self::new(ScopeKind::Parallel, ordinal)
    }
}

#[cfg(test)]
mod tests {
    use super::{ScopeId, ScopeKind};

    #[test]
    fn scope_id_is_u16_sentinel_identity() {
        assert_eq!(core::mem::size_of::<ScopeId>(), 2);
        assert!(ScopeId::none().is_none());
        assert_eq!(ScopeId::none().kind(), None);

        let route = ScopeId::route(ScopeId::MAX_LOCAL_ORDINAL);
        assert_eq!(route.kind(), Some(ScopeKind::Route));
        assert_eq!(route.local_ordinal(), 0x1fff);

        let roll = ScopeId::roll_scope(0x1ffe);
        assert_eq!(roll.kind(), Some(ScopeKind::Roll));
        assert_eq!(roll.local_ordinal(), 0x1ffe);

        let parallel = ScopeId::parallel(3071);
        assert_eq!(parallel.kind(), Some(ScopeKind::Parallel));
        assert_eq!(parallel.local_ordinal(), 3071);

        assert!(ScopeId::from_raw(route.raw()).same(route));
        assert!(ScopeId::from_raw(roll.raw()).same(roll));
        assert!(ScopeId::from_raw(parallel.raw()).same(parallel));
        assert!(ScopeId::LOCAL_CAPACITY as usize >= crate::eff::meta::MAX_EFF_NODES);
    }
}
