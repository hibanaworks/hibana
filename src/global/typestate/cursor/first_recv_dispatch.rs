use super::{EventCursorMachine, LocalAction, ScopeId, StateIndex, state_index_to_usize};

#[derive(Clone, Copy)]
enum DispatchMatch {
    None,
    One(u8, StateIndex),
    Ambiguous,
}

impl DispatchMatch {
    #[inline(always)]
    fn add(&mut self, arm: u8, target: StateIndex) {
        match *self {
            Self::None => *self = Self::One(arm, target),
            Self::One(prev_arm, prev_target) if prev_arm == arm && prev_target == target => {}
            Self::One(_, _) | Self::Ambiguous => *self = Self::Ambiguous,
        }
    }

    #[inline(always)]
    const fn one(self) -> Option<(u8, StateIndex)> {
        match self {
            Self::One(arm, target) => Some((arm, target)),
            Self::None | Self::Ambiguous => None,
        }
    }
}

impl EventCursorMachine {
    pub(in crate::global::typestate::cursor) fn visit_first_recv_dispatch(
        &self,
        scope_id: ScopeId,
        mut visitor: impl FnMut(u8, StateIndex),
    ) -> Option<()> {
        self.route_scope_dense_ordinal(scope_id)?;
        let route_count = self.event_program().footprint().route_scope_count;
        let mut slot = 0usize;
        while slot < route_count {
            if let Some(region) = self.route_scope_rows_by_slot(slot) {
                let route_scope = region.scope();
                if route_scope.same(scope_id) {
                    self.visit_first_recv_dispatch_arm(route_scope, 0, 0, &mut visitor);
                    self.visit_first_recv_dispatch_arm(route_scope, 1, 1, &mut visitor);
                } else if let Some(root_arm) =
                    self.first_recv_dispatch_root_arm(scope_id, route_scope)
                {
                    self.visit_first_recv_dispatch_arm(route_scope, 0, root_arm, &mut visitor);
                    self.visit_first_recv_dispatch_arm(route_scope, 1, root_arm, &mut visitor);
                }
            }
            slot += 1;
        }
        Some(())
    }

    #[inline(always)]
    pub(in crate::global::typestate::cursor) fn first_recv_dispatch_arm_mask(
        &self,
        scope_id: ScopeId,
    ) -> Option<u8> {
        let mut mask = 0u8;
        self.visit_first_recv_dispatch(scope_id, |arm, target| {
            if arm < 2 && !target.is_absent() {
                mask |= 1u8 << arm;
            }
        })?;
        Some(mask)
    }

    pub(in crate::global::typestate::cursor) fn first_recv_descendant_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        let mut matched = DispatchMatch::None;
        self.visit_first_recv_dispatch(scope_id, |arm, target| {
            if arm >= 2 || target.is_absent() {
                return;
            }
            let node = self.node(state_index_to_usize(target));
            let LocalAction::Recv {
                lane: target_lane,
                frame_label: target_frame_label,
                ..
            } = node.action()
            else {
                return;
            };
            if target_lane == lane && target_frame_label == frame_label {
                matched.add(arm, target);
            }
        })?;
        matched.one()
    }

    #[inline(always)]
    fn visit_first_recv_dispatch_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        root_arm: u8,
        visitor: &mut impl FnMut(u8, StateIndex),
    ) {
        if let Some(target) = self.route_recv_state(scope_id, arm) {
            visitor(root_arm, target);
        }
    }

    #[inline(always)]
    fn first_recv_dispatch_root_arm(
        &self,
        root_scope: ScopeId,
        candidate_scope: ScopeId,
    ) -> Option<u8> {
        if candidate_scope.same(root_scope) {
            return None;
        }
        let route_count = self.event_program().footprint().route_scope_count;
        let mut current = candidate_scope;
        let mut hops = 0usize;
        while hops < route_count {
            let (parent, arm) = self.passive_parent_route(current)?;
            if parent.same(root_scope) {
                return Some(arm);
            }
            current = parent;
            hops += 1;
        }
        crate::invariant();
    }

    fn passive_parent_route(&self, child_scope: ScopeId) -> Option<(ScopeId, u8)> {
        let route_count = self.event_program().footprint().route_scope_count;
        let mut found = None;
        let mut slot = 0usize;
        while slot < route_count {
            if let Some(parent) = self
                .route_scope_rows_by_slot(slot)
                .map(|region| region.scope())
            {
                let mut arm = 0u8;
                while arm <= 1 {
                    if self
                        .passive_arm_child_fact_by_slot(slot, arm)
                        .and_then(|fact| fact.child_route_scope())
                        .is_some_and(|scope| scope.same(child_scope))
                    {
                        let candidate = (parent, arm);
                        if found.is_some_and(|prev| prev != candidate) {
                            crate::invariant();
                        }
                        found = Some(candidate);
                    }
                    if arm == 1 {
                        break;
                    }
                    arm += 1;
                }
            }
            slot += 1;
        }
        found
    }
}
