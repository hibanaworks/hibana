mod definition;
use definition::define_public_operation_kernel;

define_public_operation_kernel! {
    phases {
        Idle,
        Poisoned,
        Send,
        Recv,
        Offer,
        RouteBranch,
        RestoredRouteBranch,
        BranchRecv,
        BranchSend,
    }
    edges {
        BeginOffer => (Idle, Offer),
        ResumeOffer => (RestoredRouteBranch, Offer),
        PublishRouteBranch => (Offer, RouteBranch),
        FinishOffer => (Offer, Idle),
        ParkOffer => (Offer, RestoredRouteBranch),
        ParkRouteBranch => (RouteBranch, RestoredRouteBranch),
        BeginSend => (Idle, Send),
        BeginBranchSend => (RouteBranch, BranchSend),
        FinishSend => (Send, Idle),
        FinishBranchSend => (BranchSend, Idle),
        ParkBranchSend => (BranchSend, RestoredRouteBranch),
        BeginRecv => (Idle, Recv),
        BeginBranchRecv => (RouteBranch, BranchRecv),
        FinishRecv => (Recv, Idle),
        FinishBranchRecv => (BranchRecv, Idle),
        ParkBranchRecv => (BranchRecv, RestoredRouteBranch),
    }
}

impl PublicActiveOp {
    #[inline(always)]
    pub(in crate::endpoint::kernel) fn transition(self, edge: PublicOpEdge) -> PublicOpTransition {
        if self == Self::Poisoned {
            PublicOpTransition::new(PublicOpLease::Faulted, Self::Poisoned)
        } else if self == edge.expected() {
            PublicOpTransition::new(PublicOpLease::Held, edge.next())
        } else {
            PublicOpTransition::new(PublicOpLease::Rejected, Self::Poisoned)
        }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn clear_if_current(self, expected: Self) -> Self {
        if self == expected { Self::Idle } else { self }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn clear_terminal(self) -> Self {
        Self::Idle
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn fault(self) -> Self {
        Self::Poisoned
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublicOpLease {
    Rejected = 0,
    Held = 1,
    Faulted = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint::kernel) struct PublicOpTransition {
    lease: PublicOpLease,
    phase: PublicActiveOp,
}

impl PublicOpTransition {
    #[inline(always)]
    const fn new(lease: PublicOpLease, phase: PublicActiveOp) -> Self {
        Self { lease, phase }
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn lease(self) -> PublicOpLease {
        self.lease
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn phase(self) -> PublicActiveOp {
        self.phase
    }
}

#[cfg(kani)]
mod kani;

#[cfg(all(test, hibana_repo_tests))]
mod tests;
