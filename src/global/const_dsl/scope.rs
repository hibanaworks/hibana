/// Structured scope taxonomy used by the global DSL to tag composite
/// fragments such as routes, loops, or parallel lanes.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeKind {
    /// Default scope kind when no specialised semantics are required.
    Generic = 0,
    /// Scope representing a routing decision (`g::route`).
    Route = 1,
    /// Scope representing a loop fixpoint.
    Loop = 2,
    /// Scope representing a parallel lane (`g::par`).
    Parallel = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeEvent {
    Enter,
    Exit,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlScopeKind {
    None = 0,
    Loop = 1,
    State = 2,
    Abort = 3,
    Topology = 4,
    Policy = 6,
    Route = 7,
}

impl ControlScopeKind {
    #[inline]
    pub(crate) const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Loop),
            2 => Some(Self::State),
            3 => Some(Self::Abort),
            4 => Some(Self::Topology),
            6 => Some(Self::Policy),
            7 => Some(Self::Route),
            _ => None,
        }
    }
}

/// Encoded scope identifier embedding the scope kind and its structural ordinals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeId {
    raw: u64,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompactScopeId {
    raw: u32,
}

impl Default for ScopeId {
    fn default() -> Self {
        ScopeId::none()
    }
}

impl ScopeId {
    const NONE_RAW: u64 = u64::MAX;
    const KIND_BITS: u64 = 3;
    const LOCAL_BITS: u64 = 13;
    const RANGE_BITS: u64 = 16;
    const NEST_BITS: u64 = 16;

    const NEST_SHIFT: u64 = 0;
    const RANGE_SHIFT: u64 = Self::NEST_SHIFT + Self::NEST_BITS;
    const LOCAL_SHIFT: u64 = Self::RANGE_SHIFT + Self::RANGE_BITS;
    const KIND_SHIFT: u64 = Self::LOCAL_SHIFT + Self::LOCAL_BITS;

    const KIND_MASK: u64 = (1 << Self::KIND_BITS) - 1;
    const LOCAL_MASK: u64 = (1 << Self::LOCAL_BITS) - 1;
    const RANGE_MASK: u64 = (1 << Self::RANGE_BITS) - 1;
    const NEST_MASK: u64 = (1 << Self::NEST_BITS) - 1;

    pub(crate) const ORDINAL_CAPACITY: u16 = Self::LOCAL_MASK as u16;

    pub(crate) const fn compose(kind: ScopeKind, local: u16, range: u16, nest: u16) -> Self {
        if local as u64 > Self::LOCAL_MASK
            || range as u64 > Self::RANGE_MASK
            || nest as u64 > Self::NEST_MASK
        {
            panic!("scope ordinal overflow");
        }
        let raw = ((kind as u64) << Self::KIND_SHIFT)
            | ((local as u64) << Self::LOCAL_SHIFT)
            | ((range as u64) << Self::RANGE_SHIFT)
            | ((nest as u64) << Self::NEST_SHIFT);
        Self { raw }
    }

    pub(crate) const fn new(kind: ScopeKind, local: u16) -> Self {
        Self::compose(kind, local, 0, 0)
    }

    pub(crate) const fn none() -> Self {
        Self {
            raw: Self::NONE_RAW,
        }
    }

    pub(crate) const fn is_none(self) -> bool {
        self.raw == Self::NONE_RAW
    }

    pub(crate) const fn decode_raw(raw: u64) -> Option<Self> {
        if raw == Self::NONE_RAW {
            return Some(Self::none());
        }
        if ((raw >> Self::KIND_SHIFT) & Self::KIND_MASK) > ScopeKind::Parallel as u64 {
            None
        } else {
            Some(Self { raw })
        }
    }

    pub(crate) const fn raw(self) -> u64 {
        self.raw
    }

    pub(crate) const fn kind(self) -> ScopeKind {
        if self.is_none() {
            return ScopeKind::Generic;
        }
        match ((self.raw >> Self::KIND_SHIFT) & Self::KIND_MASK) as u8 {
            0 => ScopeKind::Generic,
            1 => ScopeKind::Route,
            2 => ScopeKind::Loop,
            3 => ScopeKind::Parallel,
            _ => ScopeKind::Generic,
        }
    }

    pub(crate) const fn ordinal(self) -> u16 {
        self.local_ordinal()
    }

