//! Monolithic lowering walk owner extracted from `emit.rs`.

use super::{
    builder::{ARM_SHARED, MAX_FIRST_RECV_DISPATCH, encode_typestate_len},
    emit_route::{MAX_LOOP_TRACKED, find_loop_entry_state, store_loop_entry_if_absent},
    emit_scope::{alloc_scope_record, init_scope_registry},
    facts::{
        JumpReason, LocalAction, LocalNode, MAX_STATES, StateIndex, as_eff_index, as_state_index,
        state_index_to_usize,
    },
    registry::{
        CONTROLLER_ROLE_NONE, RouteScopeRecord, SCOPE_LINK_NONE, ScopeRecord, offer_lane_bit,
    },
    route_facts::{
        MAX_PREFIX_ACTIONS, PREFIX_KIND_LOCAL, PREFIX_KIND_SEND, PrefixAction,
        arm_common_prefix_end, arm_sequences_equal, continuations_equivalent, prefix_action_eq,
        route_policy_differs,
    },
};

const MAX_SCOPE_SCRATCH: usize = crate::eff::meta::MAX_EFF_NODES;
const LINGER_ARM_NO_NODE: u16 = u16::MAX;
const ROUTE_PASSIVE_ARM_UNSET: u16 = u16::MAX;
const MAX_JUMP_BACKPATCH: usize = MAX_STATES;

pub(crate) struct RoleTypestateBuildScratch {
    pub(super) loop_entry_ids: [ScopeId; MAX_LOOP_TRACKED],
    pub(super) loop_entry_states: [StateIndex; MAX_LOOP_TRACKED],
    pub(super) linger_arm_last_node: [[u16; 2]; MAX_SCOPE_SCRATCH],
    pub(super) linger_arm_scope_ids: [ScopeId; MAX_SCOPE_SCRATCH],
    pub(super) linger_arm_current: [u8; MAX_SCOPE_SCRATCH],
    pub(super) linger_passive_arm_start: [[u16; 2]; MAX_SCOPE_SCRATCH],
    pub(super) linger_is_passive: [bool; MAX_SCOPE_SCRATCH],
    pub(super) jump_backpatch_indices: [u16; MAX_JUMP_BACKPATCH],
    pub(super) jump_backpatch_scopes: [ScopeId; MAX_JUMP_BACKPATCH],
    pub(super) jump_backpatch_kinds: [u8; MAX_JUMP_BACKPATCH],
    pub(super) scope_stack: [ScopeId; MAX_SCOPE_SCRATCH],
    pub(super) scope_stack_kinds: [ScopeKind; MAX_SCOPE_SCRATCH],
    pub(super) scope_stack_entries: [u16; MAX_SCOPE_SCRATCH],
    pub(super) route_current_arm: [u8; MAX_SCOPE_SCRATCH],
    pub(super) last_step_was_scope: [bool; MAX_SCOPE_SCRATCH],
    pub(super) route_arm_last_node: [[StateIndex; 2]; MAX_SCOPE_SCRATCH],
    pub(super) route_enter_count: [u8; MAX_SCOPE_SCRATCH],
    pub(super) route_passive_arm_start: [[u16; 2]; MAX_SCOPE_SCRATCH],
    pub(super) route_is_passive: [bool; MAX_SCOPE_SCRATCH],
    pub(super) route_scope_entries: [RouteScopeRecord; MAX_SCOPE_SCRATCH],
    pub(super) dispatch_table: [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    pub(super) prefix_actions: [[PrefixAction; MAX_PREFIX_ACTIONS]; 2],
    pub(super) prefix_lens: [usize; 2],
    pub(super) arm_seen_recv: [bool; 2],
    pub(super) scan_stack: [StateIndex; MAX_SCOPE_SCRATCH],
    pub(super) visited: [bool; MAX_STATES],
}

impl RoleTypestateBuildScratch {
    #[cfg(test)]
    pub(crate) const fn new() -> Self {
        Self {
            loop_entry_ids: [ScopeId::generic(0); MAX_LOOP_TRACKED],
            loop_entry_states: [StateIndex::MAX; MAX_LOOP_TRACKED],
            linger_arm_last_node: [[LINGER_ARM_NO_NODE; 2]; MAX_SCOPE_SCRATCH],
            linger_arm_scope_ids: [ScopeId::generic(0); MAX_SCOPE_SCRATCH],
            linger_arm_current: [0; MAX_SCOPE_SCRATCH],
            linger_passive_arm_start: [[LINGER_ARM_NO_NODE; 2]; MAX_SCOPE_SCRATCH],
            linger_is_passive: [false; MAX_SCOPE_SCRATCH],
            jump_backpatch_indices: [0; MAX_JUMP_BACKPATCH],
            jump_backpatch_scopes: [ScopeId::generic(0); MAX_JUMP_BACKPATCH],
            jump_backpatch_kinds: [0; MAX_JUMP_BACKPATCH],
            scope_stack: [ScopeId::none(); MAX_SCOPE_SCRATCH],
            scope_stack_kinds: [ScopeKind::Generic; MAX_SCOPE_SCRATCH],
            scope_stack_entries: [0; MAX_SCOPE_SCRATCH],
            route_current_arm: [0; MAX_SCOPE_SCRATCH],
            last_step_was_scope: [false; MAX_SCOPE_SCRATCH],
            route_arm_last_node: [[StateIndex::MAX; 2]; MAX_SCOPE_SCRATCH],
            route_enter_count: [0; MAX_SCOPE_SCRATCH],
            route_passive_arm_start: [[ROUTE_PASSIVE_ARM_UNSET; 2]; MAX_SCOPE_SCRATCH],
            route_is_passive: [false; MAX_SCOPE_SCRATCH],
            route_scope_entries: [RouteScopeRecord::EMPTY; MAX_SCOPE_SCRATCH],
            dispatch_table: [(0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH],
            prefix_actions: [[PrefixAction::EMPTY; MAX_PREFIX_ACTIONS]; 2],
            prefix_lens: [0; 2],
            arm_seen_recv: [false; 2],
            scan_stack: [StateIndex::MAX; MAX_SCOPE_SCRATCH],
            visited: [false; MAX_STATES],
        }
    }

    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            let mut loop_idx = 0usize;
            while loop_idx < MAX_LOOP_TRACKED {
                core::ptr::addr_of_mut!((*dst).loop_entry_ids[loop_idx]).write(ScopeId::generic(0));
                core::ptr::addr_of_mut!((*dst).loop_entry_states[loop_idx]).write(StateIndex::MAX);
                loop_idx += 1;
            }

            let mut idx = 0usize;
            while idx < MAX_SCOPE_SCRATCH {
                core::ptr::addr_of_mut!((*dst).linger_arm_last_node[idx])
                    .write([LINGER_ARM_NO_NODE; 2]);
                core::ptr::addr_of_mut!((*dst).linger_arm_scope_ids[idx])
                    .write(ScopeId::generic(0));
                core::ptr::addr_of_mut!((*dst).linger_arm_current[idx]).write(0);
                core::ptr::addr_of_mut!((*dst).linger_passive_arm_start[idx])
                    .write([LINGER_ARM_NO_NODE; 2]);
                core::ptr::addr_of_mut!((*dst).linger_is_passive[idx]).write(false);
                core::ptr::addr_of_mut!((*dst).scope_stack[idx]).write(ScopeId::none());
                core::ptr::addr_of_mut!((*dst).scope_stack_kinds[idx]).write(ScopeKind::Generic);
                core::ptr::addr_of_mut!((*dst).scope_stack_entries[idx]).write(0);
                core::ptr::addr_of_mut!((*dst).route_current_arm[idx]).write(0);
                core::ptr::addr_of_mut!((*dst).last_step_was_scope[idx]).write(false);
                core::ptr::addr_of_mut!((*dst).route_arm_last_node[idx])
                    .write([StateIndex::MAX; 2]);
                core::ptr::addr_of_mut!((*dst).route_enter_count[idx]).write(0);
                core::ptr::addr_of_mut!((*dst).route_passive_arm_start[idx])
                    .write([ROUTE_PASSIVE_ARM_UNSET; 2]);
                core::ptr::addr_of_mut!((*dst).route_is_passive[idx]).write(false);
                idx += 1;
            }

            let mut recv_idx = 0usize;
            while recv_idx < MAX_SCOPE_SCRATCH {
                core::ptr::addr_of_mut!((*dst).route_scope_entries[recv_idx])
                    .write(RouteScopeRecord::EMPTY);
                recv_idx += 1;
            }

            let mut state_idx = 0usize;
            while state_idx < MAX_STATES {
                core::ptr::addr_of_mut!((*dst).jump_backpatch_indices[state_idx]).write(0);
                core::ptr::addr_of_mut!((*dst).jump_backpatch_scopes[state_idx])
                    .write(ScopeId::generic(0));
                core::ptr::addr_of_mut!((*dst).jump_backpatch_kinds[state_idx]).write(0);
                core::ptr::addr_of_mut!((*dst).visited[state_idx]).write(false);
                state_idx += 1;
            }

            let mut dispatch_idx = 0usize;
            while dispatch_idx < MAX_FIRST_RECV_DISPATCH {
                core::ptr::addr_of_mut!((*dst).dispatch_table[dispatch_idx]).write((
                    0,
                    0,
                    StateIndex::MAX,
                ));
                dispatch_idx += 1;
            }

            let mut prefix_idx = 0usize;
            while prefix_idx < MAX_PREFIX_ACTIONS * 2 {
                core::ptr::addr_of_mut!((*dst).prefix_actions)
                    .cast::<PrefixAction>()
                    .add(prefix_idx)
                    .write(PrefixAction::EMPTY);
                prefix_idx += 1;
            }

            core::ptr::addr_of_mut!((*dst).prefix_lens).write([0; 2]);
            core::ptr::addr_of_mut!((*dst).arm_seen_recv).write([false; 2]);

            let mut scan_idx = 0usize;
            while scan_idx < MAX_SCOPE_SCRATCH {
                core::ptr::addr_of_mut!((*dst).scan_stack[scan_idx]).write(StateIndex::MAX);
                scan_idx += 1;
            }
        }
    }
}
use crate::{
    eff::{self, EffIndex, EffStruct},
    global::{
        ControlLabelSpec, LoopControlMeaning,
        compiled::LoweringView,
        const_dsl::{EffList, PolicyMode, ScopeEvent, ScopeId, ScopeKind},
    },
};

pub(super) trait TypestateProgramView {
    fn as_slice(&self) -> &[EffStruct];
    fn scope_markers(&self) -> &[crate::global::const_dsl::ScopeMarker];
    fn policy_at(&self, offset: usize) -> Option<PolicyMode>;
    fn control_spec_at(&self, offset: usize) -> Option<ControlLabelSpec>;

