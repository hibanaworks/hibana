use crate::{
    eff::{EffIndex, EventOrigin},
    endpoint::kernel::decision_state::{
        RouteOnlyCommitRowsRef, SelectedRouteCommitRow, SelectedRouteCommitRowsRef,
    },
    global::{
        const_dsl::ScopeId,
        typestate::{RecvMeta, RelocatableResidentLaneStep, SendMeta, StateIndex},
    },
};

#[derive(Clone, Copy)]
pub(crate) struct CommitRow {
    scope: ScopeId,
    route_arm: Option<u8>,
    lane: u8,
}

impl CommitRow {
    #[inline(always)]
    pub(crate) const fn new(scope: ScopeId, route_arm: Option<u8>, lane: u8) -> Self {
        Self {
            scope,
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
        self.scope
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
    lane_relocation: Option<RelocatableResidentLaneStep>,
    pub(crate) cursor_after: StateIndex,
}

impl CommitDelta {
    #[inline(always)]
    pub(crate) const fn from_meta(
        meta: SendMeta,
        selected_routes: SelectedRouteCommitRowsRef,
        cursor_after: StateIndex,
        progress_step: RelocatableResidentLaneStep,
    ) -> Self {
        Self {
            event: Some(CommitEventRow::new(
                meta.eff_index,
                meta.label,
                meta.origin,
                CommitRow::from_send_meta(meta),
                progress_step,
            )),
            selected_routes,
            lane_relocation: None,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_recv_meta(
        meta: RecvMeta,
        selected_routes: SelectedRouteCommitRowsRef,
        cursor_after: StateIndex,
        progress_step: RelocatableResidentLaneStep,
    ) -> Self {
        Self {
            event: Some(CommitEventRow::new(
                meta.eff_index,
                meta.label,
                meta.origin,
                CommitRow::from_recv_meta(meta),
                progress_step,
            )),
            selected_routes,
            lane_relocation: None,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn from_event_row(
        eff_index: EffIndex,
        event_label: u8,
        origin: EventOrigin,
        row: CommitRow,
        selected_routes: SelectedRouteCommitRowsRef,
        cursor_after: StateIndex,
        progress_step: RelocatableResidentLaneStep,
    ) -> Self {
        Self {
            event: Some(CommitEventRow::new(
                eff_index,
                event_label,
                origin,
                row,
                progress_step,
            )),
            selected_routes,
            lane_relocation: None,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn route_rows(rows: RouteOnlyCommitRowsRef, cursor_after: StateIndex) -> Self {
        Self {
            event: None,
            selected_routes: rows.selected_routes(),
            lane_relocation: None,
            cursor_after,
        }
    }

    #[inline(always)]
    pub(crate) const fn cursor_only(cursor_after: StateIndex) -> Self {
        Self {
            event: None,
            selected_routes: SelectedRouteCommitRowsRef::EMPTY,
            lane_relocation: None,
            cursor_after,
        }
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
    pub(in crate::endpoint::kernel) const fn selected_route_rows_ref(
        &self,
    ) -> SelectedRouteCommitRowsRef {
        self.selected_routes
    }

    #[inline(always)]
    pub(crate) const fn selected_route_lane(&self) -> Option<u8> {
        self.selected_routes.selected_lane()
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
    pub(crate) fn get(
        self,
        cursor: &crate::global::typestate::EventCursor,
        idx: usize,
    ) -> Option<SelectedRouteCommitRow> {
        self.rows.get(cursor, idx)
    }
}

impl CommitEventRow {
    const SESSION: u8 = 0b0000_0001;

    #[inline(always)]
    pub(crate) const fn new(
        eff_index: EffIndex,
        event_label: u8,
        origin: EventOrigin,
        row: CommitRow,
        progress_step: RelocatableResidentLaneStep,
    ) -> Self {
        Self {
            row,
            progress_step,
            eff_dense: eff_index.dense_ordinal() as u16,
            event_label,
            event_flags: if origin.is_session() {
                Self::SESSION
            } else {
                0
            },
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
    pub(crate) const fn event_origin(self) -> EventOrigin {
        if self.event_flags & Self::SESSION != 0 {
            EventOrigin::Session
        } else {
            EventOrigin::User
        }
    }

    #[inline(always)]
    pub(crate) const fn event_id(self, message: u16, session: u16) -> u16 {
        if self.event_origin().is_session() {
            session
        } else {
            message
        }
    }
}
