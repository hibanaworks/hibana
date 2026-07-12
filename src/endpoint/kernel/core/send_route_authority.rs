#[derive(Clone, Copy)]
pub(crate) enum SendRouteAudit {
    None,
    DirectPreview { start: u16 },
}

#[derive(Clone, Copy)]
pub(crate) enum SendRouteAuthority {
    None,
    Direct { lane: u8, audit_start: u16 },
    MaterializedBranch,
}

impl SendRouteAuthority {
    #[inline]
    pub(crate) const fn none() -> Self {
        Self::None
    }

    #[inline]
    pub(crate) const fn direct(lane: u8, audit_start: u16) -> Self {
        Self::Direct { lane, audit_start }
    }

    #[inline]
    pub(crate) const fn materialized_branch() -> Self {
        Self::MaterializedBranch
    }

    #[inline]
    pub(crate) const fn route_audit(self) -> SendRouteAudit {
        match self {
            Self::Direct {
                audit_start,
                lane: _,
            } => SendRouteAudit::DirectPreview { start: audit_start },
            Self::None | Self::MaterializedBranch => SendRouteAudit::None,
        }
    }
}

impl SendRouteAudit {
    #[inline]
    pub(crate) const fn fresh_route_start(self) -> Option<usize> {
        match self {
            Self::DirectPreview { start } => Some(start as usize),
            Self::None => None,
        }
    }
}