    fn first_dynamic_policy_in_range(
        &self,
        scope_id: ScopeId,
        scope_start: usize,
        scope_end: usize,
    ) -> Option<(PolicyMode, usize, u8)> {
        if scope_start >= scope_end {
            return None;
        }
        let nodes = self.as_slice();
        let mut best_offset = nodes.len();
        let mut best_policy = None;
        let mut idx = scope_start;
        while idx < scope_end && idx < nodes.len() {
            if let Some(policy) = self.policy_at(idx)
                && policy.is_dynamic()
                && Self::policy_belongs_to_route_scope(scope_id, policy.scope())
                && idx < best_offset
            {
                best_offset = idx;
                best_policy = Some(policy);
            }
            idx += 1;
        }
        match best_policy {
            Some(policy) => {
                let eff_struct = nodes[best_offset];
                let tag = if matches!(eff_struct.kind, eff::EffKind::Atom) {
                    match eff_struct.atom_data().resource {
                        Some(tag) => tag,
                        None => 0,
                    }
                } else {
                    0
                };
                Some((policy, best_offset, tag))
            }
            None => None,
        }
    }

    fn policy_belongs_to_route_scope(route_scope: ScopeId, policy_scope: ScopeId) -> bool {
        if policy_scope.is_none() {
            return true;
        }
        if matches!(policy_scope.kind(), ScopeKind::Route) {
            policy_scope.raw() == route_scope.raw()
        } else {
            true
        }
    }
}

impl TypestateProgramView for LoweringView<'_> {
    #[inline(always)]
    fn as_slice(&self) -> &[EffStruct] {
        LoweringView::as_slice(self)
    }

    #[inline(always)]
    fn scope_markers(&self) -> &[crate::global::const_dsl::ScopeMarker] {
        LoweringView::scope_markers(self)
    }

    #[inline(always)]
    fn policy_at(&self, offset: usize) -> Option<PolicyMode> {
        LoweringView::policy_at(self, offset)
    }

    #[inline(always)]
    fn control_spec_at(&self, offset: usize) -> Option<ControlLabelSpec> {
        LoweringView::control_spec_at(self, offset)
    }

    #[inline(always)]
    fn first_dynamic_policy_in_range(
        &self,
        scope_id: ScopeId,
        scope_start: usize,
        scope_end: usize,
    ) -> Option<(PolicyMode, usize, u8)> {
        LoweringView::first_dynamic_policy_in_range(self, scope_id, scope_start, scope_end)
    }
}

impl TypestateProgramView for &EffList {
    #[inline(always)]
    fn as_slice(&self) -> &[EffStruct] {
        EffList::as_slice(self)
    }

    #[inline(always)]
    fn scope_markers(&self) -> &[crate::global::const_dsl::ScopeMarker] {
        EffList::scope_markers(self)
    }

    #[inline(always)]
    fn policy_at(&self, offset: usize) -> Option<PolicyMode> {
        EffList::policy_at(self, offset)
    }

    #[inline(always)]
    fn control_spec_at(&self, offset: usize) -> Option<ControlLabelSpec> {
        EffList::control_spec_at(self, offset)
    }
}

#[inline(never)]
fn clear_dispatch_table(table: &mut [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH]) {
    let mut idx = 0usize;
    while idx < MAX_FIRST_RECV_DISPATCH {
        table[idx] = (0, 0, StateIndex::MAX);
        idx += 1;
    }
}

#[inline(never)]
fn clear_prefix_actions(prefix_actions: &mut [[PrefixAction; MAX_PREFIX_ACTIONS]; 2]) {
    let mut arm = 0usize;
    while arm < 2 {
        let mut idx = 0usize;
        while idx < MAX_PREFIX_ACTIONS {
            prefix_actions[arm][idx] = PrefixAction::EMPTY;
            idx += 1;
        }
        arm += 1;
    }
}

#[inline(never)]
fn clear_scan_stack(scan_stack: &mut [StateIndex; eff::meta::MAX_EFF_NODES]) {
    let mut idx = 0usize;
    while idx < eff::meta::MAX_EFF_NODES {
        scan_stack[idx] = StateIndex::MAX;
        idx += 1;
    }
}

#[inline(never)]
fn clear_visited(visited: &mut [bool; MAX_STATES]) {
    let mut idx = 0usize;
    while idx < MAX_STATES {
        visited[idx] = false;
        idx += 1;
    }
}

