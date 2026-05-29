use super::{
    BindingInbox, BranchMeta, CAP_TOKEN_LEN, CapReleaseCtx, ControlDesc, E0, EndpointEpoch,
    EndpointLeaseId, EndpointSlot, EpochTable, EpochTbl, FrontierState, IngressEvidence,
    LabelUniverse, LaneGuard, LaneSlotArray, LeasedState, LoopDecision, MintConfigMarker,
    NoBinding, OfferState, Owner, PackedIngressEvidence, Payload, PhaseCursor, Port, RendezvousId,
    RouteCommitProofWorkspace, RouteDecisionSource, RouteState, ScopeId, SendMeta, SendState,
    SessionControlCtx, SessionId, StateIndex, Transport, lane_port,
};
use crate::endpoint::kernel::{decode, recv};
use crate::global::const_dsl::CompactScopeId;

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
    B: EndpointSlot = NoBinding,
> where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
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
    /// Phase-aware cursor for multi-lane parallel execution.
    pub(crate) cursor: PhaseCursor,
    pub(crate) public_rv: RendezvousId,
    pub(crate) public_slot: EndpointLeaseId,
    pub(crate) public_generation: u32,
    pub(crate) public_slot_owned: bool,
    pub(in crate::endpoint) public_offer_state: OfferState<'r>,
    pub(in crate::endpoint) public_route_branch: Option<MaterializedRouteBranch<'r>>,
    pub(in crate::endpoint) public_recv_state: recv::RecvState,
    pub(in crate::endpoint) public_decode_state: decode::DecodeState<'r>,
    pub(in crate::endpoint) public_send_state: SendState<'r>,
    pub(crate) control: SessionControlCtx<'r, T, U, C, E, MAX_RV>,
    pub(in crate::endpoint::kernel) decision_state: LeasedState<RouteState>,
    pub(in crate::endpoint::kernel) route_commit_proofs: LeasedState<RouteCommitProofWorkspace>,
    pub(in crate::endpoint::kernel) frontier_state: LeasedState<FrontierState>,
    pub(in crate::endpoint::kernel) binding_inbox: LeasedState<BindingInbox>,
    pub(crate) restored_binding_payload: Option<RestoredBindingPayload<'r>>,
    pub(crate) offer_progress_policy: crate::runtime::config::OfferProgressPolicy,
    pub(crate) mint: crate::control::cap::mint::MintConfig<
        <Mint as MintConfigMarker>::Spec,
        <Mint as MintConfigMarker>::Policy,
    >,
    pub(crate) binding: B,
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
    B: EndpointSlot + 'r,
> where
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
{
    pub(crate) label: u8,
    pub(in crate::endpoint::kernel) binding_evidence: PackedIngressEvidence,
    pub(in crate::endpoint::kernel) binding_evidence_lane: u8,
    pub(crate) staged_payload: Option<StagedPayload<'r>>,
    pub(crate) branch_meta: BranchMeta,
    pub(crate) _cfg: core::marker::PhantomData<fn() -> (&'r T, U, C, E, Mint, B)>,
}

pub(crate) struct MaterializedRouteBranch<'r> {
    pub(crate) label: u8,
    pub(in crate::endpoint::kernel) binding_evidence: PackedIngressEvidence,
    pub(in crate::endpoint::kernel) binding_evidence_lane: u8,
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
    Binding { lane: u8, payload: Payload<'a> },
}

#[derive(Clone, Copy)]
pub(crate) struct RestoredBindingPayload<'a> {
    pub(crate) lane: u8,
    pub(in crate::endpoint::kernel) evidence: PackedIngressEvidence,
    pub(crate) payload: Payload<'a>,
}

impl<'a> RestoredBindingPayload<'a> {
    #[inline]
    pub(crate) fn matches(self, lane_idx: usize, evidence: IngressEvidence) -> bool {
        let restored = self.evidence.decode();
        self.lane as usize == lane_idx
            && restored.frame_label == evidence.frame_label
            && restored.instance == evidence.instance
            && restored.channel == evidence.channel
    }
}

impl<'a> StagedPayload<'a> {
    #[inline]
    pub(crate) fn validated_payload<E, F>(&self, validate: F) -> Result<Payload<'a>, E>
    where
        F: FnOnce(Payload<'a>) -> Result<(), E>,
    {
        match self {
            Self::Transport { frame } => frame.validated_payload(validate),
            Self::Binding { payload, .. } => {
                validate(*payload)?;
                Ok(*payload)
            }
        }
    }

    #[inline]
    pub(crate) const fn lane(&self) -> u8 {
        match self {
            Self::Transport { frame } => frame.lane_wire(),
            Self::Binding { lane, .. } => *lane,
        }
    }

    #[inline]
    pub(crate) fn commit(self) -> Payload<'a> {
        match self {
            Self::Transport { frame } => frame.into_payload(),
            Self::Binding { payload, .. } => payload,
        }
    }

    #[inline]
    pub(crate) fn discard_terminal(self) {
        match self {
            Self::Transport { frame } => frame.discard_uncommitted(),
            Self::Binding { .. } => {}
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

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    From<RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>> for MaterializedRouteBranch<'r>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
{
    #[inline]
    fn from(branch: RouteBranch<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>) -> Self {
        Self {
            label: branch.label,
            binding_evidence: branch.binding_evidence,
            binding_evidence_lane: branch.binding_evidence_lane,
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

#[derive(Clone, Copy)]
pub(in crate::endpoint::kernel) enum SendControlDecisionPlan {
    None,
    Route {
        scope: CompactScopeId,
        arm: u8,
        source: RouteDecisionSource,
        lane: u8,
    },
    Loop {
        scope: CompactScopeId,
        idx: u8,
        decision: LoopDecision,
        lane: u8,
    },
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
    WireControlWithAutoRequest { dispatch: DescriptorDispatch },
    EmittedWireControl { token: MintedControlToken<'rv> },
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
