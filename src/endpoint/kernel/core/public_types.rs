use super::{
    BranchMeta, CAP_TOKEN_LEN, CapReleaseCtx, ControlDesc, E0, EndpointEpoch, EndpointLeaseId,
    EpochTable, EpochTbl, EventCursor, FrontierState, LabelUniverse, LaneGuard, LaneSlotArray,
    LeasedState, MintConfigMarker, OfferState, Owner, Payload, Port, RendezvousId,
    RouteCommitRowWorkspace, RouteState, ScopeId, SendMeta, SendState, SessionControlCtx,
    SessionId, StateIndex, Transport, lane_port,
};
use crate::endpoint::kernel::{decode, recv};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::endpoint) enum PublicActiveOp {
    Idle,
    Poisoned,
    Send,
    Recv,
    Offer,
    RouteBranch,
    Decode,
}

/// Internal endpoint kernel. Owns the rendezvous port as well as the lane
/// release handle. Dropping the endpoint releases the lane back to the
/// `SessionCluster` via the handle.
#[repr(C)]
pub struct CursorEndpoint<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    U = crate::runtime::consts::DefaultLabelUniverse,
    C = crate::runtime::config::CounterClock,
    E: EpochTable = EpochTbl,
    const MAX_RV: usize = 8,
    Mint = crate::control::cap::mint::MintConfig,
> where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    pub(crate) public_header: crate::endpoint::carrier::KernelEndpointHeader<'r>,
    /// Multi-lane port array. Each active lane has its own port.
    /// For single-lane programs, only `ports[0]` is used.
    pub(in crate::endpoint::kernel) ports: LaneSlotArray<Port<'r, T, E>>,
    /// Multi-lane guard array. Each active lane has its own guard.
    pub(in crate::endpoint::kernel) guards: LaneSlotArray<LaneGuard<'r, T, U, C>>,
    /// Primary lane index (first live application lane, not always lane 0).
    pub(crate) primary_lane: usize,
    pub(crate) sid: SessionId,
    pub(crate) _owner: Owner<'r, E0>,
    pub(crate) _epoch: EndpointEpoch<'r, E>,
    /// Event cursor for multi-lane affine progress.
    pub(crate) cursor: EventCursor,
    pub(crate) public_rv: RendezvousId,
    pub(crate) public_slot: EndpointLeaseId,
    pub(crate) public_generation: u32,
    pub(crate) public_slot_owned: bool,
    pub(in crate::endpoint) public_active_op: PublicActiveOp,
    pub(in crate::endpoint) public_offer_state: OfferState<'r>,
    pub(in crate::endpoint) public_route_branch: Option<MaterializedRouteBranch<'r>>,
    pub(in crate::endpoint) public_recv_state: recv::RecvState,
    pub(in crate::endpoint) public_decode_state: decode::DecodeState<'r>,
    pub(in crate::endpoint) public_send_state: SendState<'r>,
    pub(crate) control: SessionControlCtx<'r, T, U, C, E, MAX_RV>,
    pub(in crate::endpoint::kernel) decision_state: LeasedState<RouteState>,
    pub(in crate::endpoint::kernel) route_commit_rows: LeasedState<RouteCommitRowWorkspace>,
    pub(in crate::endpoint::kernel) frontier_state: LeasedState<FrontierState>,
    pub(crate) offer_progress_policy: crate::runtime::config::OfferProgressPolicy,
    pub(crate) mint: crate::control::cap::mint::MintConfig<
        <Mint as MintConfigMarker>::Spec,
        <Mint as MintConfigMarker>::Policy,
    >,
}

pub struct RouteBranch<
    'r,
    const ROLE: u8,
    T: Transport + 'r,
    U,
    C,
    E: EpochTable,
    const MAX_RV: usize,
    Mint,
> where
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
{
    pub(crate) label: u8,
    pub(crate) staged_payload: Option<StagedPayload<'r>>,
    pub(crate) branch_meta: BranchMeta,
    pub(crate) _cfg: core::marker::PhantomData<fn() -> (&'r T, U, C, E, Mint)>,
}

pub(crate) struct MaterializedRouteBranch<'r> {
    pub(crate) label: u8,
    pub(crate) staged_payload: Option<StagedPayload<'r>>,
    pub(crate) branch_meta: BranchMeta,
}

impl<'r> MaterializedRouteBranch<'r> {
    #[inline]
    pub(crate) const fn label(&self) -> u8 {
        self.label
    }