    pub(crate) const fn local_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::LOCAL_SHIFT) & Self::LOCAL_MASK) as u16
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

    pub(crate) const fn canonical(self) -> Self {
        if self.is_none() {
            return Self::none();
        }
        Self::compose(self.kind(), self.local_ordinal(), 0, 0)
    }

    pub(crate) const fn canonical_raw(self) -> u64 {
        if self.is_none() {
            Self::NONE_RAW
        } else {
            let variable_mask =
                (Self::RANGE_MASK << Self::RANGE_SHIFT) | (Self::NEST_MASK << Self::NEST_SHIFT);
            self.raw & !variable_mask
        }
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
        Self::compose(
            self.kind(),
            sum as u16,
            self.range_ordinal(),
            self.nest_ordinal(),
        )
    }

    pub(crate) const fn route(ordinal: u16) -> Self {
        Self::new(ScopeKind::Route, ordinal)
    }

    pub(crate) const fn parallel(ordinal: u16) -> Self {
        Self::new(ScopeKind::Parallel, ordinal)
    }
}

impl CompactScopeId {
    const NONE_RAW: u32 = u32::MAX;
    const KIND_BITS: u32 = 3;
    const ORDINAL_BITS: u32 = 9;

    const NEST_SHIFT: u32 = 0;
    const RANGE_SHIFT: u32 = Self::NEST_SHIFT + Self::ORDINAL_BITS;
    const LOCAL_SHIFT: u32 = Self::RANGE_SHIFT + Self::ORDINAL_BITS;
    const KIND_SHIFT: u32 = Self::LOCAL_SHIFT + Self::ORDINAL_BITS;

    const KIND_MASK: u32 = (1 << Self::KIND_BITS) - 1;
    const ORDINAL_MASK: u32 = (1 << Self::ORDINAL_BITS) - 1;

    pub(crate) const fn none() -> Self {
        Self {
            raw: Self::NONE_RAW,
        }
    }

    pub(crate) const fn decode_raw(raw: u32) -> Option<Self> {
        if raw == Self::NONE_RAW {
            return Some(Self::none());
        }
        if ((raw >> Self::KIND_SHIFT) & Self::KIND_MASK) > ScopeKind::Parallel as u32 {
            None
        } else {
            Some(Self { raw })
        }
    }

    pub(crate) const fn raw(self) -> u32 {
        self.raw
    }

    pub(crate) const fn is_none(self) -> bool {
        self.raw == Self::NONE_RAW
    }

    pub(crate) const fn from_scope_id(scope: ScopeId) -> Self {
        if scope.is_none() {
            return Self::none();
        }
        let local = scope.local_ordinal() as u32;
        let range = scope.range_ordinal() as u32;
        let nest = scope.nest_ordinal() as u32;
        if local > Self::ORDINAL_MASK || range > Self::ORDINAL_MASK || nest > Self::ORDINAL_MASK {
            panic!("scope ordinal overflow");
        }
        Self {
            raw: ((scope.kind() as u32) << Self::KIND_SHIFT)
                | (local << Self::LOCAL_SHIFT)
                | (range << Self::RANGE_SHIFT)
                | (nest << Self::NEST_SHIFT),
        }
    }

    pub(crate) const fn to_scope_id(self) -> ScopeId {
        if self.is_none() {
            ScopeId::none()
        } else {
            ScopeId::compose(
                self.kind(),
                self.local_ordinal(),
                self.range_ordinal(),
                self.nest_ordinal(),
            )
        }
    }

    pub(crate) const fn kind(self) -> ScopeKind {
        if self.is_none() {
            return ScopeKind::Generic;
        }
        match ((self.raw >> Self::KIND_SHIFT) & Self::KIND_MASK) as u8 {
            0 => ScopeKind::Generic,
            1 => ScopeKind::Route,
            2 => ScopeKind::Loop,
            3 => ScopeKind::Parallel,
            _ => ScopeKind::Generic,
        }
    }

    pub(crate) const fn local_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::LOCAL_SHIFT) & Self::ORDINAL_MASK) as u16
    }

    pub(crate) const fn range_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::RANGE_SHIFT) & Self::ORDINAL_MASK) as u16
    }

    pub(crate) const fn nest_ordinal(self) -> u16 {
        if self.is_none() {
            return 0;
        }
        ((self.raw >> Self::NEST_SHIFT) & Self::ORDINAL_MASK) as u16
    }
}
