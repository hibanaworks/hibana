#[derive(Clone, Copy)]
pub(crate) enum SendRouteAuthority {
    None,
    Direct { lane: u8 },
    MaterializedBranch,
}

impl SendRouteAuthority {
    #[inline]
    pub(crate) const fn none() -> Self {
        Self::None
    }

    #[inline]
    pub(crate) const fn direct(lane: u8) -> Self {
        Self::Direct { lane }
    }

    #[inline]
    pub(crate) const fn materialized_branch() -> Self {
        Self::MaterializedBranch
    }
}
