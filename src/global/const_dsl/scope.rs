/// Structured scope taxonomy used by the global DSL to tag composite
/// fragments such as routes, rolled reentry regions, or parallel lanes.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeKind {
    /// Plain source fragment without route, reentry, or parallel semantics.
    Plain = 0,
    /// Scope representing a routing decision (`g::route`).
    Route = 1,
    /// Scope representing a rolled reentry region (`Program::roll`).
    Roll = 2,
    /// Scope representing a parallel lane (`g::par`).
    Parallel = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeEvent {
    Enter,
    Exit,
}

/// Encoded scope identifier embedding the scope kind and a local ordinal.
///
/// `u16::MAX` is the absent sentinel. Present scopes use the high three bits for
/// [`ScopeKind`] and the low thirteen bits for the local ordinal.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeId {
    raw: u16,
}

impl ScopeId {
    const ABSENT_RAW: u16 = u16::MAX;
    const KIND_SHIFT: u16 = 13;
    const KIND_MASK: u16 = 0b111;
    const LOCAL_MASK: u16 = 0x1fff;

    pub(crate) const ORDINAL_CAPACITY: u16 = Self::LOCAL_MASK;

    pub(crate) const fn new(kind: ScopeKind, local: u16) -> Self {
        if local > Self::LOCAL_MASK {
            panic!("scope ordinal overflow");
        }
        let raw = ((kind as u16) << Self::KIND_SHIFT) | local;
        Self { raw }
    }

    pub(crate) const fn none() -> Self {
        Self {
            raw: Self::ABSENT_RAW,
        }
    }

    pub(crate) const fn is_none(self) -> bool {
        self.raw == Self::ABSENT_RAW
    }

    pub(crate) const fn raw(self) -> u16 {
        self.raw
    }

    pub(crate) const fn same(self, other: Self) -> bool {
        self.raw == other.raw
    }

    pub(crate) const fn from_raw(raw: u16) -> Self {
        if raw == Self::ABSENT_RAW {
            return Self::none();
        }
        if ((raw >> Self::KIND_SHIFT) & Self::KIND_MASK) > ScopeKind::Parallel as u16 {
            crate::invariant();
        }
        Self { raw }
    }

    pub(crate) const fn kind(self) -> ScopeKind {
        if self.is_none() {
            return ScopeKind::Plain;
        }
        match ((self.raw >> Self::KIND_SHIFT) & Self::KIND_MASK) as u8 {
            0 => ScopeKind::Plain,
            1 => ScopeKind::Route,
            2 => ScopeKind::Roll,
            3 => ScopeKind::Parallel,
            _ => panic!("invalid scope kind"),
        }
    }

    pub(crate) const fn ordinal(self) -> u16 {
        self.local_ordinal()
    }

    pub(crate) const fn local_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        self.raw & Self::LOCAL_MASK
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
        Self::new(self.kind(), sum as u16)
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
    fn scope_id_is_two_byte_sentinel_identity() {
        assert_eq!(core::mem::size_of::<ScopeId>(), 2);
        assert!(ScopeId::none().is_none());

        let route = ScopeId::route(ScopeId::ORDINAL_CAPACITY);
        assert_eq!(route.kind(), ScopeKind::Route);
        assert_eq!(route.local_ordinal(), 0x1fff);

        let parallel = ScopeId::parallel(3071);
        assert_eq!(parallel.kind(), ScopeKind::Parallel);
        assert_eq!(parallel.local_ordinal(), 3071);
        assert!(ScopeId::new(ScopeKind::Plain, 0).same(ScopeId::from_raw(0)));
    }
}
