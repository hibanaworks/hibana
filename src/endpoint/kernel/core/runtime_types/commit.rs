use crate::{
    eff::EffIndex,
    endpoint::kernel::authority::LoopDecision,
    global::{
        const_dsl::{CompactScopeId, ScopeId},
        typestate::{RecvMeta, RelocatableResidentLaneStep, SendMeta, StateIndex},
    },
};

#[derive(Clone, Copy)]
pub(crate) struct CommitRow {
    scope: CompactScopeId,
    route_arm: Option<u8>,
    lane: u8,
}

impl CommitRow {
    #[inline(always)]
    pub(crate) const fn new(scope: ScopeId, route_arm: Option<u8>, lane: u8) -> Self {
        Self {
            scope: CompactScopeId::from_scope_id(scope),
            route_arm,
            lane,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_send_meta(meta: SendMeta) -> Self {
        Self::new(meta.scope, meta.route_arm, meta.lane)
    }

    #[inline(always)]
    pub(crate) const fn from_recv_meta(meta: RecvMeta) -> Self {
        Self::new(meta.scope, meta.route_arm, meta.lane)
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope.to_scope_id()
    }

    #[inline(always)]
    pub(crate) const fn route_arm(self) -> Option<u8> {
        self.route_arm
    }

    #[inline(always)]
    pub(crate) const fn lane(self) -> u8 {
        self.lane
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SelectedRouteCommitRow {
    scope: CompactScopeId,
    selected_arm: u8,
    lane: u8,
    scope_slot: u16,
    flags: u8,
}

impl SelectedRouteCommitRow {
    const LINGER: u8 = 0b0000_0001;
    pub(crate) const EMPTY: Self = Self {
        scope: CompactScopeId::none(),
        selected_arm: u8::MAX,
        lane: 0,
        scope_slot: u16::MAX,
        flags: 0,
    };

    #[inline(always)]
    pub(crate) const fn new(
        scope: ScopeId,
        selected_arm: u8,
        lane: u8,
        scope_slot: u16,
        is_linger: bool,
    ) -> Self {
        Self {
            scope: CompactScopeId::from_scope_id(scope),
            selected_arm,
            lane,
            scope_slot,
            flags: if is_linger { Self::LINGER } else { 0 },
        }
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope.to_scope_id()
    }

    #[inline(always)]
    pub(crate) const fn selected_arm(self) -> u8 {
        self.selected_arm
    }

    #[inline(always)]
    pub(crate) const fn lane(self) -> u8 {
        self.lane
    }

    #[inline(always)]
    pub(crate) const fn scope_slot(self) -> u16 {
        self.scope_slot
    }

    #[inline(always)]
    pub(crate) const fn is_linger(self) -> bool {
        self.flags & Self::LINGER != 0
    }

    #[inline(always)]
    pub(crate) const fn is_empty(self) -> bool {
        self.selected_arm == u8::MAX || self.scope_slot == u16::MAX || self.scope.is_none()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum LoopCommitDisposition {
    Decision { decision: LoopDecision },
    Ack { role: u8, local_decision: bool },
}

#[derive(Clone, Copy)]
pub(crate) struct LoopCommitRow {
    scope: CompactScopeId,
    idx: u8,
    lane: u8,
    disposition: Option<LoopCommitDisposition>,
}

impl LoopCommitRow {
    pub(crate) const EMPTY: Self = Self {
        scope: CompactScopeId::none(),
        idx: u8::MAX,
        lane: u8::MAX,
        disposition: None,
    };

    #[inline(always)]
    pub(crate) const fn decision(
        scope: ScopeId,
        idx: u8,
        lane: u8,
        decision: LoopDecision,
    ) -> Self {
        Self {
            scope: CompactScopeId::from_scope_id(scope),
            idx,
            lane,
            disposition: Some(LoopCommitDisposition::Decision { decision }),
        }
    }

    #[inline(always)]
    pub(crate) const fn ack(
        scope: ScopeId,
        idx: u8,
        lane: u8,
        role: u8,
        local_decision: bool,
    ) -> Self {
        Self {
            scope: CompactScopeId::from_scope_id(scope),
            idx,
            lane,
            disposition: Some(LoopCommitDisposition::Ack {
                role,
                local_decision,
            }),
        }
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope.to_scope_id()
    }

    #[inline(always)]
    pub(crate) const fn idx(self) -> u8 {
        self.idx
    }

    #[inline(always)]
    pub(crate) const fn lane(self) -> u8 {
        self.lane
    }

    #[inline(always)]
    pub(crate) const fn disposition(self) -> Option<LoopCommitDisposition> {
        self.disposition
    }

    #[inline(always)]
    pub(crate) const fn is_empty(self) -> bool {
        self.disposition.is_none() || self.scope.is_none() || self.idx == u8::MAX
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CommitEventRow {
    row: CommitRow,
    progress_step: RelocatableResidentLaneStep,
    eff_dense: u16,
    event_label: u8,
    event_flags: u8,
}

#[derive(Clone, Copy)]
pub(crate) struct CommitDelta {
    event: Option<CommitEventRow>,
    selected_routes: SelectedRouteCommitRowsRef,
    loop_row: LoopCommitRow,
    lane_relocation: Option<RelocatableResidentLaneStep>,
    pub(crate) cursor_after: StateIndex,
}

#[derive(Clone, Copy)]
pub(crate) struct PreparedCommitDelta {
    delta: CommitDelta,
}

#[derive(Clone, Copy)]
pub(crate) struct ParentRouteEvidenceRow {
    scope: CompactScopeId,
    arm: u8,
    lane: u8,
}

impl ParentRouteEvidenceRow {
    #[inline(always)]
    pub(crate) const fn new(scope: ScopeId, arm: u8, lane: u8) -> Self {
        Self {
            scope: CompactScopeId::from_scope_id(scope),
            arm,
            lane,
        }
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope.to_scope_id()
    }

    #[inline(always)]
    pub(crate) const fn arm(self) -> u8 {
        self.arm
    }

    #[inline(always)]
    pub(crate) const fn lane(self) -> u8 {
        self.lane
    }
}

impl PreparedCommitDelta {
    #[inline(always)]
    pub(in crate::endpoint::kernel) const fn from_preflighted(delta: CommitDelta) -> Self {
        Self { delta }
    }

    #[inline(always)]
    pub(crate) const fn delta(self) -> CommitDelta {
        self.delta
    }
}

impl CommitDelta {
    #[inline(always)]
    pub(crate) const fn from_meta(
        meta: SendMeta,
        cursor_after: StateIndex,
        progress_step: RelocatableResidentLaneStep,
    ) -> Self {
        Self {
            event: Some(CommitEventRow::new(
                meta.eff_index,
                meta.label,
                meta.is_control,
                CommitRow::from_send_meta(meta),
                progress_step,
            )),
            selected_routes: SelectedRouteCommitRowsRef::EMPTY,
            loop_row: LoopCommitRow::EMPTY,
            lane_relocation: None,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_recv_meta(
        meta: RecvMeta,
        cursor_after: StateIndex,
        progress_step: RelocatableResidentLaneStep,
    ) -> Self {
        Self {
            event: Some(CommitEventRow::new(
                meta.eff_index,
                meta.label,
                meta.is_control,
                CommitRow::from_recv_meta(meta),
                progress_step,
            )),
            selected_routes: SelectedRouteCommitRowsRef::EMPTY,
            loop_row: LoopCommitRow::EMPTY,
            lane_relocation: None,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_event_row(
        eff_index: EffIndex,
        event_label: u8,
        event_control: bool,
        row: CommitRow,
        cursor_after: StateIndex,
        progress_step: RelocatableResidentLaneStep,
    ) -> Self {
        Self {
            event: Some(CommitEventRow::new(
                eff_index,
                event_label,
                event_control,
                row,
                progress_step,
            )),
            selected_routes: SelectedRouteCommitRowsRef::EMPTY,
            loop_row: LoopCommitRow::EMPTY,
            lane_relocation: None,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn route_only(row: SelectedRouteCommitRow, cursor_after: StateIndex) -> Self {
        Self {
            event: None,
            selected_routes: SelectedRouteCommitRowsRef::from_inline(row),
            loop_row: LoopCommitRow::EMPTY,
            lane_relocation: None,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn route_rows(
        rows: SelectedRouteCommitRowsRef,
        cursor_after: StateIndex,
    ) -> Self {
        Self {
            event: None,
            selected_routes: rows,
            loop_row: LoopCommitRow::EMPTY,
            lane_relocation: None,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn cursor_only(cursor_after: StateIndex) -> Self {
        Self {
            event: None,
            selected_routes: SelectedRouteCommitRowsRef::EMPTY,
            loop_row: LoopCommitRow::EMPTY,
            lane_relocation: None,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn with_selected_route(mut self, row: SelectedRouteCommitRow) -> Self {
        self.selected_routes = SelectedRouteCommitRowsRef::from_inline(row);
        self
    }

    #[inline(always)]
    pub(crate) const fn with_selected_route_rows(
        mut self,
        rows: SelectedRouteCommitRowsRef,
    ) -> Self {
        self.selected_routes = rows;
        self
    }

    #[inline(always)]
    pub(crate) const fn with_loop_row(mut self, row: LoopCommitRow) -> Self {
        self.loop_row = row;
        self
    }

    #[inline(always)]
    pub(crate) const fn with_lane_relocation(
        mut self,
        step: Option<RelocatableResidentLaneStep>,
    ) -> Self {
        self.lane_relocation = step;
        self
    }

    #[inline(always)]
    pub(crate) const fn event(&self) -> Option<CommitEventRow> {
        self.event
    }

    #[inline(always)]
    pub(crate) const fn selected_routes(&self) -> SelectedRouteCommitSet {
        SelectedRouteCommitSet::new(self.selected_routes)
    }

    #[inline(always)]
    pub(crate) const fn parent_route_evidence(&self) -> Option<ParentRouteEvidenceRow> {
        self.selected_routes.parent_route_evidence()
    }

    #[inline(always)]
    pub(crate) const fn loop_row(&self) -> LoopCommitRow {
        self.loop_row
    }

    #[inline(always)]
    pub(crate) const fn lane_relocation(&self) -> Option<RelocatableResidentLaneStep> {
        self.lane_relocation
    }

    #[inline(always)]
    pub(crate) const fn cursor_after(&self) -> StateIndex {
        self.cursor_after
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SelectedRouteCommitRowsRef {
    word0: u64,
    word1: u64,
}

impl SelectedRouteCommitRowsRef {
    const KIND_MASK: u64 = 0b11;
    const KIND_EMPTY: u64 = 0;
    const KIND_INLINE: u64 = 1;
    const KIND_SLICE: u64 = 2;
    const KIND_INLINE_PARENT: u64 = 3;
    const PARENT_SCOPE_LOW_MASK: u64 = (1u64 << 30) - 1;

    pub(crate) const EMPTY: Self = Self {
        word0: Self::KIND_EMPTY,
        word1: 0,
    };

    #[inline(always)]
    pub(crate) const fn from_inline(row: SelectedRouteCommitRow) -> Self {
        if row.is_empty() {
            return Self::EMPTY;
        }
        Self {
            word0: ((row.scope.compact_raw() as u64) << 2) | Self::KIND_INLINE,
            word1: (row.selected_arm as u64)
                | ((row.lane as u64) << 8)
                | ((row.scope_slot as u64) << 16)
                | ((row.flags as u64) << 32),
        }
    }

    #[inline(always)]
    pub(crate) const fn from_inline_with_parent_route_evidence(
        row: SelectedRouteCommitRow,
        parent: ParentRouteEvidenceRow,
    ) -> Self {
        if row.is_empty() {
            return Self::EMPTY;
        }
        let parent_raw = parent.scope.compact_raw() as u64;
        Self {
            word0: ((row.scope.compact_raw() as u64) << 2)
                | ((parent_raw & Self::PARENT_SCOPE_LOW_MASK) << 34)
                | Self::KIND_INLINE_PARENT,
            word1: (row.selected_arm as u64)
                | ((row.lane as u64) << 8)
                | ((row.scope_slot as u64) << 16)
                | ((row.flags as u64) << 32)
                | ((parent_raw >> 30) << 40)
                | ((parent.arm as u64) << 42)
                | ((parent.lane as u64) << 50),
        }
    }

    #[inline(always)]
    pub(crate) fn from_slice(rows: &[SelectedRouteCommitRow]) -> Self {
        if rows.is_empty() {
            Self::EMPTY
        } else {
            let ptr = rows.as_ptr() as usize as u64;
            debug_assert_eq!(ptr & Self::KIND_MASK, 0);
            Self {
                word0: ptr | Self::KIND_SLICE,
                word1: rows.len() as u64,
            }
        }
    }

    #[inline(always)]
    pub(crate) const fn is_empty(self) -> bool {
        self.len() == 0
    }

    #[inline(always)]
    const fn len(self) -> usize {
        match self.kind() {
            Self::KIND_INLINE | Self::KIND_INLINE_PARENT => 1,
            Self::KIND_SLICE => self.word1 as usize,
            _ => 0,
        }
    }

    #[inline(always)]
    fn get(self, idx: usize) -> Option<SelectedRouteCommitRow> {
        match self.kind() {
            Self::KIND_INLINE | Self::KIND_INLINE_PARENT => {
                if idx == 0 {
                    Some(self.inline_row())
                } else {
                    None
                }
            }
            Self::KIND_SLICE => {
                if idx >= self.len() {
                    return None;
                }
                let ptr = (self.word0 & !Self::KIND_MASK) as usize as *const SelectedRouteCommitRow;
                if ptr.is_null() {
                    return None;
                }
                Some(
                    /* SAFETY: `idx < len` and `ptr` was produced from a live scratch slice for the duration of the prepared commit. */
                    unsafe { *ptr.add(idx) },
                )
            }
            _ => None,
        }
    }

    #[inline(always)]
    const fn kind(self) -> u64 {
        self.word0 & Self::KIND_MASK
    }

    #[inline(always)]
    const fn inline_row(self) -> SelectedRouteCommitRow {
        SelectedRouteCommitRow {
            scope: CompactScopeId::from_compact_raw(((self.word0 >> 2) & 0xffff_ffff) as u32),
            selected_arm: (self.word1 & 0xff) as u8,
            lane: ((self.word1 >> 8) & 0xff) as u8,
            scope_slot: ((self.word1 >> 16) & 0xffff) as u16,
            flags: ((self.word1 >> 32) & 0xff) as u8,
        }
    }

    #[inline(always)]
    const fn parent_route_evidence(self) -> Option<ParentRouteEvidenceRow> {
        if self.kind() != Self::KIND_INLINE_PARENT {
            return None;
        }
        let parent_raw = ((self.word0 >> 34) & Self::PARENT_SCOPE_LOW_MASK)
            | (((self.word1 >> 40) & 0b11) << 30);
        Some(ParentRouteEvidenceRow {
            scope: CompactScopeId::from_compact_raw(parent_raw as u32),
            arm: ((self.word1 >> 42) & 0xff) as u8,
            lane: ((self.word1 >> 50) & 0xff) as u8,
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SelectedRouteCommitSet {
    rows: SelectedRouteCommitRowsRef,
}

impl SelectedRouteCommitSet {
    #[inline(always)]
    const fn new(rows: SelectedRouteCommitRowsRef) -> Self {
        Self { rows }
    }

    #[inline(always)]
    pub(crate) const fn is_empty(self) -> bool {
        self.rows.len() == 0
    }

    #[inline(always)]
    pub(crate) const fn len(self) -> usize {
        self.rows.len()
    }

    #[inline(always)]
    pub(crate) fn get(self, idx: usize) -> Option<SelectedRouteCommitRow> {
        self.rows.get(idx)
    }
}

impl CommitEventRow {
    const CONTROL: u8 = 0b0000_0001;

    #[inline(always)]
    pub(crate) const fn new(
        eff_index: EffIndex,
        event_label: u8,
        event_control: bool,
        row: CommitRow,
        progress_step: RelocatableResidentLaneStep,
    ) -> Self {
        Self {
            row,
            progress_step,
            eff_dense: eff_index.dense_ordinal() as u16,
            event_label,
            event_flags: if event_control { Self::CONTROL } else { 0 },
        }
    }

    #[inline(always)]
    pub(crate) const fn eff_index(self) -> EffIndex {
        EffIndex::from_dense_ordinal(self.eff_dense as usize)
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.row.scope()
    }

    #[inline(always)]
    pub(crate) const fn route_arm(self) -> Option<u8> {
        self.row.route_arm()
    }

    #[inline(always)]
    pub(crate) const fn lane(self) -> u8 {
        self.row.lane()
    }

    #[inline(always)]
    pub(crate) const fn progress_step(self) -> RelocatableResidentLaneStep {
        self.progress_step
    }

    #[inline(always)]
    pub(crate) const fn event_label(self) -> u8 {
        self.event_label
    }

    #[inline(always)]
    pub(crate) const fn event_control(self) -> bool {
        self.event_flags & Self::CONTROL != 0
    }

    #[inline(always)]
    pub(crate) const fn event_id(self, message: u16, control: u16) -> u16 {
        if self.event_control() {
            control
        } else {
            message
        }
    }
}
