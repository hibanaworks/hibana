use super::{
    EventCursorMachine, FirstRecvDispatchSpec, LocalAction, MAX_FIRST_RECV_DISPATCH,
    PackedEventConflict, ScopeId, StateIndex, state_index_to_usize,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FirstRecvDispatchVisit {
    scope: ScopeId,
    arm: u8,
    root_arm: u8,
}

impl FirstRecvDispatchVisit {
    const EMPTY: Self = Self {
        scope: ScopeId::none(),
        arm: 0,
        root_arm: 0,
    };

    #[inline(always)]
    const fn new(scope: ScopeId, arm: u8, root_arm: u8) -> Self {
        Self {
            scope,
            arm,
            root_arm,
        }
    }
}

impl EventCursorMachine {
    #[inline(always)]
    pub(in crate::global::typestate::cursor) fn first_recv_dispatch_table(
        &self,
        scope_id: ScopeId,
    ) -> Option<([FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH], u8)> {
        self.route_scope_dense_ordinal(scope_id)?;
        let mut table = [FirstRecvDispatchSpec::EMPTY; MAX_FIRST_RECV_DISPATCH];
        let mut table_len = 0usize;
        let mut visits = [FirstRecvDispatchVisit::EMPTY; MAX_FIRST_RECV_DISPATCH];
        let mut visit_len = 0usize;

        Self::push_first_recv_dispatch_visit(
            &mut visits,
            &mut visit_len,
            FirstRecvDispatchVisit::new(scope_id, 1, 1),
        );
        Self::push_first_recv_dispatch_visit(
            &mut visits,
            &mut visit_len,
            FirstRecvDispatchVisit::new(scope_id, 0, 0),
        );

        let mut depth = 0usize;
        while visit_len > 0 {
            if depth >= PackedEventConflict::MAX_CHAIN_DEPTH {
                crate::invariant();
            }
            visit_len -= 1;
            let visit = visits[visit_len];
            depth += 1;

            if let Some(target) = self.route_recv_state(visit.scope, visit.arm) {
                self.push_first_recv_dispatch_spec(
                    &mut table,
                    &mut table_len,
                    visit.root_arm,
                    target,
                );
            }
            if let Some(child_scope) = self.passive_child_scope_for_dispatch(visit.scope, visit.arm)
            {
                Self::push_first_recv_dispatch_visit(
                    &mut visits,
                    &mut visit_len,
                    FirstRecvDispatchVisit::new(child_scope, 1, visit.root_arm),
                );
                Self::push_first_recv_dispatch_visit(
                    &mut visits,
                    &mut visit_len,
                    FirstRecvDispatchVisit::new(child_scope, 0, visit.root_arm),
                );
            }
        }
        Some((table, table_len as u8))
    }

    #[inline(always)]
    fn push_first_recv_dispatch_visit(
        visits: &mut [FirstRecvDispatchVisit; MAX_FIRST_RECV_DISPATCH],
        len: &mut usize,
        visit: FirstRecvDispatchVisit,
    ) {
        if *len >= visits.len() {
            crate::invariant();
        }
        visits[*len] = visit;
        *len += 1;
    }

    #[inline(always)]
    fn push_first_recv_dispatch_spec(
        &self,
        table: &mut [FirstRecvDispatchSpec; MAX_FIRST_RECV_DISPATCH],
        len: &mut usize,
        root_arm: u8,
        target: StateIndex,
    ) {
        let node = self.node(state_index_to_usize(target));
        let LocalAction::Recv {
            lane, frame_label, ..
        } = node.action()
        else {
            return;
        };
        let mut idx = 0usize;
        while idx < *len {
            let entry = table[idx];
            if entry.lane() == lane
                && entry.frame_label() == frame_label
                && entry.arm() == root_arm
                && entry.target() == target
            {
                return;
            }
            idx += 1;
        }
        if *len >= table.len() {
            crate::invariant();
        }
        table[*len] = FirstRecvDispatchSpec::new(lane, frame_label, root_arm, target);
        *len += 1;
    }

    #[inline(always)]
    fn passive_child_scope_for_dispatch(&self, scope_id: ScopeId, arm: u8) -> Option<ScopeId> {
        let slot = self.route_scope_dense_ordinal(scope_id)?;
        self.passive_arm_child_fact_by_slot(slot, arm)?
            .child_route_scope()
    }
}
