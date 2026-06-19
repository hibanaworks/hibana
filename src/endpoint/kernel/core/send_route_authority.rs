use super::SelectedRouteCommitRowsRef;

#[derive(Clone, Copy)]
pub(crate) enum SendRouteAudit {
    None,
    DirectPreview,
}

#[derive(Clone, Copy)]
pub(crate) enum SendRouteAuthority {
    None,
    Direct {
        selected_routes: SelectedRouteCommitRowsRef,
        lane: u8,
    },
    MaterializedBranch,
}

impl SendRouteAuthority {
    #[inline]
    pub(crate) const fn none() -> Self {
        Self::None
    }

    #[inline]
    pub(crate) const fn direct(selected_routes: SelectedRouteCommitRowsRef, lane: u8) -> Self {
        if selected_routes.is_empty() {
            Self::None
        } else {
            Self::Direct {
                selected_routes,
                lane,
            }
        }
    }

    #[inline]
    pub(crate) const fn materialized_branch() -> Self {
        Self::MaterializedBranch
    }

    #[inline]
    pub(crate) const fn route_audit(self) -> SendRouteAudit {
        match self {
            Self::Direct { .. } => SendRouteAudit::DirectPreview,
            Self::None | Self::MaterializedBranch => SendRouteAudit::None,
        }
    }
}