#[inline(never)]
fn merge_dispatch_entry(
    nodes: &[LocalNode],
    scope_end: StateIndex,
    dispatch_table: &mut [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    dispatch_len: &mut u8,
    dispatch_functional: &mut bool,
    arm: u8,
    label: u8,
    target: StateIndex,
) {
    let mut conflict = false;
    let mut found = false;
    let mut idx = 0usize;
    while idx < *dispatch_len as usize {
        let (existing_label, existing_arm, existing_target) = dispatch_table[idx];
        if existing_label == label {
            found = true;
            let same_continuation = existing_target.raw() == target.raw()
                || continuations_equivalent(nodes, scope_end, existing_target, target);
            if same_continuation {
                if existing_arm != arm && existing_arm != ARM_SHARED {
                    dispatch_table[idx] = (label, ARM_SHARED, existing_target);
                }
            } else {
                conflict = true;
            }
            break;
        }
        idx += 1;
    }

    if conflict {
        *dispatch_functional = false;
    } else if !found {
        if *dispatch_len >= MAX_FIRST_RECV_DISPATCH as u8 {
            panic!("FIRST-recv dispatch table overflow");
        }
        dispatch_table[*dispatch_len as usize] = (label, arm, target);
        *dispatch_len += 1;
    }
}

#[inline(never)]
fn merge_nested_dispatch_entries(
    nodes: &[LocalNode],
    scope_end: StateIndex,
    scope_entries: &[ScopeRecord],
    route_scope_entries: &[RouteScopeRecord],
    scope_entries_len: usize,
    nested_ordinal: u16,
    arm: u8,
    dispatch_table: &mut [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    dispatch_len: &mut u8,
    dispatch_functional: &mut bool,
) -> bool {
    let mut nested_entry_idx = 0usize;
    while nested_entry_idx < scope_entries_len {
        if scope_entries[nested_entry_idx].scope_id.local_ordinal() == nested_ordinal {
            let nested = &route_scope_entries[nested_entry_idx];
            let mut ni = 0usize;
            while ni < nested.first_recv_len as usize {
                let (label, _nested_arm, target) = nested.first_recv_dispatch[ni];
                merge_dispatch_entry(
                    nodes,
                    scope_end,
                    dispatch_table,
                    dispatch_len,
                    dispatch_functional,
                    arm,
                    label,
                    target,
                );
                ni += 1;
            }
            return true;
        }
        nested_entry_idx += 1;
    }
    false
}

#[inline(never)]
fn finalize_route_scope_exit_for_role(
    role: u8,
    nodes: &mut [LocalNode],
    node_len: usize,
    entry_idx: usize,
    scope_entries: &mut [ScopeRecord],
    route_scope_entries: &mut [RouteScopeRecord],
    scope_entries_len: usize,
    dispatch_table: &mut [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    prefix_actions: &mut [[PrefixAction; MAX_PREFIX_ACTIONS]; 2],
    prefix_lens: &mut [usize; 2],
    arm_seen_recv: &mut [bool; 2],
    scan_stack: &mut [StateIndex; eff::meta::MAX_EFF_NODES],
    visited: &mut [bool; MAX_STATES],
) -> bool {
    let mut offer_entry_locked = false;
    let scope_id = scope_entries[entry_idx].scope_id.to_scope_id();
    let is_linger = scope_entries[entry_idx].linger;
    let is_controller = scope_entries[entry_idx].controller_role == role;
    let scope_end = as_state_index(node_len);

    if !is_linger {
        let arm0_entry = scope_entries[entry_idx].arm_entry[0];
        let arm1_entry = scope_entries[entry_idx].arm_entry[1];
        if !arm0_entry.is_max() && !arm1_entry.is_max() {
            let (prefix_end0, prefix_end1, prefix_len) =
                arm_common_prefix_end(nodes, scope_id, scope_end, arm0_entry, arm1_entry);
            if prefix_len > 0 {
                let parent_scope = if scope_entries[entry_idx].parent == SCOPE_LINK_NONE {
                    ScopeId::none()
                } else {
                    scope_entries[scope_entries[entry_idx].parent as usize]
                        .scope_id
                        .to_scope_id()
                };
                let mut arm = 0u8;
                while arm < 2 {
                    let mut steps = 0usize;
                    let mut idx = if arm == 0 { arm0_entry } else { arm1_entry };
                    while steps < prefix_len {
                        if idx.is_max() {
                            break;
                        }
                        let node_idx = state_index_to_usize(idx);
                        if node_idx >= node_len {
                            break;
                        }
                        let node = nodes[node_idx];
                        nodes[node_idx] = node.with_scope(parent_scope).with_route_arm(None);
                        let next = node.next();
                        if next.is_max() {
                            break;
                        }
                        idx = next;
                        steps += 1;
                    }
                    arm += 1;
                }

                let min_start = if prefix_end0.raw() < prefix_end1.raw() {
                    prefix_end0
                } else {
                    prefix_end1
                };
                if !min_start.is_max() {
                    scope_entries[entry_idx].start = min_start;
                }
                if is_controller {
                    scope_entries[entry_idx].arm_entry[0] = prefix_end0;
                    scope_entries[entry_idx].arm_entry[1] = prefix_end1;

                    let mut arm = 0u8;
                    while arm < 2 {
                        let entry = scope_entries[entry_idx].arm_entry[arm as usize];
                        if !entry.is_max() {
                            let node_idx = state_index_to_usize(entry);
                            if node_idx < node_len {
                                match nodes[node_idx].action() {
                                    LocalAction::Local { .. } => {}
                                    _ => {
                                        scope_entries[entry_idx].arm_entry[arm as usize] =
                                            StateIndex::MAX;
                                    }
                                }
                            } else {
                                scope_entries[entry_idx].arm_entry[arm as usize] = StateIndex::MAX;
                            }
                        }
                        arm += 1;
                    }

                    route_scope_entries[entry_idx].route_recv = [StateIndex::MAX, StateIndex::MAX];
                    route_scope_entries[entry_idx].offer_lanes = 0;
                    if prefix_end0.raw() != prefix_end1.raw() {
                        let mut arm = 0u8;
                        while arm < 2 {
                            let arm_entry = if arm == 0 { prefix_end0 } else { prefix_end1 };
                            if arm == route_scope_entries[entry_idx].route_recv_count()
                                && !arm_entry.is_max()
                            {
                                let node_idx = state_index_to_usize(arm_entry);
                                if node_idx < node_len
                                    && let LocalAction::Recv { lane, .. } = nodes[node_idx].action()
                                {
                                    route_scope_entries[entry_idx].route_recv[arm as usize] =
                                        arm_entry;
                                    route_scope_entries[entry_idx].offer_lanes |=
                                        offer_lane_bit(lane);
                                }
                            }
                            arm += 1;
                        }
                    }
                } else {
                    scope_entries[entry_idx].arm_entry[0] = prefix_end0;
                    scope_entries[entry_idx].arm_entry[1] = prefix_end1;
                }
                route_scope_entries[entry_idx].offer_entry =
                    if prefix_end0.raw() == prefix_end1.raw() {
                        prefix_end0
                    } else {
                        StateIndex::MAX
                    };
                offer_entry_locked = true;
            }
        }
    }

    if is_controller {
        clear_dispatch_table(dispatch_table);
        route_scope_entries[entry_idx].first_recv_dispatch = *dispatch_table;
        route_scope_entries[entry_idx].first_recv_len = 0;
        return offer_entry_locked;
    }

    let mut dispatch_len = 0u8;
    let mut dispatch_functional = true;
    clear_dispatch_table(dispatch_table);
    clear_prefix_actions(prefix_actions);
    *prefix_lens = [0; 2];
    *arm_seen_recv = [false; 2];

    let mut arm = 0u8;
    while arm < 2 {
        let arm_idx = arm as usize;
        let arm_entry = scope_entries[entry_idx].arm_entry[arm as usize];
        if !arm_entry.is_max() {
            clear_scan_stack(scan_stack);
            clear_visited(visited);
            let mut scan_len = 1usize;
            scan_stack[0] = arm_entry;

            while scan_len > 0 {
                scan_len -= 1;
                let scan_idx = state_index_to_usize(scan_stack[scan_len]);
                if scan_idx >= node_len {
                    arm += 1;
                    continue;
                }
                if visited[scan_idx] {
                    continue;
                }
                visited[scan_idx] = true;
                let node = nodes[scan_idx];
                let scan_scope = node.scope();
                if matches!(scan_scope.kind(), ScopeKind::Route)
                    && !scan_scope.is_none()
                    && scan_scope.local_ordinal() != scope_id.local_ordinal()
                {
                    let nested_ordinal = scan_scope.local_ordinal();
                    let _ = merge_nested_dispatch_entries(
                        nodes,
                        scope_end,
                        scope_entries,
                        route_scope_entries,
                        scope_entries_len,
                        nested_ordinal,
                        arm,
                        dispatch_table,
                        &mut dispatch_len,
                        &mut dispatch_functional,
                    );
                    continue;
                }
                match node.action() {
                    LocalAction::Recv { label, .. } => {
                        let target_idx = as_state_index(scan_idx);
                        arm_seen_recv[arm_idx] = true;
                        merge_dispatch_entry(
                            nodes,
                            scope_end,
                            dispatch_table,
                            &mut dispatch_len,
                            &mut dispatch_functional,
                            arm,
                            label,
                            target_idx,
                        );

                        let recv_scope = node.scope();
                        if matches!(recv_scope.kind(), ScopeKind::Route)
                            && !recv_scope.is_none()
                            && recv_scope.local_ordinal() != scope_id.local_ordinal()
                        {
                            let nested_ordinal = recv_scope.local_ordinal();
                            let _ = merge_nested_dispatch_entries(
                                nodes,
                                scope_end,
                                scope_entries,
                                route_scope_entries,
                                scope_entries_len,
                                nested_ordinal,
                                arm,
                                dispatch_table,
                                &mut dispatch_len,
                                &mut dispatch_functional,
                            );
                        }
                    }
                    LocalAction::Send {
                        peer, label, lane, ..
                    } => {
                        if !arm_seen_recv[arm_idx] {
                            if prefix_lens[arm_idx] >= MAX_PREFIX_ACTIONS {
                                panic!("route prefix action overflow");
                            }
                            let prefix_idx = prefix_lens[arm_idx];
                            prefix_actions[arm_idx][prefix_idx] = PrefixAction {
                                kind: PREFIX_KIND_SEND,
                                peer,
                                label,
                                lane,
                            };
                            prefix_lens[arm_idx] += 1;
                        }
                        let next_state = node.next();
                        let next_idx = state_index_to_usize(next_state);
                        let mut nested_merged = false;
                        if next_idx < node_len && next_idx != scan_idx {
                            let next_node = nodes[next_idx];
                            let next_scope = next_node.scope();
                            let current_scope = node.scope();

                            if matches!(next_scope.kind(), ScopeKind::Route)
                                && !next_scope.is_none()
                                && next_scope.local_ordinal() != current_scope.local_ordinal()
                            {
                                let nested_ordinal = next_scope.local_ordinal();
                                nested_merged = merge_nested_dispatch_entries(
                                    nodes,
                                    scope_end,
                                    scope_entries,
                                    route_scope_entries,
                                    scope_entries_len,
                                    nested_ordinal,
                                    arm,
                                    dispatch_table,
                                    &mut dispatch_len,
                                    &mut dispatch_functional,
                                );
                            }
                        }
                        if !nested_merged && !next_state.is_max() && scan_len < scan_stack.len() {
                            scan_stack[scan_len] = next_state;
                            scan_len += 1;
                        }
                    }
                    LocalAction::Local { label, lane, .. } => {
                        if !arm_seen_recv[arm_idx] {
                            if prefix_lens[arm_idx] >= MAX_PREFIX_ACTIONS {
                                panic!("route prefix action overflow");
                            }
                            let prefix_idx = prefix_lens[arm_idx];
                            prefix_actions[arm_idx][prefix_idx] = PrefixAction {
                                kind: PREFIX_KIND_LOCAL,
                                peer: role,
                                label,
                                lane,
                            };
                            prefix_lens[arm_idx] += 1;
                        }
                        let next_state = node.next();
                        let next_idx = state_index_to_usize(next_state);
                        let mut nested_merged = false;
                        if next_idx < node_len && next_idx != scan_idx {
                            let next_node = nodes[next_idx];
                            let next_scope = next_node.scope();
                            let current_scope = node.scope();

                            if matches!(next_scope.kind(), ScopeKind::Route)
                                && !next_scope.is_none()
                                && next_scope.local_ordinal() != current_scope.local_ordinal()
                            {
                                let nested_ordinal = next_scope.local_ordinal();
                                nested_merged = merge_nested_dispatch_entries(
                                    nodes,
                                    scope_end,
                                    scope_entries,
                                    route_scope_entries,
                                    scope_entries_len,
                                    nested_ordinal,
                                    arm,
                                    dispatch_table,
                                    &mut dispatch_len,
                                    &mut dispatch_functional,
                                );
                            }
                        }
                        if !nested_merged && !next_state.is_max() && scan_len < scan_stack.len() {
                            scan_stack[scan_len] = next_state;
                            scan_len += 1;
                        }
                    }
                    LocalAction::Jump {
                        reason: JumpReason::PassiveObserverBranch,
                    } => {
                        let target = node.next();
                        if !target.is_max() && scan_len < scan_stack.len() {
                            scan_stack[scan_len] = target;
                            scan_len += 1;
                        }
                    }
                    LocalAction::Jump {
                        reason:
                            JumpReason::RouteArmEnd | JumpReason::LoopContinue | JumpReason::LoopBreak,
                    } => {}
                    _ => {
                        let next_state = node.next();
                        let next_idx = state_index_to_usize(next_state);
                        let mut nested_merged = false;
                        if next_idx < node_len && next_idx != scan_idx {
                            let next_node = nodes[next_idx];
                            let next_scope = next_node.scope();
                            let current_scope = node.scope();

                            if matches!(next_scope.kind(), ScopeKind::Route)
                                && !next_scope.is_none()
                                && next_scope.local_ordinal() != current_scope.local_ordinal()
                            {
                                let nested_ordinal = next_scope.local_ordinal();
                                nested_merged = merge_nested_dispatch_entries(
                                    nodes,
                                    scope_end,
                                    scope_entries,
                                    route_scope_entries,
                                    scope_entries_len,
                                    nested_ordinal,
                                    arm,
                                    dispatch_table,
                                    &mut dispatch_len,
                                    &mut dispatch_functional,
                                );
                            }
                        }
                        if !nested_merged && !next_state.is_max() && scan_len < scan_stack.len() {
                            scan_stack[scan_len] = next_state;
                            scan_len += 1;
                        }
                    }
                }
            }
        }
        arm += 1;
    }

    let mut prefix_mismatch = false;
    if dispatch_len > 0 {
        if prefix_lens[0] != prefix_lens[1] {
            prefix_mismatch = true;
        } else {
            let mut pi = 0usize;
            while pi < prefix_lens[0] {
                if !prefix_action_eq(prefix_actions[0][pi], prefix_actions[1][pi]) {
                    prefix_mismatch = true;
                    break;
                }
                pi += 1;
            }
        }
        if prefix_mismatch {
            dispatch_functional = false;
        }
    }

    let arm0_entry = scope_entries[entry_idx].arm_entry[0];
    let arm1_entry = scope_entries[entry_idx].arm_entry[1];
    let mergeable = arm_sequences_equal(nodes, scope_end, arm0_entry, arm1_entry);

    if mergeable {
        scope_entries[entry_idx].arm_entry[1] = scope_entries[entry_idx].arm_entry[0];
        clear_dispatch_table(dispatch_table);
        route_scope_entries[entry_idx].first_recv_dispatch = *dispatch_table;
        route_scope_entries[entry_idx].first_recv_len = 0;
    } else if dispatch_functional && dispatch_len > 0 {
        route_scope_entries[entry_idx].first_recv_dispatch = *dispatch_table;
        route_scope_entries[entry_idx].first_recv_len = dispatch_len;
        let mut offer_lanes = route_scope_entries[entry_idx].offer_lanes;
        let mut di = 0u8;
        while di < dispatch_len {
            let target_idx = state_index_to_usize(dispatch_table[di as usize].2);
            if target_idx < node_len
                && let LocalAction::Recv { lane, .. } = nodes[target_idx].action()
            {
                offer_lanes |= offer_lane_bit(lane);
            }
            di += 1;
        }
        route_scope_entries[entry_idx].offer_lanes = offer_lanes;
    } else if scope_entries[entry_idx].route_policy_eff != EffIndex::MAX {
        clear_dispatch_table(dispatch_table);
        route_scope_entries[entry_idx].first_recv_dispatch = *dispatch_table;
        route_scope_entries[entry_idx].first_recv_len = 0;
    } else {
        panic!(
            "Route unprojectable for this role: arms not mergeable, wire dispatch non-deterministic, and no dynamic policy annotation provided"
        );
    }

    offer_entry_locked
}

#[inline(never)]
pub(super) unsafe fn init_role_typestate_value<P: TypestateProgramView>(
    nodes_ptr: *mut LocalNode,
    nodes_cap: usize,
    len_dst: *mut u16,
    scope_registry_dst: *mut super::registry::ScopeRegistry,
    role: u8,
    scratch: &mut RoleTypestateBuildScratch,
    scope_records: &mut [super::registry::ScopeRecord],
    scope_slots_by_scope: *mut u16,
    route_dense_by_slot: *mut u16,
    route_records: *mut RouteScopeRecord,
    route_scope_cap: usize,
    program: P,
) {
    let slice = program.as_slice();
    let scope_markers = program.scope_markers();
    let nodes = unsafe { core::slice::from_raw_parts_mut(nodes_ptr, nodes_cap) };

    let loop_entry_ids = &mut scratch.loop_entry_ids;
    let loop_entry_states = &mut scratch.loop_entry_states;
    let mut loop_entry_len = 0usize;

    // Track the last node index of each arm for linger (loop) scopes.
    // Used to insert Jump nodes at arm ends.
    // Index 0 = arm 0 (Continue), Index 1 = arm 1 (Break).
    // Use u16::MAX as sentinel for "no node yet" to distinguish from node index 0.
    // Capacity = MAX_EFF_NODES (can have at most one linger scope per effect node).
    const MAX_LINGER_ARM_TRACK: usize = eff::meta::MAX_EFF_NODES;
    const LINGER_ARM_NO_NODE: u16 = u16::MAX;
    let linger_arm_last_node = &mut scratch.linger_arm_last_node;
    let linger_arm_scope_ids = &mut scratch.linger_arm_scope_ids;
    let linger_arm_current = &mut scratch.linger_arm_current; // current arm (0 or 1)
    let mut linger_arm_len = 0usize;

    // Track passive observer arm boundaries for linger (loop) scopes.
    // When another role's self-send defines an arm, passive observers need Jump targets.
    // linger_passive_arm_start[li][arm] = node_len when arm boundary was detected.
    // This allows inserting PassiveObserverBranch Jump nodes at scope exit.
    // Use u16::MAX as sentinel for "not set" to distinguish from node_len == 0.
    const PASSIVE_ARM_UNSET: u16 = u16::MAX;
    let linger_passive_arm_start = &mut scratch.linger_passive_arm_start;
    // Flag indicating this scope has passive arm tracking (ROLE != controller).
    let linger_is_passive = &mut scratch.linger_is_passive;

    // Non-linger Route arm tracking for RouteArmEnd Jump generation.
    // Uses "Scope-as-Block" strategy: treat nested scopes as opaque blocks.
    // - last_step_was_scope[stack_idx]: true if last step was a scope exit
    // - route_arm_last_node[stack_idx][arm]: last node index for each arm

    // Backpatch list for Jump nodes that need their target resolved.
    // Records (node_index, scope, kind) where kind:
    // - 0 = loop_start (LoopContinue)
    // - 1 = scope_end (LoopBreak)
    // - 2 = scope_end (RouteArmEnd)
    // Capacity = MAX_STATES (at most one backpatch per node).
    const MAX_JUMP_BACKPATCH: usize = MAX_STATES;
    let jump_backpatch_indices = &mut scratch.jump_backpatch_indices;
    let jump_backpatch_scopes = &mut scratch.jump_backpatch_scopes;
    let jump_backpatch_kinds = &mut scratch.jump_backpatch_kinds;
    let mut jump_backpatch_len = 0usize;

    let mut node_len = 0usize;
    let mut eff_idx = 0usize;

    let mut scope_marker_idx = 0usize;
    let scope_stack = &mut scratch.scope_stack;
    let scope_stack_kinds = &mut scratch.scope_stack_kinds;
    let scope_stack_entries = &mut scratch.scope_stack_entries;
    // Track current arm number for each route scope in the stack.
    // Starts at 0 (no arm yet), incremented when a dynamic control recv is found.
    let route_current_arm = &mut scratch.route_current_arm;
    // Scope-as-Block: Track whether the last step was a scope exit (for nested route handling).
    let last_step_was_scope = &mut scratch.last_step_was_scope;
    // Scope-as-Block: Track the last node index for each arm in non-linger Route scopes.
    // route_arm_last_node[stack_idx][arm] = last node index for that arm.
    let route_arm_last_node = &mut scratch.route_arm_last_node;
    // Non-linger Route passive observer tracking using is_immediate_reenter method.
    // The arm boundary is detected via Exit→Enter pairs in ScopeEvent, not via
    // other roles' self-send messages (which passive observers don't see).
    //
    // route_enter_count[stack_idx] = number of Enter events for this scope.
    // arm number = enter_count - 1 (arm 0 at first Enter, arm 1 at second Enter).
    let route_enter_count = &mut scratch.route_enter_count;
    // route_passive_arm_start[stack_idx][arm] = node_len at arm start.
    // Use u16::MAX as sentinel for "not set".
    const ROUTE_PASSIVE_ARM_UNSET: u16 = u16::MAX;
    let route_passive_arm_start = &mut scratch.route_passive_arm_start;
    // Flag indicating this non-linger Route scope has passive tracking (ROLE != controller).
    let route_is_passive = &mut scratch.route_is_passive;
    let mut scope_stack_len = 0usize;
    let scope_entries = &mut *scope_records;
    let route_scope_entries = &mut scratch.route_scope_entries;
    let mut scope_entries_len = 0usize;
    let mut scope_range_counter: u16 = 0;

    while eff_idx <= slice.len() {
        while scope_marker_idx < scope_markers.len()
            && scope_markers[scope_marker_idx].offset == eff_idx
        {
            let marker = scope_markers[scope_marker_idx];
            let scope = marker.scope_id;
            match marker.event {
                ScopeEvent::Enter => {
                    if scope_stack_len >= eff::meta::MAX_EFF_NODES {
                        panic!("structured scope stack overflow");
                    }
                    let parent_entry = if scope_stack_len == 0 {
                        SCOPE_LINK_NONE
                    } else {
                        scope_stack_entries[scope_stack_len - 1]
                    };
                    let (entry_idx, is_new_ordinal) = alloc_scope_record(
                        scope_entries,
                        &mut scope_entries_len,
                        &mut scope_range_counter,
                        scope,
                        marker.scope_kind,
                        marker.linger,
                        parent_entry,
                        scope_stack_len,
                    );
                    scope_stack[scope_stack_len] = scope;
                    scope_stack_kinds[scope_stack_len] = marker.scope_kind;
                    scope_stack_entries[scope_stack_len] = entry_idx as u16;
                    // Initialize route tracking arrays only for NEW scope ordinals.
                    // This ensures seq(ROUTE1, ROUTE2) starts ROUTE2 at arm 0,
                    // while preserving arm count when re-entering the same route
                    // scope (e.g., different arms within the same binary route).
                    if is_new_ordinal {
                        route_current_arm[scope_stack_len] = 0;
                        route_enter_count[scope_stack_len] = 0;
                        route_passive_arm_start[scope_stack_len] =
                            [ROUTE_PASSIVE_ARM_UNSET, ROUTE_PASSIVE_ARM_UNSET];
                        route_is_passive[scope_stack_len] = false;
                        route_arm_last_node[scope_stack_len] = [StateIndex::MAX, StateIndex::MAX];
                        last_step_was_scope[scope_stack_len] = false;
                    }
                    scope_stack_len += 1;

                    // Update entry fields (short borrow scope)
                    {
                        let entry = &mut scope_entries[entry_idx];
                        if marker.linger {
                            entry.linger = true;
                        }
                        if entry.parent != SCOPE_LINK_NONE && entry.parent != parent_entry {
                            panic!("scope parent mismatch for ordinal");
                        }
                        if entry.start.is_max() {
                            entry.start = as_state_index(node_len);
                        }
                        // Propagate controller_role from ScopeMarker to ScopeEntry.
                        // This allows type-level controller detection instead of runtime inference.
                        if let Some(controller_role) = marker.controller_role
                            && entry.controller_role == CONTROLLER_ROLE_NONE
                        {
                            entry.controller_role = controller_role;
                        }
                    }

                    // Linger scope tracking for Jump insertion
                    if marker.linger && is_new_ordinal {
                        if linger_arm_len >= MAX_LINGER_ARM_TRACK {
                            panic!("linger arm tracking capacity exceeded");
                        }
                        linger_arm_scope_ids[linger_arm_len] = scope;
                        linger_arm_last_node[linger_arm_len] =
                            [LINGER_ARM_NO_NODE, LINGER_ARM_NO_NODE];
                        linger_arm_current[linger_arm_len] = 0;
                        linger_passive_arm_start[linger_arm_len] =
                            [PASSIVE_ARM_UNSET, PASSIVE_ARM_UNSET];
                        linger_is_passive[linger_arm_len] = false;
                        linger_arm_len += 1;
                    }

                    // Nested scope passive_arm_entry propagation
                    // Note: scope_stack_len was already incremented above, so the parent
                    // is at scope_stack_len - 2, not scope_stack_len - 1 (which is "self").
                    if scope_stack_len >= 2 {
                        let parent_idx = scope_stack_len - 2;
                        if matches!(scope_stack_kinds[parent_idx], ScopeKind::Route) {
                            let parent_entry_idx = scope_stack_entries[parent_idx] as usize;
                            let arm = route_current_arm[parent_idx] as usize;
                            let parent_is_passive = scope_entries[parent_entry_idx].controller_role
                                != CONTROLLER_ROLE_NONE
                                && scope_entries[parent_entry_idx].controller_role != role;
                            if arm < 2
                                && parent_is_passive
                                && scope_entries[parent_entry_idx].arm_entry[arm].is_max()
                            {
                                scope_entries[parent_entry_idx].arm_entry[arm] =
                                    as_state_index(node_len);
                            }
                        }
                    }

                    // Route arm tracking via ScopeMarker Enter events (binary route invariant)
                    if matches!(marker.scope_kind, ScopeKind::Route) {
                        let stack_idx = scope_stack_len - 1;
                        route_enter_count[stack_idx] = route_enter_count[stack_idx]
                            .checked_add(1)
                            .expect("route enter count overflow");
                        if route_enter_count[stack_idx] > 2 {
                            panic!("route must have exactly 2 arms (Enter count > 2)");
                        }
                        route_current_arm[stack_idx] = route_enter_count[stack_idx] - 1;
                        let arm = route_current_arm[stack_idx] as usize;
                        route_arm_last_node[stack_idx][arm] = StateIndex::MAX;
                        last_step_was_scope[stack_idx] = false;

                        // At first Enter (enter_count == 1), set route policy from EffList.
                        // This keeps route policy metadata independent of role projection.
                        if route_enter_count[stack_idx] == 1
                            && scope_entries[entry_idx].route_policy_eff == EffIndex::MAX
                        {
                            let scope_start = marker.offset;
                            let mut scope_end = slice.len();
                            let mut scan_idx = scope_marker_idx + 1;
                            let mut nest_depth = 1usize;
                            while scan_idx < scope_markers.len() {
                                let scan_marker = scope_markers[scan_idx];
                                if scan_marker.scope_id.local_ordinal() == scope.local_ordinal() {
                                    match scan_marker.event {
                                        ScopeEvent::Enter => nest_depth += 1,
                                        ScopeEvent::Exit => {
                                            nest_depth -= 1;
                                            if nest_depth == 0 {
                                                scope_end = scan_marker.offset;
                                                break;
                                            }
                                        }
                                    }
                                }
                                scan_idx += 1;
                            }
                            if let Some((policy, eff_offset, tag)) =
                                program.first_dynamic_policy_in_range(scope, scope_start, scope_end)
                            {
                                scope_entries[entry_idx].route_policy_id = policy
                                    .dynamic_policy_id()
                                    .expect("route policy marker must be dynamic");
                                scope_entries[entry_idx].route_policy_eff =
                                    as_eff_index(eff_offset);
                                scope_entries[entry_idx].route_policy_tag = tag;
                            }
                        }
                    }
                }
                ScopeEvent::Exit => {
                    if scope_stack_len == 0 {
                        panic!("structured scope stack underflow");
                    }
                    scope_stack_len -= 1;
                    let expected = scope_stack[scope_stack_len];
                    if expected.local_ordinal() != scope.local_ordinal() {
                        panic!("structured scope stack mismatch");
                    }
                    let entry_idx = scope_stack_entries[scope_stack_len] as usize;
                    let is_linger = scope_entries[entry_idx].linger;
                    let mut offer_entry_locked = false;

                    // Check if the next scope marker is an Enter for the same scope.
                    // If so, this is an intermediate Exit between arms in the same binary route.
                    // We need to insert arm 0's Jump HERE, not at the final Exit.
                    let next_marker_idx = scope_marker_idx + 1;
                    let is_immediate_reenter = next_marker_idx < scope_markers.len()
                        && scope_markers[next_marker_idx].offset
                            == scope_markers[scope_marker_idx].offset
                        && matches!(scope_markers[next_marker_idx].event, ScopeEvent::Enter)
                        && scope_markers[next_marker_idx].scope_id.local_ordinal()
                            == scope.local_ordinal();

                    // For linger (loop) scopes, insert Jump nodes at arm ends.
                    // We need to do this BEFORE setting scope_entries[entry_idx].end
                    // because the Jump nodes become part of the scope.
                    //
                    // With a binary route, we get multiple Exit/Enter pairs for the same scope:
                    // - Intermediate Exit (is_immediate_reenter=true): Insert arm 0's Jump
                    // - Final Exit (is_immediate_reenter=false): Insert arm 1's Jump
                    if is_linger {
                        // Find the linger tracking entry for this scope
                        let mut linger_idx = 0usize;
                        while linger_idx < linger_arm_len {
                            if linger_arm_scope_ids[linger_idx].local_ordinal()
                                == scope.local_ordinal()
                            {
                                break;
                            }
                            linger_idx += 1;
                        }

                        if linger_idx < linger_arm_len {
                            let arm_last = linger_arm_last_node[linger_idx];
                            let loop_start = scope_entries[entry_idx].start;
                            // Passive observer detection using type-level controller_role.
                            // controller_role is propagated from the route arm entry via ScopeMarker.
                            let is_passive = scope_entries[entry_idx].controller_role
                                != CONTROLLER_ROLE_NONE
                                && scope_entries[entry_idx].controller_role != role;
                            // For passive observers, use passive_arm_entry for arm start positions.
                            // passive_arm_entry tracks the first cross-role node (Send or Recv)
                            // of each arm, which is more reliable than any derived recv lookup
                            // (which only tracks Recv nodes).
                            let passive_starts = if is_passive {
                                let arm0_start = if !scope_entries[entry_idx].arm_entry[0].is_max()
                                {
                                    state_index_to_usize(scope_entries[entry_idx].arm_entry[0])
                                } else {
                                    usize::from(PASSIVE_ARM_UNSET)
                                };
                                let arm1_start = if !scope_entries[entry_idx].arm_entry[1].is_max()
                                {
                                    state_index_to_usize(scope_entries[entry_idx].arm_entry[1])
                                } else {
                                    usize::from(PASSIVE_ARM_UNSET)
                                };
                                [arm0_start, arm1_start]
                            } else {
                                [
                                    usize::from(PASSIVE_ARM_UNSET),
                                    usize::from(PASSIVE_ARM_UNSET),
                                ]
                            };

                            // At intermediate Exit: Insert Jump for arm 0 (Continue)
                            // At final Exit: Insert Jump for arm 1 (Break)
                            if is_immediate_reenter {
                                // Insert Jump for Continue arm (arm 0).
                                // For controller: LoopContinue Jump (rewinding flow)
                                // For passive observer: PassiveObserverBranch Jump (arm entry navigation)
                                if is_passive && passive_starts[0] != usize::from(PASSIVE_ARM_UNSET)
                                {
                                    // Passive observer: insert PassiveObserverBranch Jump FIRST
                                    // This takes priority because passive observers don't control
                                    // the loop - they need arm entry navigation, not rewind logic.
                                    if node_len >= MAX_STATES {
                                        panic!(
                                            "node capacity exceeded inserting PassiveObserverBranch Jump for arm 0"
                                        );
                                    }
                                    let continue_target =
                                        as_state_index(passive_starts[0] as usize);
                                    let jump_node = LocalNode::jump(
                                        continue_target,
                                        JumpReason::PassiveObserverBranch,
                                        scope,
                                        Some(scope),
                                        Some(0),
                                    );
                                    nodes[node_len] = jump_node;
                                    route_scope_entries[entry_idx].passive_arm_jump[0] =
                                        as_state_index(node_len);
                                    node_len += 1;
                                    // Also insert LoopContinue Jump if there are nodes to connect
                                    if arm_last[0] != LINGER_ARM_NO_NODE {
                                        if node_len >= MAX_STATES {
                                            panic!(
                                                "node capacity exceeded inserting LoopContinue Jump for passive"
                                            );
                                        }
                                        let jump_node = LocalNode::jump(
                                            loop_start,
                                            JumpReason::LoopContinue,
                                            scope,
                                            Some(scope),
                                            Some(0),
                                        );
                                        let prev_idx = arm_last[0] as usize;
                                        nodes[prev_idx] =
                                            nodes[prev_idx].with_next(as_state_index(node_len));
                                        nodes[node_len] = jump_node;
                                        node_len += 1;
                                    }
                                } else if arm_last[0] != LINGER_ARM_NO_NODE {
                                    // Controller: LoopContinue Jump
                                    if node_len >= MAX_STATES {
                                        panic!(
                                            "node capacity exceeded inserting LoopContinue Jump"
                                        );
                                    }
                                    // Create Jump node for LoopContinue
                                    // Target = loop_start (known at this point)
                                    let jump_node = LocalNode::jump(
                                        loop_start,
                                        JumpReason::LoopContinue,
                                        scope,
                                        Some(scope), // loop_scope is this scope
                                        Some(0),     // arm 0 = Continue
                                    );
                                    // Update the previous node's `next` to point to this Jump
                                    let prev_idx = arm_last[0] as usize;
                                    nodes[prev_idx] =
                                        nodes[prev_idx].with_next(as_state_index(node_len));
                                    nodes[node_len] = jump_node;
                                    node_len += 1;
                                } else if passive_starts[0] != usize::from(PASSIVE_ARM_UNSET) {
                                    if node_len >= MAX_STATES {
                                        panic!(
                                            "node capacity exceeded inserting PassiveObserverBranch Jump for arm 0"
                                        );
                                    }
                                    // Passive observer: insert PassiveObserverBranch Jump for arm 0
                                    // The target should be the start of arm 0's body, which is
                                    // recorded in passive_starts[0]. This is the index where
                                    // the first node of arm 0 was created (e.g., Recv BodyMsg).
                                    //
                                    // Note: We use passive_starts[0] directly instead of
                                    // find_loop_entry_state because:
                                    // 1. Passive observers have nodes inside the scope (arm body)
                                    // 2. passive_starts[0] was set when the arm boundary was
                                    //    detected, which is the position where the body starts
                                    let continue_target =
                                        as_state_index(passive_starts[0] as usize);
                                    let jump_node = LocalNode::jump(
                                        continue_target,
                                        JumpReason::PassiveObserverBranch,
                                        scope,
                                        Some(scope),
                                        Some(0),
                                    );
                                    nodes[node_len] = jump_node;
                                    route_scope_entries[entry_idx].passive_arm_jump[0] =
                                        as_state_index(node_len);
                                    node_len += 1;
                                }
                            } else {
                                // Final Exit: Insert Jump for Break arm (arm 1) if it has nodes
                                if arm_last[1] != LINGER_ARM_NO_NODE {
                                    if node_len >= MAX_STATES {
                                        panic!("node capacity exceeded inserting LoopBreak Jump");
                                    }
                                    // Create Jump node for LoopBreak
                                    // Target = scope_end (needs backpatch)
                                    let jump_node = LocalNode::jump(
                                        StateIndex::ZERO, // Sentinel, will be backpatched
                                        JumpReason::LoopBreak,
                                        scope,
                                        Some(scope), // loop_scope is this scope
                                        Some(1),     // arm 1 = Break
                                    );
                                    // Update the previous node's `next` to point to this Jump
                                    let prev_idx = arm_last[1] as usize;
                                    nodes[prev_idx] =
                                        nodes[prev_idx].with_next(as_state_index(node_len));
                                    nodes[node_len] = jump_node;
                                    // Record for backpatch
                                    if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                        panic!("jump backpatch capacity exceeded for LoopBreak");
                                    }
                                    jump_backpatch_indices[jump_backpatch_len] = node_len as u16;
                                    jump_backpatch_scopes[jump_backpatch_len] = scope;
                                    jump_backpatch_kinds[jump_backpatch_len] = 1; // scope_end
                                    jump_backpatch_len += 1;
                                    node_len += 1;
                                } else if is_passive
                                    && passive_starts[1] != usize::from(PASSIVE_ARM_UNSET)
                                {
                                    if node_len >= MAX_STATES {
                                        panic!(
                                            "node capacity exceeded inserting PassiveObserverBranch Jump for arm 1"
                                        );
                                    }
                                    // Passive observer: insert PassiveObserverBranch Jump for arm 1
                                    // Target = arm 1 body start (passive_starts[1]), similar to arm 0.
                                    // This handles protocols where the break arm has cross-role
                                    // messages for the passive observer (e.g., ExitMsg send).
                                    //
                                    // If passive_starts[1] == node_len, the break arm is EMPTY
                                    // (no cross-role content). In that case, the Jump should point
                                    // directly to scope_end (terminal), not to itself. We use
                                    // backpatch to set the target to scope_end.

                                    // Determine if the break arm has content for passive observer
                                    let arm_is_empty = passive_starts[1] as usize == node_len;

                                    // IMPORTANT: Before inserting the PassiveObserverBranch, record the
                                    // arm's last node for backpatch. This node's `next` currently points
                                    // to where we're about to insert the PassiveObserverBranch. We need
                                    // to patch it to point to scope_end instead, so that after completing
                                    // the break arm, the cursor moves to scope_end (terminal) rather than
                                    // looping back through the PassiveObserverBranch.
                                    //
                                    // The arm's last action is at (node_len - 1) because node_len is
                                    // where we're about to insert the PassiveObserverBranch.
                                    if node_len > 0 && (passive_starts[1] as usize) < node_len {
                                        let arm_last_node = node_len - 1;
                                        // Only patch if this is an actual action node (not a Jump)
                                        if !nodes[arm_last_node].action().is_jump() {
                                            if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                                panic!(
                                                    "jump backpatch capacity exceeded for arm last node"
                                                );
                                            }
                                            jump_backpatch_indices[jump_backpatch_len] =
                                                arm_last_node as u16;
                                            jump_backpatch_scopes[jump_backpatch_len] = scope;
                                            jump_backpatch_kinds[jump_backpatch_len] = 1; // scope_end
                                            jump_backpatch_len += 1;
                                        }
                                    }

                                    // Target: if arm is empty, use sentinel for backpatch to scope_end
                                    // Otherwise, use the arm body start
                                    let break_target = if arm_is_empty {
                                        StateIndex::ZERO // Sentinel, will be backpatched to scope_end
                                    } else {
                                        as_state_index(passive_starts[1] as usize)
                                    };
                                    let jump_node = LocalNode::jump(
                                        break_target,
                                        JumpReason::PassiveObserverBranch,
                                        scope,
                                        Some(scope),
                                        Some(1),
                                    );
                                    nodes[node_len] = jump_node;
                                    route_scope_entries[entry_idx].passive_arm_jump[1] =
                                        as_state_index(node_len);

                                    // If arm is empty, backpatch the Jump target to scope_end
                                    if arm_is_empty {
                                        if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                            panic!(
                                                "jump backpatch capacity exceeded for empty arm"
                                            );
                                        }
                                        jump_backpatch_indices[jump_backpatch_len] =
                                            node_len as u16;
                                        jump_backpatch_scopes[jump_backpatch_len] = scope;
                                        jump_backpatch_kinds[jump_backpatch_len] = 1; // scope_end
                                        jump_backpatch_len += 1;
                                    }

                                    node_len += 1;
                                }
                            }
                        }
                    }
                    // Non-linger Route Jump generation using is_immediate_reenter.
                    // Arm boundaries are visible via Exit→Enter pairs in ScopeEvent (generated by
                    // binary route wrapping each arm with with_scope()).
                    //
                    // CFG-pure design: arm 0 ends with RouteArmEnd Jump → scope_end, NOT fall-through to arm 1.
                    // This eliminates sequential layout dependency and runtime arm repositioning.
                    //
                    // At intermediate Exit (is_immediate_reenter=true):
                    //   - Controller: RouteArmEnd Jump → scope_end
                    //   - Passive observer: PassiveObserverBranch Jump → arm entry
                    // At final Exit (is_immediate_reenter=false):
                    //   - Passive observer: PassiveObserverBranch Jump → arm entry
                    //
                    // Passive observer detection using type-level controller_role.
                    // controller_role is propagated from the route arm entry via ScopeMarker.
                    // If controller_role matches this role, we're the controller.
                    let _is_passive_observer = scope_entries[entry_idx].controller_role
                        != CONTROLLER_ROLE_NONE
                        && scope_entries[entry_idx].controller_role != role;

                    // Generate RouteArmEnd Jump at arm 0's end (intermediate Exit).
                    // This explicitly exits arm 0 to scope_end, purifying the CFG.
                    // Both controller and passive observer roles get RouteArmEnd to ensure
                    // arm completion leads directly to scope_end without passing through
                    // PassiveObserverBranch nodes (which are decision points, not terminators).
                    if !is_linger
                        && matches!(scope_entries[entry_idx].kind, ScopeKind::Route)
                        && is_immediate_reenter
                    {
                        // For τ-eliminated arm 0 (passive observer has no nodes in arm 0),
                        // this RouteArmEnd also serves as the arm entry placeholder.
                        let arm0_is_tau_eliminated = scope_entries[entry_idx].arm_entry[0].is_max();
                        let is_passive = scope_entries[entry_idx].controller_role
                            != CONTROLLER_ROLE_NONE
                            && scope_entries[entry_idx].controller_role != role;

                        if node_len >= MAX_STATES {
                            panic!("node capacity exceeded inserting RouteArmEnd Jump for arm 0");
                        }
                        // Target is scope_end, which will be backpatched after scope closes.
                        let jump_node = LocalNode::jump(
                            StateIndex::ZERO, // Sentinel, will be backpatched to scope_end
                            JumpReason::RouteArmEnd,
                            scope,
                            None, // Not a loop
                            Some(0),
                        );
                        nodes[node_len] = jump_node;

                        // For τ-eliminated arm 0, set passive_arm_entry to this RouteArmEnd.
                        // This ensures follow_passive_observer_arm_for_scope always returns
                        // a valid entry (ArmEmpty placeholder).
                        if is_passive && arm0_is_tau_eliminated {
                            scope_entries[entry_idx].arm_entry[0] = as_state_index(node_len);
                        }

                        // Record for backpatch to scope_end
                        if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                            panic!("jump backpatch capacity exceeded for RouteArmEnd Jump");
                        }
                        jump_backpatch_indices[jump_backpatch_len] = node_len as u16;
                        jump_backpatch_scopes[jump_backpatch_len] = scope;
                        jump_backpatch_kinds[jump_backpatch_len] = 2; // scope_end via RouteArmEnd
                        jump_backpatch_len += 1;

                        node_len += 1;
                    }

                    // Generate RouteArmEnd Jump at arm 1's end (final Exit).
                    // This removes reliance on sequential layout for the last arm and
                    // ensures both arms explicitly exit to scope_end.
                    if !is_linger
                        && matches!(scope_entries[entry_idx].kind, ScopeKind::Route)
                        && !is_immediate_reenter
                    {
                        let arm1_last = route_arm_last_node[scope_stack_len][1];
                        let last_was_scope = last_step_was_scope[scope_stack_len];
                        if !arm1_last.is_max() {
                            if last_was_scope {
                                // Arm ended with a nested scope; insert RouteArmEnd at scope exit.
                                if node_len >= MAX_STATES {
                                    panic!(
                                        "node capacity exceeded inserting RouteArmEnd Jump for arm 1 (scope exit)"
                                    );
                                }
                                let jump_node = LocalNode::jump(
                                    StateIndex::ZERO, // Sentinel, will be backpatched to scope_end
                                    JumpReason::RouteArmEnd,
                                    scope,
                                    None, // Not a loop
                                    Some(1),
                                );
                                nodes[node_len] = jump_node;
                                if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                    panic!(
                                        "jump backpatch capacity exceeded for RouteArmEnd Jump (arm 1 scope exit)"
                                    );
                                }
                                jump_backpatch_indices[jump_backpatch_len] = node_len as u16;
                                jump_backpatch_scopes[jump_backpatch_len] = scope;
                                jump_backpatch_kinds[jump_backpatch_len] = 2; // scope_end via RouteArmEnd
                                jump_backpatch_len += 1;
                                node_len += 1;
                            } else {
                                if node_len >= MAX_STATES {
                                    panic!(
                                        "node capacity exceeded inserting RouteArmEnd Jump for arm 1"
                                    );
                                }
                                let jump_node = LocalNode::jump(
                                    StateIndex::ZERO, // Sentinel, will be backpatched to scope_end
                                    JumpReason::RouteArmEnd,
                                    scope,
                                    None, // Not a loop
                                    Some(1),
                                );
                                // Patch last node in arm 1 to jump to RouteArmEnd
                                let prev_idx = state_index_to_usize(arm1_last);
                                nodes[prev_idx] =
                                    nodes[prev_idx].with_next(as_state_index(node_len));
                                nodes[node_len] = jump_node;
                                if jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                                    panic!(
                                        "jump backpatch capacity exceeded for RouteArmEnd Jump (arm 1)"
                                    );
                                }
                                jump_backpatch_indices[jump_backpatch_len] = node_len as u16;
                                jump_backpatch_scopes[jump_backpatch_len] = scope;
                                jump_backpatch_kinds[jump_backpatch_len] = 2; // scope_end via RouteArmEnd
                                jump_backpatch_len += 1;
                                node_len += 1;
                            }
                        }
                    }

                    // Generate ArmEmpty placeholder for τ-eliminated arm 1 (final Exit).
                    // This ensures passive observers always have a valid arm entry,
                    // eliminating the need for runtime ScopeExited recovery.
                    //
                    // CFG-pure design: All τ-eliminated arms have ArmEmpty placeholder.
                    // For both linger (loop) and non-linger routes, passive_arm_entry must be set.
                    //
                    // Note: For non-linger routes, ArmEmpty is a RouteArmEnd Jump → scope_end.
                    // For linger routes, ArmEmpty is a LoopBreak Jump (handled differently).
                    if matches!(scope_entries[entry_idx].kind, ScopeKind::Route)
                        && !is_immediate_reenter
                    {
                        let arm1_has_content = !scope_entries[entry_idx].arm_entry[1].is_max();
                        let is_passive = scope_entries[entry_idx].controller_role
                            != CONTROLLER_ROLE_NONE
                            && scope_entries[entry_idx].controller_role != role;
                        if !arm1_has_content {
                            // τ-eliminated arm 1: insert ArmEmpty placeholder
                            if node_len >= MAX_STATES {
                                panic!(
                                    "node capacity exceeded inserting ArmEmpty placeholder for arm 1"
                                );
                            }

                            let jump_node = if is_linger {
                                // Linger scope: ArmEmpty is a LoopBreak Jump → scope start (for loop back)
                                // Actually for break arm, target is scope_end (exit loop).
                                LocalNode::jump(
                                    as_state_index(node_len + 1), // scope_end
                                    JumpReason::LoopBreak,
                                    scope,
                                    Some(scope), // loop scope
                                    Some(1),
                                )
                            } else {
                                // Non-linger: ArmEmpty is a RouteArmEnd Jump → scope_end
                                LocalNode::jump(
                                    as_state_index(node_len + 1), // scope_end
                                    JumpReason::RouteArmEnd,
                                    scope,
                                    None,
                                    Some(1),
                                )
                            };
                            nodes[node_len] = jump_node;
                            if is_passive {
                                scope_entries[entry_idx].arm_entry[1] = as_state_index(node_len);
                            }
                            node_len += 1;
                        }
                    }

                    // Scope-as-Block: Mark parent scope as "last step was a scope exit".
                    // This enables correct Jump insertion when the parent scope's arm boundary
                    // is detected - if this flag is true, we insert a Jump node at the current
                    // position (Inner.end) instead of patching the previous node's next field.
                    if scope_stack_len > 0 {
                        last_step_was_scope[scope_stack_len - 1] = true;
                    }

                    // FIRST-recv dispatch computation for Route scopes (final Exit only).
                    // Computes label → (arm, target_idx) mapping for passive observers.
                    // This enables O(1) nested route resolution in offer().
                    if matches!(scope_entries[entry_idx].kind, ScopeKind::Route)
                        && !is_immediate_reenter
                    {
                        offer_entry_locked = finalize_route_scope_exit_for_role(
                            role,
                            nodes,
                            node_len,
                            entry_idx,
                            scope_entries,
                            route_scope_entries,
                            scope_entries_len,
                            &mut scratch.dispatch_table,
                            &mut scratch.prefix_actions,
                            &mut scratch.prefix_lens,
                            &mut scratch.arm_seen_recv,
                            &mut scratch.scan_stack,
                            &mut scratch.visited,
                        );
                    }

                    if matches!(scope_entries[entry_idx].kind, ScopeKind::Route)
                        && !offer_entry_locked
                    {
                        route_scope_entries[entry_idx].offer_entry =
                            if scope_entries[entry_idx].linger {
                                StateIndex::MAX
                            } else {
                                scope_entries[entry_idx].start
                            };
                    }

                    scope_entries[entry_idx].end = as_state_index(node_len);
                }
            }
            scope_marker_idx += 1;
        }

        if eff_idx == slice.len() {
            break;
        }

        let current_scope = if scope_stack_len == 0 {
            ScopeId::none()
        } else {
            scope_stack[scope_stack_len - 1]
        };
        // Find the innermost loop scope (either ScopeKind::Loop or linger Route).
        // Linger scopes are 2-arm Routes with linger=true (like LoopContinue/LoopBreak).
        let mut loop_scope = None;
        let mut search = scope_stack_len;
        while search > 0 {
            let idx = search - 1;
            if matches!(scope_stack_kinds[idx], ScopeKind::Loop) {
                loop_scope = Some(scope_stack[idx]);
                break;
            }
            // Also check for linger Route scopes
            if matches!(scope_stack_kinds[idx], ScopeKind::Route) {
                let entry_idx = scope_stack_entries[idx] as usize;
                if scope_entries[entry_idx].linger {
                    loop_scope = Some(scope_stack[idx]);
                    break;
                }
            }
            search -= 1;
        }

        let eff = slice[eff_idx];
        if matches!(eff.kind, eff::EffKind::Atom) {
            let atom = eff.atom_data();
            let policy = match program.policy_at(eff_idx) {
                Some(policy) => policy.with_scope(current_scope),
                None => PolicyMode::Static,
            };
            let control_spec = if atom.is_control {
                program.control_spec_at(eff_idx)
            } else {
                None
            };
            let loop_control = LoopControlMeaning::from_control_spec(control_spec);
            let shot = if atom.is_control {
                match control_spec {
                    Some(spec) => Some(spec.shot),
                    None => None,
                }
            } else {
                None
            };
            if scope_stack_len > 0
                && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
            {
                let entry_idx = scope_stack_entries[scope_stack_len - 1] as usize;
                let entry = &mut scope_entries[entry_idx];
                let route_entry = &mut route_scope_entries[entry_idx];
                if let Some(policy_id) = policy.dynamic_policy_id() {
                    if entry.route_policy_eff == EffIndex::MAX {
                        entry.route_policy_id = policy_id;
                        entry.route_policy_eff = as_eff_index(eff_idx);
                        entry.route_policy_tag = match atom.resource {
                            Some(tag) => tag,
                            None => 0,
                        };
                    } else if route_policy_differs(entry.route_policy_id, policy) {
                        panic!("route scope recorded conflicting controller policy annotations");
                    }
                }
                if policy.is_dynamic() || loop_control.is_some() {
                    route_entry.offer_lanes |= offer_lane_bit(atom.lane);
                }
            }

            // Passive observer arm tracking is now handled by ScopeMarker Enter events.
            // The arm index is determined solely by route_enter_count (set in ScopeEvent::Enter).
            // Passive observer arm start positions are recorded when the first node of each
            // arm is generated (in Local/Send/Recv processing below).
            //
            // Note: We no longer need to track "other role's self-send" here because:
            // 1. All roles see the same ScopeMarker Enter/Exit events
            // 2. Arm index is route_current_arm = route_enter_count - 1 (set at Enter)
            // 3. Passive arm starts are recorded at first node generation per arm

            if atom.from == role && atom.to == role {
                // Compute route_arm for local actions (self-send).
                // Arm index is determined solely by ScopeMarker Enter count (binary route).
                // route_current_arm is set at ScopeEvent::Enter: arm = enter_count - 1.
                //
                // Note: Local nodes (self-send) are never choice determinants
                // (passive observers only see recv nodes on the wire).
                let route_arm = if scope_stack_len > 0
                    && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                {
                    let stack_idx = scope_stack_len - 1;
                    let arm = route_current_arm[stack_idx] as usize;
                    let entry_idx = scope_stack_entries[stack_idx] as usize;

                    let entry = &mut scope_entries[entry_idx];
                    debug_assert!(
                        !matches!(entry.kind, ScopeKind::Route)
                            || entry.controller_role != CONTROLLER_ROLE_NONE,
                        "route scope missing controller_role"
                    );

                    // Record arm entry for local actions.
                    // A projected role is controller-owned or passive-owned per route scope,
                    // so one shared arm-entry slot is enough here.
                    if arm < 2 {
                        if entry.arm_entry[arm].is_max() {
                            entry.arm_entry[arm] = as_state_index(node_len);
                        }
                    }

                    Some(route_current_arm[stack_idx])
                } else {
                    None
                };

                // Update the current_state after potential Jump node insertion
                let current_state = as_state_index(node_len);
                let mut next = as_state_index(node_len + 1);
                // Loop continue decisions jump back to the loop start.
                if matches!(loop_control, Some(LoopControlMeaning::Continue))
                    && let Some(scope_id) = loop_scope
                    && let Some(entry) = find_loop_entry_state(
                        &loop_entry_ids,
                        &loop_entry_states,
                        loop_entry_len,
                        scope_id,
                    )
                {
                    next = entry;
                }

                nodes[node_len] = LocalNode::local(
                    as_eff_index(eff_idx),
                    atom.label,
                    atom.resource,
                    atom.is_control,
                    shot,
                    policy,
                    atom.lane,
                    next,
                    current_scope,
                    loop_scope,
                    route_arm,
                    false, // Local nodes are never choice determinants
                );
                let lane_idx = atom.lane as usize;
                let mut stack_idx = 0usize;
                while stack_idx < scope_stack_len {
                    let entry_idx = scope_stack_entries[stack_idx] as usize;
                    let scope_entry = &mut scope_entries[entry_idx];
                    if scope_entry.lane_first_eff[lane_idx] == crate::eff::EffIndex::MAX {
                        scope_entry.lane_first_eff[lane_idx] = as_eff_index(eff_idx);
                    }
                    scope_entry.lane_last_eff[lane_idx] = as_eff_index(eff_idx);
                    if matches!(scope_stack_kinds[stack_idx], ScopeKind::Route) {
                        let arm = route_current_arm[stack_idx] as usize;
                        if arm == 0 {
                            route_scope_entries[entry_idx].arm0_lane_last_eff[lane_idx] =
                                as_eff_index(eff_idx);
                        } else if arm == 1 {
                            route_scope_entries[entry_idx].arm1_lane_mask |=
                                offer_lane_bit(atom.lane);
                        }
                    }
                    stack_idx += 1;
                }
                if let Some(scope_id) = loop_scope
                    && loop_control.is_none()
                {
                    store_loop_entry_if_absent(
                        loop_entry_ids,
                        loop_entry_states,
                        &mut loop_entry_len,
                        scope_id,
                        current_state,
                    );
                }
                // Update linger arm tracking for self-send LoopBreak.
                if let Some(scope_id) = loop_scope {
                    let mut li = 0;
                    while li < linger_arm_len {
                        if linger_arm_scope_ids[li].local_ordinal() == scope_id.local_ordinal() {
                            if matches!(loop_control, Some(LoopControlMeaning::Break)) {
                                linger_arm_current[li] = 1;
                            }
                            break;
                        }
                        li += 1;
                    }
                }
                // Update linger arm tracking for all active linger scopes (outer + inner).
                if linger_arm_len > 0 {
                    let mut stack_idx = 0usize;
                    while stack_idx < scope_stack_len {
                        let entry_idx = scope_stack_entries[stack_idx] as usize;
                        if scope_entries[entry_idx].linger {
                            let scope_id = scope_stack[stack_idx];
                            let mut li = 0usize;
                            while li < linger_arm_len {
                                if linger_arm_scope_ids[li].local_ordinal()
                                    == scope_id.local_ordinal()
                                {
                                    let arm = linger_arm_current[li] as usize;
                                    if arm < 2 {
                                        linger_arm_last_node[li][arm] = node_len as u16;
                                    }
                                    break;
                                }
                                li += 1;
                            }
                        }
                        stack_idx += 1;
                    }
                }
                // Scope-as-Block: Update non-linger Route arm tracking and reset flag.
                if scope_stack_len > 0
                    && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                {
                    let stack_idx = scope_stack_len - 1;
                    let entry_idx = scope_stack_entries[stack_idx] as usize;
                    if !scope_entries[entry_idx].linger {
                        // Reset "last step was scope" flag
                        last_step_was_scope[stack_idx] = false;
                        // Track last node for current arm
                        if let Some(arm) = route_arm {
                            if (arm as usize) < 2 {
                                route_arm_last_node[stack_idx][arm as usize] =
                                    as_state_index(node_len);
                            }
                        }
                    }
                }
                node_len += 1;
            } else if atom.from == role {
                // Compute route_arm for send nodes inside a route scope.
                // This is needed for linger rewind logic to distinguish arms.
                //
                // Arm index is determined solely by ScopeMarker Enter count (binary route).
                // route_current_arm is set at ScopeEvent::Enter: arm = enter_count - 1.
                //
                // Note: Send nodes are never choice determinants (passive observers
                // only see recv nodes on the wire).
                let route_arm = if scope_stack_len > 0
                    && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                {
                    let stack_idx = scope_stack_len - 1;
                    let arm = route_current_arm[stack_idx];
                    let entry_idx = scope_stack_entries[stack_idx] as usize;
                    let is_passive = scope_entries[entry_idx].controller_role
                        != CONTROLLER_ROLE_NONE
                        && scope_entries[entry_idx].controller_role != role;

                    // Passive observers need the first cross-role send as the arm entry
                    // when an arm has no earlier local entry or recv.
                    if (arm as usize) < 2
                        && is_passive
                        && scope_entries[entry_idx].arm_entry[arm as usize].is_max()
                    {
                        scope_entries[entry_idx].arm_entry[arm as usize] = as_state_index(node_len);
                    }

                    Some(arm)
                } else {
                    None
                };

                // Update the current_state after potential Jump node insertion
                let current_state = as_state_index(node_len);
                let mut next = as_state_index(node_len + 1);
                // Loop continue decisions jump back to the loop start.
                if matches!(loop_control, Some(LoopControlMeaning::Continue))
                    && let Some(scope_id) = loop_scope
                    && let Some(entry) = find_loop_entry_state(
                        &loop_entry_ids,
                        &loop_entry_states,
                        loop_entry_len,
                        scope_id,
                    )
                {
                    next = entry;
                }

                nodes[node_len] = LocalNode::send(
                    as_eff_index(eff_idx),
                    atom.to,
                    atom.label,
                    atom.resource,
                    atom.is_control,
                    shot,
                    policy,
                    atom.lane,
                    next,
                    current_scope,
                    loop_scope,
                    route_arm,
                    false, // Send nodes are never choice determinants
                );
                let lane_idx = atom.lane as usize;
                let mut stack_idx = 0usize;
                while stack_idx < scope_stack_len {
                    let entry_idx = scope_stack_entries[stack_idx] as usize;
                    let scope_entry = &mut scope_entries[entry_idx];
                    if scope_entry.lane_first_eff[lane_idx] == crate::eff::EffIndex::MAX {
                        scope_entry.lane_first_eff[lane_idx] = as_eff_index(eff_idx);
                    }
                    scope_entry.lane_last_eff[lane_idx] = as_eff_index(eff_idx);
                    if matches!(scope_stack_kinds[stack_idx], ScopeKind::Route) {
                        let arm = route_current_arm[stack_idx] as usize;
                        if arm == 0 {
                            route_scope_entries[entry_idx].arm0_lane_last_eff[lane_idx] =
                                as_eff_index(eff_idx);
                        } else if arm == 1 {
                            route_scope_entries[entry_idx].arm1_lane_mask |=
                                offer_lane_bit(atom.lane);
                        }
                    }
                    stack_idx += 1;
                }
                if let Some(scope_id) = loop_scope
                    && loop_control.is_none()
                {
                    store_loop_entry_if_absent(
                        loop_entry_ids,
                        loop_entry_states,
                        &mut loop_entry_len,
                        scope_id,
                        current_state,
                    );
                }
                // Update linger arm tracking for all active linger scopes (outer + inner).
                if linger_arm_len > 0 {
                    let mut stack_idx = 0usize;
                    while stack_idx < scope_stack_len {
                        let entry_idx = scope_stack_entries[stack_idx] as usize;
                        if scope_entries[entry_idx].linger {
                            let scope_id = scope_stack[stack_idx];
                            let mut li = 0usize;
                            while li < linger_arm_len {
                                if linger_arm_scope_ids[li].local_ordinal()
                                    == scope_id.local_ordinal()
                                {
                                    let arm = linger_arm_current[li] as usize;
                                    if arm < 2 {
                                        linger_arm_last_node[li][arm] = node_len as u16;
                                    }
                                    break;
                                }
                                li += 1;
                            }
                        }
                        stack_idx += 1;
                    }
                }
                // Scope-as-Block: Update non-linger Route arm tracking and reset flag.
                if scope_stack_len > 0
                    && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                {
                    let stack_idx = scope_stack_len - 1;
                    let entry_idx = scope_stack_entries[stack_idx] as usize;
                    if !scope_entries[entry_idx].linger {
                        // Reset "last step was scope" flag
                        last_step_was_scope[stack_idx] = false;
                        // Track last node for current arm
                        if let Some(arm) = route_arm {
                            if (arm as usize) < 2 {
                                route_arm_last_node[stack_idx][arm as usize] =
                                    as_state_index(node_len);
                            }
                        }
                    }
                }
                node_len += 1;
            } else if atom.to == role {
                // Determine route_arm and is_choice_determinant for this recv node.
                // Arm index is determined solely by ScopeMarker Enter count (binary route).
                // route_current_arm is set at ScopeEvent::Enter: arm = enter_count - 1.
                //
                // is_choice_determinant: The first recv of each arm is a choice determinant
                // for passive observer mode (allows label-based arm resolution).
                let (route_arm, is_choice_determinant) = if scope_stack_len > 0
                    && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                {
                    let stack_idx = scope_stack_len - 1;
                    let arm = route_current_arm[stack_idx];
                    let entry_idx = scope_stack_entries[stack_idx] as usize;
                    let entry = &mut scope_entries[entry_idx];
                    let route_entry = &mut route_scope_entries[entry_idx];
                    let is_passive = entry.controller_role != CONTROLLER_ROLE_NONE
                        && entry.controller_role != role;

                    // Passive observers use the first recv when it is the first cross-role
                    // node, or when it must replace an earlier local/send placeholder.
                    if (arm as usize) < 2 && is_passive {
                        let existing = entry.arm_entry[arm as usize];
                        let should_set = if existing.is_max() {
                            true
                        } else {
                            let existing_node = nodes[state_index_to_usize(existing)];
                            !matches!(existing_node.action(), LocalAction::Recv { .. })
                        };
                        if should_set {
                            entry.arm_entry[arm as usize] = as_state_index(node_len);
                        }
                    }

                    // Check if this is the first recv for this arm in this scope.
                    // For binary routes, recv registration stays contiguous:
                    // arm 0 may register first, then arm 1 may register second.
                    let is_first_recv_of_arm = arm == route_entry.route_recv_count();

                    if is_first_recv_of_arm && (arm as usize) < 2 {
                        let current_state = as_state_index(node_len);
                        route_entry.route_recv[arm as usize] = current_state;
                        route_entry.offer_lanes |= offer_lane_bit(atom.lane);
                        (Some(arm), true) // First recv of arm = choice determinant
                    } else {
                        // Subsequent recv within the same arm - not a choice determinant
                        (Some(arm), false)
                    }
                } else {
                    (None, false)
                };

                // Update the current_state after potential Jump node insertion
                let current_state = as_state_index(node_len);
                let mut next = as_state_index(node_len + 1);
                // Loop continue decisions jump back to the loop start.
                if matches!(loop_control, Some(LoopControlMeaning::Continue))
                    && let Some(scope_id) = loop_scope
                    && let Some(entry) = find_loop_entry_state(
                        &loop_entry_ids,
                        &loop_entry_states,
                        loop_entry_len,
                        scope_id,
                    )
                {
                    next = entry;
                }

                nodes[node_len] = LocalNode::recv(
                    as_eff_index(eff_idx),
                    atom.from,
                    atom.label,
                    atom.resource,
                    atom.is_control,
                    shot,
                    policy,
                    atom.lane,
                    next,
                    current_scope,
                    loop_scope,
                    route_arm,
                    is_choice_determinant,
                );
                let lane_idx = atom.lane as usize;
                let mut stack_idx = 0usize;
                while stack_idx < scope_stack_len {
                    let entry_idx = scope_stack_entries[stack_idx] as usize;
                    let scope_entry = &mut scope_entries[entry_idx];
                    if scope_entry.lane_first_eff[lane_idx] == crate::eff::EffIndex::MAX {
                        scope_entry.lane_first_eff[lane_idx] = as_eff_index(eff_idx);
                    }
                    scope_entry.lane_last_eff[lane_idx] = as_eff_index(eff_idx);
                    if matches!(scope_stack_kinds[stack_idx], ScopeKind::Route) {
                        let arm = route_current_arm[stack_idx] as usize;
                        if arm == 0 {
                            route_scope_entries[entry_idx].arm0_lane_last_eff[lane_idx] =
                                as_eff_index(eff_idx);
                        } else if arm == 1 {
                            route_scope_entries[entry_idx].arm1_lane_mask |=
                                offer_lane_bit(atom.lane);
                        }
                    }
                    stack_idx += 1;
                }
                if let Some(scope_id) = loop_scope
                    && loop_control.is_none()
                {
                    store_loop_entry_if_absent(
                        loop_entry_ids,
                        loop_entry_states,
                        &mut loop_entry_len,
                        scope_id,
                        current_state,
                    );
                }
                // Update linger arm tracking for all active linger scopes (outer + inner).
                if linger_arm_len > 0 {
                    let mut stack_idx = 0usize;
                    while stack_idx < scope_stack_len {
                        let entry_idx = scope_stack_entries[stack_idx] as usize;
                        if scope_entries[entry_idx].linger {
                            let scope_id = scope_stack[stack_idx];
                            let mut li = 0usize;
                            while li < linger_arm_len {
                                if linger_arm_scope_ids[li].local_ordinal()
                                    == scope_id.local_ordinal()
                                {
                                    let arm = linger_arm_current[li] as usize;
                                    if arm < 2 {
                                        linger_arm_last_node[li][arm] = node_len as u16;
                                    }
                                    break;
                                }
                                li += 1;
                            }
                        }
                        stack_idx += 1;
                    }
                }
                // Scope-as-Block: Update non-linger Route arm tracking and reset flag.
                if scope_stack_len > 0
                    && matches!(scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
                {
                    let stack_idx = scope_stack_len - 1;
                    let entry_idx = scope_stack_entries[stack_idx] as usize;
                    if !scope_entries[entry_idx].linger {
                        // Reset "last step was scope" flag
                        last_step_was_scope[stack_idx] = false;
                        // Track last node for current arm
                        if let Some(arm) = route_arm {
                            if (arm as usize) < 2 {
                                route_arm_last_node[stack_idx][arm as usize] =
                                    as_state_index(node_len);
                            }
                        }
                    }
                }
                node_len += 1;
            }
        }
        eff_idx += 1;
    }

    if scope_stack_len != 0 {
        panic!("unbalanced structured scope markers");
    }

    if node_len >= MAX_STATES {
        panic!("typestate capacity exceeded for role");
    }

    // Apply backpatches for Jump nodes.
    // Jump targets that were unknown at node creation time now have their
    // destinations resolved.
    {
        let mut bi = 0;
        while bi < jump_backpatch_len {
            let node_idx = jump_backpatch_indices[bi] as usize;
            let scope = jump_backpatch_scopes[bi];
            let kind = jump_backpatch_kinds[bi];

            // Find the scope entry for this scope
            let target_raw = scope.canonical().raw();
            let mut entry_idx = None;
            let mut scope_entry_idx = 0usize;
            while scope_entry_idx < scope_entries_len {
                let entry = &scope_entries[scope_entry_idx];
                if entry.scope_id.canonical().raw() == target_raw {
                    entry_idx = Some(scope_entry_idx);
                    break;
                }
                scope_entry_idx += 1;
            }

            let Some(entry_idx) = entry_idx else {
                panic!("jump backpatch failed: canonical scope id not found");
            };
            let entry = &scope_entries[entry_idx];
            let target = if kind == 1 || kind == 2 {
                // scope_end target for LoopBreak Jump (kind=1) or RouteArmEnd (kind=2)
                entry.end
            } else {
                // loop_start target for LoopContinue Jump (kind=0)
                entry.start
            };
            nodes[node_idx] = nodes[node_idx].with_next(target);

            bi += 1;
        }
    }

    let terminal_index = as_state_index(node_len);
    nodes[node_len] = LocalNode::terminal(terminal_index);
    unsafe {
        len_dst.write(encode_typestate_len(node_len + 1));
        init_scope_registry(
            scope_registry_dst,
            scope_entries.as_mut_ptr(),
            scope_slots_by_scope,
            route_dense_by_slot,
            route_records,
            route_scope_cap,
            route_scope_entries.as_mut_ptr(),
            scope_entries_len,
        );
    }
}
