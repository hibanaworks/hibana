pub(crate) mod meta {
    /// Maximum number of effect nodes the const DSL may emit.
    pub(crate) const MAX_EFF_NODES: usize = 256;
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EffIndex(u16);

impl EffIndex {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(u16::MAX);

    #[inline(always)]
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub const fn from_usize(idx: usize) -> Self {
        if idx > (u16::MAX as usize) {
            panic!("eff index overflow");
        }
        Self(idx as u16)
    }

    #[inline(always)]
    pub const fn raw(self) -> u16 {
        self.0
    }

    #[inline(always)]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl core::fmt::Display for EffIndex {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EffSlice(&'static [EffStruct]);

impl EffSlice {
    #[inline(always)]
    pub const fn new(slice: &'static [EffStruct]) -> Self {
        Self(slice)
    }

    #[inline(always)]
    pub const fn len(self) -> usize {
        self.0.len()
    }

    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0.is_empty()
    }

    #[inline(always)]
    pub const fn at(self, idx: usize) -> EffStruct {
        self.0[idx]
    }

    #[inline(always)]
    pub const fn as_slice(self) -> &'static [EffStruct] {
        self.0
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffKind {
    Pure = 0,
    Seq = 1,
    Par = 2,
    Alt = 3,
    Atom = 4,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffChildren {
    pub a: EffIndex,
    pub b: EffIndex,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffAtom {
    pub from: u8,
    pub to: u8,
    pub label: u8,
    pub is_control: bool,
    pub resource: Option<u8>,
    pub direction: EffDirection,
    /// Type-level lane for parallel composition (default 0).
    pub lane: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union EffData {
    pub children: EffChildren,
    pub atom: EffAtom,
    pub empty: (),
}

impl EffData {
    pub const fn empty() -> Self {
        Self { empty: () }
    }

    pub const fn from_children(children: EffChildren) -> Self {
        Self { children }
    }

    pub const fn from_atom(atom: EffAtom) -> Self {
        Self { atom }
    }

    #[inline(always)]
    pub const fn children(&self) -> EffChildren {
        unsafe { self.children }
    }

    #[inline(always)]
    pub const fn atom(&self) -> EffAtom {
        unsafe { self.atom }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EffStruct {
    pub kind: EffKind,
    pub data: EffData,
}

impl EffStruct {
    pub const fn pure() -> Self {
        Self {
            kind: EffKind::Pure,
            data: EffData::empty(),
        }
    }

    pub const fn seq(left: EffIndex, right: EffIndex) -> Self {
        Self {
            kind: EffKind::Seq,
            data: EffData::from_children(EffChildren { a: left, b: right }),
        }
    }

    pub const fn par(left: EffIndex, right: EffIndex) -> Self {
        Self {
            kind: EffKind::Par,
            data: EffData::from_children(EffChildren { a: left, b: right }),
        }
    }

    pub const fn alt(left: EffIndex, right: EffIndex) -> Self {
        Self {
            kind: EffKind::Alt,
            data: EffData::from_children(EffChildren { a: left, b: right }),
        }
    }

    pub const fn atom(atom: EffAtom) -> Self {
        Self {
            kind: EffKind::Atom,
            data: EffData::from_atom(atom),
        }
    }

    #[inline(always)]
    pub fn children(&self) -> EffChildren {
        self.data.children()
    }

    #[inline(always)]
    pub const fn atom_data(&self) -> EffAtom {
        self.data.atom()
    }
}

impl core::fmt::Debug for EffStruct {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.kind {
            EffKind::Pure => f.debug_struct("EffStruct::Pure").finish(),
            EffKind::Seq => {
                let children = self.children();
                f.debug_struct("EffStruct::Seq")
                    .field("a", &children.a)
                    .field("b", &children.b)
                    .finish()
            }
            EffKind::Par => {
                let children = self.children();
                f.debug_struct("EffStruct::Par")
                    .field("a", &children.a)
                    .field("b", &children.b)
                    .finish()
            }
            EffKind::Alt => {
                let children = self.children();
                f.debug_struct("EffStruct::Alt")
                    .field("a", &children.a)
                    .field("b", &children.b)
                    .finish()
            }
            EffKind::Atom => f
                .debug_struct("EffStruct::Atom")
                .field("atom", &self.atom_data())
                .finish(),
        }
    }
}

impl PartialEq for EffStruct {
    fn eq(&self, other: &Self) -> bool {
        if self.kind != other.kind {
            return false;
        }
        match self.kind {
            EffKind::Pure => true,
            EffKind::Seq | EffKind::Par | EffKind::Alt => {
                let lhs = self.children();
                let rhs = other.children();
                lhs.a == rhs.a && lhs.b == rhs.b
            }
            EffKind::Atom => self.atom_data() == other.atom_data(),
        }
    }
}

impl Eq for EffStruct {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffDirection {
    Send,
    Recv,
}