    #[inline]
    pub(crate) fn discard_terminal(mut self) {
        if let Some(payload) = self.staged_payload.take() {
            payload.discard_terminal();
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct BranchPreviewView {
    pub(in crate::endpoint::kernel) label: u8,
    pub(in crate::endpoint::kernel) branch_meta: BranchMeta,
}

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) struct ParentRouteDecisionPlan {
    pub(crate) scope: ScopeId,
    pub(crate) arm: u8,
    pub(crate) lane: u8,
}

impl BranchPreviewView {
    #[inline]
    pub(in crate::endpoint::kernel) const fn new(label: u8, branch_meta: BranchMeta) -> Self {
        Self { label, branch_meta }
    }

    #[inline]
    pub(in crate::endpoint::kernel) const fn from_materialized(
        branch: &MaterializedRouteBranch<'_>,
    ) -> Self {
        Self::new(branch.label, branch.branch_meta)
    }
}

pub(crate) enum StagedPayload<'a> {
    Transport { frame: lane_port::ReceivedFrame<'a> },
}

impl<'a> StagedPayload<'a> {
    #[inline]
    pub(crate) fn validated_payload<E, F>(&self, validate: F) -> Result<Payload<'a>, E>
    where
        F: FnOnce(Payload<'a>) -> Result<(), E>,
    {
        match self {
            Self::Transport { frame } => frame.validated_payload(validate),
        }
    }

    #[inline]
    pub(crate) const fn lane(&self) -> u8 {
        match self {
            Self::Transport { frame } => frame.lane_wire(),
        }
    }

    #[inline]
    pub(crate) const fn transport_frame_label(&self) -> Option<u8> {
        match self {
            Self::Transport { frame } => Some(frame.frame_label_raw()),
        }
    }

    #[inline]
    pub(crate) fn commit(self) -> Payload<'a> {
        match self {
            Self::Transport { frame } => frame.into_payload(),
        }
    }

    #[inline]
    pub(crate) fn discard_terminal(self) {
        match self {
            Self::Transport { frame } => frame.discard_uncommitted(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SendPreview {
    meta: SendMeta,
    cursor_index: StateIndex,
}

impl SendPreview {
    #[inline]
    pub(crate) const fn new(meta: SendMeta, cursor_index: StateIndex) -> Self {
        Self { meta, cursor_index }
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> u8 {
        self.meta.frame_label
    }

    #[inline]
    pub(crate) const fn into_parts(self) -> (SendMeta, StateIndex) {
        (self.meta, self.cursor_index)
    }
}

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    From<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint>> for MaterializedRouteBranch<'r>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    #[inline]
    fn from(branch: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint>) -> Self {
        Self {
            label: branch.label,
            staged_payload: branch.staged_payload,
            branch_meta: branch.branch_meta,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DescriptorDispatch {
    pub(crate) desc: ControlDesc,
    pub(crate) scope_id: u16,
    pub(crate) epoch: u16,
}

impl DescriptorDispatch {
    #[inline(always)]
    pub(crate) const fn new(desc: ControlDesc, scope: ScopeId, epoch: u16) -> Self {
        Self {
            desc,
            scope_id: scope.local_ordinal(),
            epoch,
        }
    }
}

pub(crate) struct MintedControlToken<'rv> {
    pub(crate) token_bytes: [u8; CAP_TOKEN_LEN],
    pub(crate) dispatch: DescriptorDispatch,
    pub(crate) rollback: PendingCapRelease<'rv>,
}

pub(crate) enum SendPayloadPlan<'rv> {
    Data,
    LocalControl { token: MintedControlToken<'rv> },
    ExplicitWireControl { dispatch: DescriptorDispatch },
}

pub(crate) struct PendingCapRelease<'rv> {
    nonce: [u8; crate::control::cap::mint::CAP_NONCE_LEN],
    release_ctx: Option<CapReleaseCtx<'rv>>,
}

impl<'rv> PendingCapRelease<'rv> {
    #[inline(always)]
    pub(crate) fn new(
        nonce: [u8; crate::control::cap::mint::CAP_NONCE_LEN],
        release_ctx: CapReleaseCtx<'rv>,
    ) -> Self {
        Self {
            nonce,
            release_ctx: Some(release_ctx),
        }
    }

    #[inline(always)]
    pub(crate) fn release_now(mut self) {
        if let Some(release_ctx) = self.release_ctx.take() {
            release_ctx.release(&self.nonce);
        }
        self.nonce.fill(0);
    }
}

impl<'rv> Drop for PendingCapRelease<'rv> {
    fn drop(&mut self) {
        if let Some(release_ctx) = self.release_ctx.take() {
            release_ctx.release(&self.nonce);
        }
        self.nonce.fill(0);
    }
}
