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

/// Encoded scope identifier carried by lowering, route tables, resolver sites,
/// and endpoint evidence.
///
/// `u32::MAX` is the absent sentinel. Present scopes use a Pico-sized packed
/// layout: `kind:2 | nest:9 | range:9 | local:12`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeId {
    raw: u32,
}

impl ScopeId {
    const ABSENT_RAW: u32 = u32::MAX;
    const LOCAL_SHIFT: u32 = 0;
    const RANGE_SHIFT: u32 = 12;
    const NEST_SHIFT: u32 = 21;
    const KIND_SHIFT: u32 = 30;
    const KIND_MASK: u32 = 0b11;
    const LOCAL_MASK: u32 = 0x0fff;
    const RANGE_MASK: u32 = 0x01ff;
    const NEST_MASK: u32 = 0x01ff;

    pub(crate) const MAX_LOCAL_ORDINAL: u16 = Self::LOCAL_MASK as u16;
    pub(crate) const LOCAL_CAPACITY: u16 = Self::MAX_LOCAL_ORDINAL + 1;

    pub(crate) const fn new(kind: ScopeKind, local: u16) -> Self {
        Self::new_with_parts(kind, local, 0, 0)
    }

    pub(crate) const fn new_with_parts(kind: ScopeKind, local: u16, range: u16, nest: u16) -> Self {
        if local as u32 > Self::LOCAL_MASK
            || range as u32 > Self::RANGE_MASK
            || nest as u32 > Self::NEST_MASK
        {
            panic!("scope ordinal overflow");
        }
        let raw = ((kind as u32) << Self::KIND_SHIFT)
            | ((nest as u32) << Self::NEST_SHIFT)
            | ((range as u32) << Self::RANGE_SHIFT)
            | ((local as u32) << Self::LOCAL_SHIFT);
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

    pub(crate) const fn raw(self) -> u32 {
        self.raw
    }

    pub(crate) const fn same(self, other: Self) -> bool {
        self.raw == other.raw
    }

    pub(crate) const fn from_raw(raw: u32) -> Self {
        if raw == Self::ABSENT_RAW {
            return Self::none();
        }
        if ((raw >> Self::KIND_SHIFT) & Self::KIND_MASK) > ScopeKind::Parallel as u32 {
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
        (self.raw & Self::LOCAL_MASK) as u16
    }

    pub(crate) const fn range_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::RANGE_SHIFT) & Self::RANGE_MASK) as u16
    }

    pub(crate) const fn nest_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::NEST_SHIFT) & Self::NEST_MASK) as u16
    }

    pub(crate) const fn add_ordinal(self, delta: u16) -> Self {
        if self.is_none() {
            return Self::none();
        }
        let ordinal = self.local_ordinal();
        let sum = ordinal as u32 + delta as u32;
        if sum > Self::LOCAL_MASK {
            panic!("scope ordinal overflow");
        }
        Self::new_with_parts(
            self.kind(),
            sum as u16,
            self.range_ordinal(),
            self.nest_ordinal(),
        )
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
    fn scope_id_is_u32_sentinel_identity() {
        assert_eq!(core::mem::size_of::<ScopeId>(), 4);
        assert!(ScopeId::none().is_none());

        let route = ScopeId::route(ScopeId::MAX_LOCAL_ORDINAL);
        assert_eq!(route.kind(), ScopeKind::Route);
        assert_eq!(route.local_ordinal(), 0x0fff);

        let parallel = ScopeId::parallel(3071);
        assert_eq!(parallel.kind(), ScopeKind::Parallel);
        assert_eq!(parallel.local_ordinal(), 3071);
        assert!(ScopeId::new(ScopeKind::Plain, 0).same(ScopeId::from_raw(0)));

        let max = ScopeId::new_with_parts(ScopeKind::Route, 0x0fff, 0x01ff, 0x01ff);
        assert_eq!(max.kind(), ScopeKind::Route);
        assert_eq!(max.local_ordinal(), 0x0fff);
        assert_eq!(max.range_ordinal(), 0x01ff);
        assert_eq!(max.nest_ordinal(), 0x01ff);
        assert!(max.same(ScopeId::from_raw(max.raw())));
        assert!(ScopeId::LOCAL_CAPACITY as usize >= crate::eff::meta::MAX_EFF_NODES);
    }
}
