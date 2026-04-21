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
        CONTROLLER_ROLE_NONE, RouteScopeScratchRecord, SCOPE_LINK_NONE, ScopeRecord,
        insert_offer_lane,
    },
    route_facts::{
        MAX_PREFIX_ACTIONS, PREFIX_KIND_LOCAL, PREFIX_KIND_SEND, PrefixAction,
        arm_common_prefix_end, arm_sequences_equal, continuations_equivalent, prefix_action_eq,
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
    pub(super) scope_controller_roles: [u8; MAX_SCOPE_SCRATCH],
    pub(super) scope_route_policy_tags: [u8; MAX_SCOPE_SCRATCH],
    pub(super) scope_route_policy_ids: [u16; MAX_SCOPE_SCRATCH],
    pub(super) scope_route_policy_effs: [EffIndex; MAX_SCOPE_SCRATCH],
    pub(super) last_step_was_scope: [bool; MAX_SCOPE_SCRATCH],
    pub(super) route_arm_last_node: [[StateIndex; 2]; MAX_SCOPE_SCRATCH],
    pub(super) route_enter_count: [u8; MAX_SCOPE_SCRATCH],
    pub(super) route_passive_arm_start: [[u16; 2]; MAX_SCOPE_SCRATCH],
    pub(super) route_is_passive: [bool; MAX_SCOPE_SCRATCH],
    pub(super) route_scope_entries: [RouteScopeScratchRecord; MAX_SCOPE_SCRATCH],
    pub(super) dispatch_table: [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    pub(super) prefix_actions: [[PrefixAction; MAX_PREFIX_ACTIONS]; 2],
    pub(super) prefix_lens: [usize; 2],
    pub(super) arm_seen_recv: [bool; 2],
    pub(super) scan_stack: [StateIndex; MAX_SCOPE_SCRATCH],
    pub(super) visited: [bool; MAX_STATES],
}

impl RoleTypestateBuildScratch {
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
                core::ptr::addr_of_mut!((*dst).scope_controller_roles[idx])
                    .write(CONTROLLER_ROLE_NONE);
                core::ptr::addr_of_mut!((*dst).scope_route_policy_tags[idx]).write(0);
                core::ptr::addr_of_mut!((*dst).scope_route_policy_ids[idx]).write(u16::MAX);
                core::ptr::addr_of_mut!((*dst).scope_route_policy_effs[idx]).write(EffIndex::MAX);
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
                    .write(RouteScopeScratchRecord::EMPTY);
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
        LoopControlMeaning, StaticControlDesc,
        compiled::{images::ControlSemanticKind, lowering::LoweringView},
        const_dsl::{PolicyMode, ScopeEvent, ScopeId, ScopeKind},
        role_program::LaneWord,
    },
};

pub(super) trait TypestateProgramView {
    fn as_slice(&self) -> &[EffStruct];
    fn scope_markers(&self) -> &[crate::global::const_dsl::ScopeMarker];
    fn policy_at(&self, offset: usize) -> Option<PolicyMode>;
    fn control_spec_at(&self, offset: usize) -> Option<StaticControlDesc>;
    fn first_route_head_dynamic_policy_in_range(
        &self,
        scope_id: ScopeId,
        route_enter_marker_idx: usize,
        scope_end: usize,
    ) -> Option<(PolicyMode, usize, u8, crate::control::cap::mint::ControlOp)>;
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
    fn control_spec_at(&self, offset: usize) -> Option<StaticControlDesc> {
        LoweringView::control_spec_at(self, offset)
    }

    #[inline(always)]
    fn first_route_head_dynamic_policy_in_range(
        &self,
        scope_id: ScopeId,
        route_enter_marker_idx: usize,
        scope_end: usize,
    ) -> Option<(PolicyMode, usize, u8, crate::control::cap::mint::ControlOp)> {
        LoweringView::first_route_head_dynamic_policy_in_range(
            self,
            scope_id,
            route_enter_marker_idx,
            scope_end,
        )
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

#[inline(always)]
fn dispatch_label_bit(label: u8) -> u128 {
    if label < u128::BITS as u8 {
        1u128 << label
    } else {
        0
    }
}

#[inline(always)]
fn dispatch_lane_bit(lane: u8) -> u8 {
    if lane < u8::BITS as u8 {
        1u8 << lane
    } else {
        0
    }
}

#[inline(never)]
fn sort_dispatch_table(
    dispatch_table: &mut [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    dispatch_len: u8,
) {
    let len = dispatch_len as usize;
    let mut idx = 1usize;
    while idx < len {
        let entry = dispatch_table[idx];
        let mut scan = idx;
        while scan > 0 && dispatch_table[scan - 1].0 > entry.0 {
            dispatch_table[scan] = dispatch_table[scan - 1];
            scan -= 1;
        }
        dispatch_table[scan] = entry;
        idx += 1;
    }
}

#[inline(never)]
fn store_dispatch_summary(
    nodes: &[LocalNode],
    route_entry: &mut RouteScopeScratchRecord,
    dispatch_table: &mut [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    dispatch_len: u8,
) -> u8 {
    sort_dispatch_table(dispatch_table, dispatch_len);
    route_entry.first_recv_dispatch = *dispatch_table;
    route_entry.first_recv_len = dispatch_len;
    route_entry.first_recv_label_mask = 0;
    route_entry.first_recv_dispatch_label_mask = [0; 2];
    route_entry.first_recv_dispatch_arm_mask = 0;
    route_entry.first_recv_dispatch_lane_mask = [0; 2];

    let mut offer_lane_mask = 0u8;
    let mut idx = 0usize;
    while idx < dispatch_len as usize {
        let (label, arm, target) = dispatch_table[idx];
        let label_bit = dispatch_label_bit(label);
        route_entry.first_recv_label_mask |= label_bit;

        let target_idx = state_index_to_usize(target);
        let lane_bit = if target_idx < nodes.len() {
            match nodes[target_idx].action() {
                LocalAction::Recv { lane, .. } => dispatch_lane_bit(lane),
                _ => 0,
            }
        } else {
            0
        };

        offer_lane_mask |= lane_bit;
        if arm == ARM_SHARED {
            route_entry.first_recv_dispatch_arm_mask |= 0b11;
            route_entry.first_recv_dispatch_lane_mask[0] |= lane_bit;
            route_entry.first_recv_dispatch_lane_mask[1] |= lane_bit;
        } else if arm < 2 {
            let arm_idx = arm as usize;
            route_entry.first_recv_dispatch_arm_mask |= 1u8 << arm_idx;
            route_entry.first_recv_dispatch_label_mask[arm_idx] |= label_bit;
            route_entry.first_recv_dispatch_lane_mask[arm_idx] |= lane_bit;
        }
        idx += 1;
    }

    offer_lane_mask
}

#[inline(always)]
fn insert_offer_lane_mask(words: &mut [LaneWord], lane_mask: u8) {
    let mut lane = 0u8;
    while lane < u8::BITS as u8 {
        if (lane_mask & (1u8 << lane)) != 0 {
            insert_offer_lane(words, lane);
        }
        lane += 1;
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

#[inline(always)]
fn route_scope_lane_words_mut(
    lane_words: &mut [LaneWord],
    lane_word_start: usize,
    lane_word_len: usize,
) -> &mut [LaneWord] {
    &mut lane_words[lane_word_start..lane_word_start + lane_word_len]
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
    route_scope_entries: &[RouteScopeScratchRecord],
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

struct RouteFinalizeCtx<'a> {
    nodes: &'a mut [LocalNode],
    scope_entries: &'a mut [ScopeRecord],
    scope_controller_roles: &'a [u8; MAX_SCOPE_SCRATCH],
    scope_route_policy_effs: &'a [EffIndex; MAX_SCOPE_SCRATCH],
    route_scope_entries: &'a mut [RouteScopeScratchRecord],
    route_scope_offer_lane_words: &'a mut [LaneWord],
    route_lane_word_len: usize,
    scope_entries_len: usize,
    dispatch_table: &'a mut [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    prefix_actions: &'a mut [[PrefixAction; MAX_PREFIX_ACTIONS]; 2],
    prefix_lens: &'a mut [usize; 2],
    arm_seen_recv: &'a mut [bool; 2],
    scan_stack: &'a mut [StateIndex; eff::meta::MAX_EFF_NODES],
    visited: &'a mut [bool; MAX_STATES],
}

struct RoleWalkCtx<'a> {
    nodes: &'a mut [LocalNode],
    scope_entries: &'a mut [ScopeRecord],
    route_scope_entries: &'a mut [RouteScopeScratchRecord],
    route_scope_offer_lane_words: &'a mut [LaneWord],
    route_scope_arm1_lane_words: &'a mut [LaneWord],
    route_lane_word_len: usize,
    lane_slot_count: usize,
    scope_lane_first_eff: &'a mut [EffIndex],
    scope_lane_last_eff: &'a mut [EffIndex],
    route_arm0_lane_last_eff_by_slot: &'a mut [EffIndex],
    loop_entry_ids: &'a mut [ScopeId; MAX_LOOP_TRACKED],
    loop_entry_states: &'a mut [StateIndex; MAX_LOOP_TRACKED],
    linger_arm_scope_ids: &'a mut [ScopeId; MAX_SCOPE_SCRATCH],
    linger_arm_current: &'a mut [u8; MAX_SCOPE_SCRATCH],
    linger_arm_last_node: &'a mut [[u16; 2]; MAX_SCOPE_SCRATCH],
    jump_backpatch_indices: &'a mut [u16; MAX_JUMP_BACKPATCH],
    jump_backpatch_scopes: &'a mut [ScopeId; MAX_JUMP_BACKPATCH],
    jump_backpatch_kinds: &'a mut [u8; MAX_JUMP_BACKPATCH],
    scope_stack: &'a mut [ScopeId; MAX_SCOPE_SCRATCH],
    scope_stack_kinds: &'a mut [ScopeKind; MAX_SCOPE_SCRATCH],
    scope_stack_entries: &'a mut [u16; MAX_SCOPE_SCRATCH],
    route_current_arm: &'a mut [u8; MAX_SCOPE_SCRATCH],
    scope_controller_roles: &'a mut [u8; MAX_SCOPE_SCRATCH],
    scope_route_policy_effs: &'a mut [EffIndex; MAX_SCOPE_SCRATCH],
    last_step_was_scope: &'a mut [bool; MAX_SCOPE_SCRATCH],
    route_arm_last_node: &'a mut [[StateIndex; 2]; MAX_SCOPE_SCRATCH],
    dispatch_table: &'a mut [(u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    prefix_actions: &'a mut [[PrefixAction; MAX_PREFIX_ACTIONS]; 2],
    prefix_lens: &'a mut [usize; 2],
    arm_seen_recv: &'a mut [bool; 2],
    scan_stack: &'a mut [StateIndex; eff::meta::MAX_EFF_NODES],
    visited: &'a mut [bool; MAX_STATES],
}

#[derive(Clone, Copy)]
struct RouteDispatchOutcome {
    dispatch_len: u8,
    dispatch_functional: bool,
}

#[inline(never)]
fn collect_route_dispatch_for_exit(
    ctx: &mut RouteFinalizeCtx<'_>,
    role: u8,
    node_len: usize,
    entry_idx: usize,
    scope_id: ScopeId,
    scope_end: StateIndex,
) -> RouteDispatchOutcome {
    let mut dispatch_len = 0u8;
    let mut dispatch_functional = true;
    clear_dispatch_table(ctx.dispatch_table);
    clear_prefix_actions(ctx.prefix_actions);
    *ctx.prefix_lens = [0; 2];
    *ctx.arm_seen_recv = [false; 2];

    let mut arm = 0u8;
    while arm < 2 {
        let arm_idx = arm as usize;
        let arm_entry = ctx.scope_entries[entry_idx].arm_entry[arm_idx];
        if !arm_entry.is_max() {
            clear_scan_stack(ctx.scan_stack);
            clear_visited(ctx.visited);
            let mut scan_len = 1usize;
            ctx.scan_stack[0] = arm_entry;

            while scan_len > 0 {
                scan_len -= 1;
                let scan_idx = state_index_to_usize(ctx.scan_stack[scan_len]);
                if scan_idx >= node_len {
                    arm += 1;
                    continue;
                }
                if ctx.visited[scan_idx] {
                    continue;
                }
                ctx.visited[scan_idx] = true;
                let node = ctx.nodes[scan_idx];
                let scan_scope = node.scope();
                if matches!(scan_scope.kind(), ScopeKind::Route)
                    && !scan_scope.is_none()
                    && scan_scope.local_ordinal() != scope_id.local_ordinal()
                {
                    let nested_ordinal = scan_scope.local_ordinal();
                    let _ = merge_nested_dispatch_entries(
                        ctx.nodes,
                        scope_end,
                        ctx.scope_entries,
                        ctx.route_scope_entries,
                        ctx.scope_entries_len,
                        nested_ordinal,
                        arm,
                        ctx.dispatch_table,
                        &mut dispatch_len,
                        &mut dispatch_functional,
                    );
                    continue;
                }
                match node.action() {
                    LocalAction::Recv { label, .. } => {
                        let target_idx = as_state_index(scan_idx);
                        ctx.arm_seen_recv[arm_idx] = true;
                        merge_dispatch_entry(
                            ctx.nodes,
                            scope_end,
                            ctx.dispatch_table,
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
                                ctx.nodes,
                                scope_end,
                                ctx.scope_entries,
                                ctx.route_scope_entries,
                                ctx.scope_entries_len,
                                nested_ordinal,
                                arm,
                                ctx.dispatch_table,
                                &mut dispatch_len,
                                &mut dispatch_functional,
                            );
                        }
                    }
                    LocalAction::Send {
                        peer, label, lane, ..
                    } => {
                        if !ctx.arm_seen_recv[arm_idx] {
                            if ctx.prefix_lens[arm_idx] >= MAX_PREFIX_ACTIONS {
                                panic!("route prefix action overflow");
                            }
                            let prefix_idx = ctx.prefix_lens[arm_idx];
                            ctx.prefix_actions[arm_idx][prefix_idx] = PrefixAction {
                                kind: PREFIX_KIND_SEND,
                                peer,
                                label,
                                lane,
                            };
                            ctx.prefix_lens[arm_idx] += 1;
                        }
                        let next_state = node.next();
                        let next_idx = state_index_to_usize(next_state);
                        let mut nested_merged = false;
                        if next_idx < node_len && next_idx != scan_idx {
                            let next_node = ctx.nodes[next_idx];
                            let next_scope = next_node.scope();
                            let current_scope = node.scope();

                            if matches!(next_scope.kind(), ScopeKind::Route)
                                && !next_scope.is_none()
                                && next_scope.local_ordinal() != current_scope.local_ordinal()
                            {
                                let nested_ordinal = next_scope.local_ordinal();
                                nested_merged = merge_nested_dispatch_entries(
                                    ctx.nodes,
                                    scope_end,
                                    ctx.scope_entries,
                                    ctx.route_scope_entries,
                                    ctx.scope_entries_len,
                                    nested_ordinal,
                                    arm,
                                    ctx.dispatch_table,
                                    &mut dispatch_len,
                                    &mut dispatch_functional,
                                );
                            }
                        }
                        if !nested_merged && !next_state.is_max() && scan_len < ctx.scan_stack.len()
                        {
                            ctx.scan_stack[scan_len] = next_state;
                            scan_len += 1;
                        }
                    }
                    LocalAction::Local { label, lane, .. } => {
                        if !ctx.arm_seen_recv[arm_idx] {
                            if ctx.prefix_lens[arm_idx] >= MAX_PREFIX_ACTIONS {
                                panic!("route prefix action overflow");
                            }
                            let prefix_idx = ctx.prefix_lens[arm_idx];
                            ctx.prefix_actions[arm_idx][prefix_idx] = PrefixAction {
                                kind: PREFIX_KIND_LOCAL,
                                peer: role,
                                label,
                                lane,
                            };
                            ctx.prefix_lens[arm_idx] += 1;
                        }
                        let next_state = node.next();
                        let next_idx = state_index_to_usize(next_state);
                        let mut nested_merged = false;
                        if next_idx < node_len && next_idx != scan_idx {
                            let next_node = ctx.nodes[next_idx];
                            let next_scope = next_node.scope();
                            let current_scope = node.scope();

                            if matches!(next_scope.kind(), ScopeKind::Route)
                                && !next_scope.is_none()
                                && next_scope.local_ordinal() != current_scope.local_ordinal()
                            {
                                let nested_ordinal = next_scope.local_ordinal();
                                nested_merged = merge_nested_dispatch_entries(
                                    ctx.nodes,
                                    scope_end,
                                    ctx.scope_entries,
                                    ctx.route_scope_entries,
                                    ctx.scope_entries_len,
                                    nested_ordinal,
                                    arm,
                                    ctx.dispatch_table,
                                    &mut dispatch_len,
                                    &mut dispatch_functional,
                                );
                            }
                        }
                        if !nested_merged && !next_state.is_max() && scan_len < ctx.scan_stack.len()
                        {
                            ctx.scan_stack[scan_len] = next_state;
                            scan_len += 1;
                        }
                    }
                    LocalAction::Jump {
                        reason: JumpReason::PassiveObserverBranch,
                    } => {
                        let target = node.next();
                        if !target.is_max() && scan_len < ctx.scan_stack.len() {
                            ctx.scan_stack[scan_len] = target;
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
                            let next_node = ctx.nodes[next_idx];
                            let next_scope = next_node.scope();
                            let current_scope = node.scope();

                            if matches!(next_scope.kind(), ScopeKind::Route)
                                && !next_scope.is_none()
                                && next_scope.local_ordinal() != current_scope.local_ordinal()
                            {
                                let nested_ordinal = next_scope.local_ordinal();
                                nested_merged = merge_nested_dispatch_entries(
                                    ctx.nodes,
                                    scope_end,
                                    ctx.scope_entries,
                                    ctx.route_scope_entries,
                                    ctx.scope_entries_len,
                                    nested_ordinal,
                                    arm,
                                    ctx.dispatch_table,
                                    &mut dispatch_len,
                                    &mut dispatch_functional,
                                );
                            }
                        }
                        if !nested_merged && !next_state.is_max() && scan_len < ctx.scan_stack.len()
                        {
                            ctx.scan_stack[scan_len] = next_state;
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
        if ctx.prefix_lens[0] != ctx.prefix_lens[1] {
            prefix_mismatch = true;
        } else {
            let mut pi = 0usize;
            while pi < ctx.prefix_lens[0] {
                if !prefix_action_eq(ctx.prefix_actions[0][pi], ctx.prefix_actions[1][pi]) {
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

    RouteDispatchOutcome {
        dispatch_len,
        dispatch_functional,
    }
}

#[inline(never)]
fn finalize_route_scope_exit_for_role(
    ctx: &mut RouteFinalizeCtx<'_>,
    role: u8,
    node_len: usize,
    entry_idx: usize,
) -> bool {
    let mut offer_entry_locked = false;
    let scope_id = ctx.scope_entries[entry_idx].scope_id.to_scope_id();
    let is_linger = ctx.scope_entries[entry_idx].linger;
    let is_controller = ctx.scope_controller_roles[entry_idx] == role;
    let scope_end = as_state_index(node_len);

    if !is_linger {
        let arm0_entry = ctx.scope_entries[entry_idx].arm_entry[0];
        let arm1_entry = ctx.scope_entries[entry_idx].arm_entry[1];
        if !arm0_entry.is_max() && !arm1_entry.is_max() {
            let (prefix_end0, prefix_end1, prefix_len) =
                arm_common_prefix_end(ctx.nodes, scope_id, scope_end, arm0_entry, arm1_entry);
            if prefix_len > 0 {
                let parent_scope = if ctx.scope_entries[entry_idx].parent == SCOPE_LINK_NONE {
                    ScopeId::none()
                } else {
                    ctx.scope_entries[ctx.scope_entries[entry_idx].parent as usize]
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
                        let node = ctx.nodes[node_idx];
                        ctx.nodes[node_idx] = node.with_scope(parent_scope).with_route_arm(None);
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
                    ctx.scope_entries[entry_idx].start = min_start;
                }
                if is_controller {
                    ctx.scope_entries[entry_idx].arm_entry[0] = prefix_end0;
                    ctx.scope_entries[entry_idx].arm_entry[1] = prefix_end1;

                    let mut arm = 0u8;
                    while arm < 2 {
                        let entry = ctx.scope_entries[entry_idx].arm_entry[arm as usize];
                        if !entry.is_max() {
                            let node_idx = state_index_to_usize(entry);
                            if node_idx < node_len {
                                match ctx.nodes[node_idx].action() {
                                    LocalAction::Local { .. } => {}
                                    _ => {
                                        ctx.scope_entries[entry_idx].arm_entry[arm as usize] =
                                            StateIndex::MAX;
                                    }
                                }
                            } else {
                                ctx.scope_entries[entry_idx].arm_entry[arm as usize] =
                                    StateIndex::MAX;
                            }
                        }
                        arm += 1;
                    }

                    ctx.route_scope_entries[entry_idx].route_recv =
                        [StateIndex::MAX, StateIndex::MAX];
                    let lane_word_start = ctx.route_scope_entries[entry_idx].lane_word_start();
                    route_scope_lane_words_mut(
                        ctx.route_scope_offer_lane_words,
                        lane_word_start,
                        ctx.route_lane_word_len,
                    )
                    .fill(0);
                    if prefix_end0.raw() != prefix_end1.raw() {
                        let mut arm = 0u8;
                        while arm < 2 {
                            let arm_entry = if arm == 0 { prefix_end0 } else { prefix_end1 };
                            if arm == ctx.route_scope_entries[entry_idx].route_recv_count()
                                && !arm_entry.is_max()
                            {
                                let node_idx = state_index_to_usize(arm_entry);
                                if node_idx < node_len
                                    && let LocalAction::Recv { lane, .. } =
                                        ctx.nodes[node_idx].action()
                                {
                                    ctx.route_scope_entries[entry_idx].route_recv[arm as usize] =
                                        arm_entry;
                                    insert_offer_lane(
                                        route_scope_lane_words_mut(
                                            ctx.route_scope_offer_lane_words,
                                            lane_word_start,
                                            ctx.route_lane_word_len,
                                        ),
                                        lane,
                                    );
                                }
                            }
                            arm += 1;
                        }
                    }
                } else {
                    ctx.scope_entries[entry_idx].arm_entry[0] = prefix_end0;
                    ctx.scope_entries[entry_idx].arm_entry[1] = prefix_end1;
                }
                ctx.route_scope_entries[entry_idx].offer_entry =
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
        clear_dispatch_table(ctx.dispatch_table);
        ctx.route_scope_entries[entry_idx].first_recv_dispatch = *ctx.dispatch_table;
        ctx.route_scope_entries[entry_idx].first_recv_len = 0;
        return offer_entry_locked;
    }

    let dispatch =
        collect_route_dispatch_for_exit(ctx, role, node_len, entry_idx, scope_id, scope_end);

    let arm0_entry = ctx.scope_entries[entry_idx].arm_entry[0];
    let arm1_entry = ctx.scope_entries[entry_idx].arm_entry[1];
    let mergeable = arm_sequences_equal(ctx.nodes, scope_end, arm0_entry, arm1_entry);

    if mergeable {
        ctx.scope_entries[entry_idx].arm_entry[1] = ctx.scope_entries[entry_idx].arm_entry[0];
        clear_dispatch_table(ctx.dispatch_table);
        store_dispatch_summary(
            ctx.nodes,
            &mut ctx.route_scope_entries[entry_idx],
            ctx.dispatch_table,
            0,
        );
    } else if dispatch.dispatch_functional && dispatch.dispatch_len > 0 {
        let dispatch_lane_mask = store_dispatch_summary(
            ctx.nodes,
            &mut ctx.route_scope_entries[entry_idx],
            ctx.dispatch_table,
            dispatch.dispatch_len,
        );
        let offer_lanes = route_scope_lane_words_mut(
            ctx.route_scope_offer_lane_words,
            ctx.route_scope_entries[entry_idx].lane_word_start(),
            ctx.route_lane_word_len,
        );
        insert_offer_lane_mask(offer_lanes, dispatch_lane_mask);
    } else if ctx.scope_route_policy_effs[entry_idx] != EffIndex::MAX {
        clear_dispatch_table(ctx.dispatch_table);
        store_dispatch_summary(
            ctx.nodes,
            &mut ctx.route_scope_entries[entry_idx],
            ctx.dispatch_table,
            0,
        );
    } else {
        panic!(
            "Route unprojectable for this role: arms not mergeable, wire dispatch non-deterministic, and no dynamic policy annotation provided"
        );
    }

    offer_entry_locked
}

#[inline(never)]
fn handle_scope_exit_for_role(
    ctx: &mut RoleWalkCtx<'_>,
    node_len: &mut usize,
    scope_markers: &[crate::global::const_dsl::ScopeMarker],
    scope_marker_idx: usize,
    scope: ScopeId,
    role: u8,
    scope_stack_len: &mut usize,
    scope_entries_len: usize,
    linger_arm_len: usize,
    jump_backpatch_len: &mut usize,
) {
    if *scope_stack_len == 0 {
        panic!("structured scope stack underflow");
    }
    *scope_stack_len -= 1;
    let expected = ctx.scope_stack[*scope_stack_len];
    if expected.local_ordinal() != scope.local_ordinal() {
        panic!("structured scope stack mismatch");
    }
    let entry_idx = ctx.scope_stack_entries[*scope_stack_len] as usize;
    let is_linger = ctx.scope_entries[entry_idx].linger;
    let mut offer_entry_locked = false;

    let next_marker_idx = scope_marker_idx + 1;
    let is_immediate_reenter = next_marker_idx < scope_markers.len()
        && scope_markers[next_marker_idx].offset == scope_markers[scope_marker_idx].offset
        && matches!(scope_markers[next_marker_idx].event, ScopeEvent::Enter)
        && scope_markers[next_marker_idx].scope_id.local_ordinal() == scope.local_ordinal();

    if is_linger {
        let mut linger_idx = 0usize;
        while linger_idx < linger_arm_len {
            if ctx.linger_arm_scope_ids[linger_idx].local_ordinal() == scope.local_ordinal() {
                break;
            }
            linger_idx += 1;
        }

        if linger_idx < linger_arm_len {
            let arm_last = ctx.linger_arm_last_node[linger_idx];
            let loop_start = ctx.scope_entries[entry_idx].start;
            let controller_role = ctx.scope_controller_roles[entry_idx];
            let is_passive = controller_role != CONTROLLER_ROLE_NONE && controller_role != role;
            let passive_starts = if is_passive {
                let arm0_start = if !ctx.scope_entries[entry_idx].arm_entry[0].is_max() {
                    state_index_to_usize(ctx.scope_entries[entry_idx].arm_entry[0])
                } else {
                    usize::from(LINGER_ARM_NO_NODE)
                };
                let arm1_start = if !ctx.scope_entries[entry_idx].arm_entry[1].is_max() {
                    state_index_to_usize(ctx.scope_entries[entry_idx].arm_entry[1])
                } else {
                    usize::from(LINGER_ARM_NO_NODE)
                };
                [arm0_start, arm1_start]
            } else {
                [
                    usize::from(LINGER_ARM_NO_NODE),
                    usize::from(LINGER_ARM_NO_NODE),
                ]
            };

            if is_immediate_reenter {
                if is_passive && passive_starts[0] != usize::from(LINGER_ARM_NO_NODE) {
                    if *node_len >= MAX_STATES {
                        panic!(
                            "node capacity exceeded inserting PassiveObserverBranch Jump for arm 0"
                        );
                    }
                    let continue_target = as_state_index(passive_starts[0]);
                    let jump_node = LocalNode::jump(
                        continue_target,
                        JumpReason::PassiveObserverBranch,
                        scope,
                        Some(scope),
                        Some(0),
                    );
                    ctx.nodes[*node_len] = jump_node;
                    ctx.route_scope_entries[entry_idx].passive_arm_jump[0] =
                        as_state_index(*node_len);
                    *node_len += 1;
                    if arm_last[0] != LINGER_ARM_NO_NODE {
                        if *node_len >= MAX_STATES {
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
                        ctx.nodes[prev_idx] =
                            ctx.nodes[prev_idx].with_next(as_state_index(*node_len));
                        ctx.nodes[*node_len] = jump_node;
                        *node_len += 1;
                    }
                } else if arm_last[0] != LINGER_ARM_NO_NODE {
                    if *node_len >= MAX_STATES {
                        panic!("node capacity exceeded inserting LoopContinue Jump");
                    }
                    let jump_node = LocalNode::jump(
                        loop_start,
                        JumpReason::LoopContinue,
                        scope,
                        Some(scope),
                        Some(0),
                    );
                    let prev_idx = arm_last[0] as usize;
                    ctx.nodes[prev_idx] = ctx.nodes[prev_idx].with_next(as_state_index(*node_len));
                    ctx.nodes[*node_len] = jump_node;
                    *node_len += 1;
                } else if passive_starts[0] != usize::from(LINGER_ARM_NO_NODE) {
                    if *node_len >= MAX_STATES {
                        panic!(
                            "node capacity exceeded inserting PassiveObserverBranch Jump for arm 0"
                        );
                    }
                    let continue_target = as_state_index(passive_starts[0]);
                    let jump_node = LocalNode::jump(
                        continue_target,
                        JumpReason::PassiveObserverBranch,
                        scope,
                        Some(scope),
                        Some(0),
                    );
                    ctx.nodes[*node_len] = jump_node;
                    ctx.route_scope_entries[entry_idx].passive_arm_jump[0] =
                        as_state_index(*node_len);
                    *node_len += 1;
                }
            } else if arm_last[1] != LINGER_ARM_NO_NODE {
                if *node_len >= MAX_STATES {
                    panic!("node capacity exceeded inserting LoopBreak Jump");
                }
                let jump_node = LocalNode::jump(
                    StateIndex::ZERO,
                    JumpReason::LoopBreak,
                    scope,
                    Some(scope),
                    Some(1),
                );
                let prev_idx = arm_last[1] as usize;
                ctx.nodes[prev_idx] = ctx.nodes[prev_idx].with_next(as_state_index(*node_len));
                ctx.nodes[*node_len] = jump_node;
                if *jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                    panic!("jump backpatch capacity exceeded for LoopBreak");
                }
                ctx.jump_backpatch_indices[*jump_backpatch_len] = *node_len as u16;
                ctx.jump_backpatch_scopes[*jump_backpatch_len] = scope;
                ctx.jump_backpatch_kinds[*jump_backpatch_len] = 1;
                *jump_backpatch_len += 1;
                *node_len += 1;
            } else if is_passive && passive_starts[1] != usize::from(LINGER_ARM_NO_NODE) {
                if *node_len >= MAX_STATES {
                    panic!("node capacity exceeded inserting PassiveObserverBranch Jump for arm 1");
                }
                let arm_is_empty = passive_starts[1] == *node_len;
                if *node_len > 0 && passive_starts[1] < *node_len {
                    let arm_last_node = *node_len - 1;
                    if !ctx.nodes[arm_last_node].action().is_jump() {
                        if *jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                            panic!("jump backpatch capacity exceeded for arm last node");
                        }
                        ctx.jump_backpatch_indices[*jump_backpatch_len] = arm_last_node as u16;
                        ctx.jump_backpatch_scopes[*jump_backpatch_len] = scope;
                        ctx.jump_backpatch_kinds[*jump_backpatch_len] = 1;
                        *jump_backpatch_len += 1;
                    }
                }
                let break_target = if arm_is_empty {
                    StateIndex::ZERO
                } else {
                    as_state_index(passive_starts[1])
                };
                let jump_node = LocalNode::jump(
                    break_target,
                    JumpReason::PassiveObserverBranch,
                    scope,
                    Some(scope),
                    Some(1),
                );
                ctx.nodes[*node_len] = jump_node;
                ctx.route_scope_entries[entry_idx].passive_arm_jump[1] = as_state_index(*node_len);
                if arm_is_empty {
                    if *jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                        panic!("jump backpatch capacity exceeded for empty arm");
                    }
                    ctx.jump_backpatch_indices[*jump_backpatch_len] = *node_len as u16;
                    ctx.jump_backpatch_scopes[*jump_backpatch_len] = scope;
                    ctx.jump_backpatch_kinds[*jump_backpatch_len] = 1;
                    *jump_backpatch_len += 1;
                }
                *node_len += 1;
            }
        }
    }

    let controller_role = ctx.scope_controller_roles[entry_idx];
    if !is_linger
        && matches!(ctx.scope_entries[entry_idx].kind, ScopeKind::Route)
        && is_immediate_reenter
    {
        let arm0_is_tau_eliminated = ctx.scope_entries[entry_idx].arm_entry[0].is_max();
        let is_passive = controller_role != CONTROLLER_ROLE_NONE && controller_role != role;

        if *node_len >= MAX_STATES {
            panic!("node capacity exceeded inserting RouteArmEnd Jump for arm 0");
        }
        let jump_node = LocalNode::jump(
            StateIndex::ZERO,
            JumpReason::RouteArmEnd,
            scope,
            None,
            Some(0),
        );
        ctx.nodes[*node_len] = jump_node;
        if is_passive && arm0_is_tau_eliminated {
            ctx.scope_entries[entry_idx].arm_entry[0] = as_state_index(*node_len);
        }
        if *jump_backpatch_len >= MAX_JUMP_BACKPATCH {
            panic!("jump backpatch capacity exceeded for RouteArmEnd Jump");
        }
        ctx.jump_backpatch_indices[*jump_backpatch_len] = *node_len as u16;
        ctx.jump_backpatch_scopes[*jump_backpatch_len] = scope;
        ctx.jump_backpatch_kinds[*jump_backpatch_len] = 2;
        *jump_backpatch_len += 1;
        *node_len += 1;
    }

    if !is_linger
        && matches!(ctx.scope_entries[entry_idx].kind, ScopeKind::Route)
        && !is_immediate_reenter
    {
        let arm1_last = ctx.route_arm_last_node[*scope_stack_len][1];
        let last_was_scope = ctx.last_step_was_scope[*scope_stack_len];
        if !arm1_last.is_max() {
            if *node_len >= MAX_STATES {
                panic!("node capacity exceeded inserting RouteArmEnd Jump for arm 1");
            }
            let jump_node = LocalNode::jump(
                StateIndex::ZERO,
                JumpReason::RouteArmEnd,
                scope,
                None,
                Some(1),
            );
            if last_was_scope {
                ctx.nodes[*node_len] = jump_node;
            } else {
                let prev_idx = state_index_to_usize(arm1_last);
                ctx.nodes[prev_idx] = ctx.nodes[prev_idx].with_next(as_state_index(*node_len));
                ctx.nodes[*node_len] = jump_node;
            }
            if *jump_backpatch_len >= MAX_JUMP_BACKPATCH {
                panic!("jump backpatch capacity exceeded for RouteArmEnd Jump (arm 1)");
            }
            ctx.jump_backpatch_indices[*jump_backpatch_len] = *node_len as u16;
            ctx.jump_backpatch_scopes[*jump_backpatch_len] = scope;
            ctx.jump_backpatch_kinds[*jump_backpatch_len] = 2;
            *jump_backpatch_len += 1;
            *node_len += 1;
        }
    }

    if matches!(ctx.scope_entries[entry_idx].kind, ScopeKind::Route) && !is_immediate_reenter {
        let arm1_has_content = !ctx.scope_entries[entry_idx].arm_entry[1].is_max();
        let is_passive = controller_role != CONTROLLER_ROLE_NONE && controller_role != role;
        if !arm1_has_content {
            if *node_len >= MAX_STATES {
                panic!("node capacity exceeded inserting ArmEmpty placeholder for arm 1");
            }
            let jump_node = if is_linger {
                LocalNode::jump(
                    as_state_index(*node_len + 1),
                    JumpReason::LoopBreak,
                    scope,
                    Some(scope),
                    Some(1),
                )
            } else {
                LocalNode::jump(
                    as_state_index(*node_len + 1),
                    JumpReason::RouteArmEnd,
                    scope,
                    None,
                    Some(1),
                )
            };
            ctx.nodes[*node_len] = jump_node;
            if is_passive {
                ctx.scope_entries[entry_idx].arm_entry[1] = as_state_index(*node_len);
            }
            *node_len += 1;
        }
    }

    if *scope_stack_len > 0 {
        ctx.last_step_was_scope[*scope_stack_len - 1] = true;
    }

    if matches!(ctx.scope_entries[entry_idx].kind, ScopeKind::Route) && !is_immediate_reenter {
        let mut finalize_ctx = RouteFinalizeCtx {
            nodes: ctx.nodes,
            scope_entries: ctx.scope_entries,
            scope_controller_roles: ctx.scope_controller_roles,
            scope_route_policy_effs: ctx.scope_route_policy_effs,
            route_scope_entries: ctx.route_scope_entries,
            route_scope_offer_lane_words: ctx.route_scope_offer_lane_words,
            route_lane_word_len: ctx.route_lane_word_len,
            scope_entries_len,
            dispatch_table: ctx.dispatch_table,
            prefix_actions: ctx.prefix_actions,
            prefix_lens: ctx.prefix_lens,
            arm_seen_recv: ctx.arm_seen_recv,
            scan_stack: ctx.scan_stack,
            visited: ctx.visited,
        };
        offer_entry_locked =
            finalize_route_scope_exit_for_role(&mut finalize_ctx, role, *node_len, entry_idx);
    }

    if matches!(ctx.scope_entries[entry_idx].kind, ScopeKind::Route) && !offer_entry_locked {
        ctx.route_scope_entries[entry_idx].offer_entry = if ctx.scope_entries[entry_idx].linger {
            StateIndex::MAX
        } else {
            ctx.scope_entries[entry_idx].start
        };
    }

    ctx.scope_entries[entry_idx].end = as_state_index(*node_len);
}

#[inline(never)]
fn handle_atom_for_role<P: TypestateProgramView>(
    ctx: &mut RoleWalkCtx<'_>,
    program: &P,
    eff_idx: usize,
    eff: EffStruct,
    role: u8,
    node_len_out: &mut usize,
    current_scope: ScopeId,
    loop_scope: Option<ScopeId>,
    scope_stack_len: usize,
    loop_entry_len_out: &mut usize,
    linger_arm_len: usize,
) {
    let mut node_len = *node_len_out;
    let mut loop_entry_len = *loop_entry_len_out;
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
    let control_semantic = ControlSemanticKind::from_control_spec(control_spec);
    let loop_control = LoopControlMeaning::from_control_spec(control_spec);
    let shot = if atom.is_control {
        match control_spec {
            Some(spec) => Some(spec.shot),
            None => None,
        }
    } else {
        None
    };
    if scope_stack_len > 0 && matches!(ctx.scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
    {
        let entry_idx = ctx.scope_stack_entries[scope_stack_len - 1] as usize;
        if policy.is_dynamic() || loop_control.is_some() {
            insert_offer_lane(
                route_scope_lane_words_mut(
                    ctx.route_scope_offer_lane_words,
                    ctx.route_scope_entries[entry_idx].lane_word_start(),
                    ctx.route_lane_word_len,
                ),
                atom.lane,
            );
        }
    }

    if atom.from == role && atom.to == role {
        let route_arm = if scope_stack_len > 0
            && matches!(ctx.scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
        {
            let stack_idx = scope_stack_len - 1;
            let arm = ctx.route_current_arm[stack_idx] as usize;
            let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;

            let entry = &mut ctx.scope_entries[entry_idx];
            debug_assert!(
                !matches!(entry.kind, ScopeKind::Route)
                    || ctx.scope_controller_roles[entry_idx] != CONTROLLER_ROLE_NONE,
                "route scope missing controller_role"
            );
            if arm < 2 && entry.arm_entry[arm].is_max() {
                entry.arm_entry[arm] = as_state_index(node_len);
            }

            Some(ctx.route_current_arm[stack_idx])
        } else {
            None
        };

        let current_state = as_state_index(node_len);
        let mut next = as_state_index(node_len + 1);
        if matches!(loop_control, Some(LoopControlMeaning::Continue))
            && let Some(scope_id) = loop_scope
            && let Some(entry) = find_loop_entry_state(
                ctx.loop_entry_ids,
                ctx.loop_entry_states,
                loop_entry_len,
                scope_id,
            )
        {
            next = entry;
        }

        ctx.nodes[node_len] = LocalNode::local(
            as_eff_index(eff_idx),
            atom.label,
            atom.resource,
            atom.is_control,
            shot,
            policy,
            atom.lane,
            control_semantic,
            next,
            current_scope,
            loop_scope,
            route_arm,
            false,
        );
        let lane_idx = atom.lane as usize;
        if lane_idx >= ctx.lane_slot_count {
            panic!("scope lane facts missing lane slot capacity");
        }
        let mut stack_idx = 0usize;
        while stack_idx < scope_stack_len {
            let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
            let lane_offset = entry_idx * ctx.lane_slot_count + lane_idx;
            if ctx.scope_lane_first_eff[lane_offset] == EffIndex::MAX {
                ctx.scope_lane_first_eff[lane_offset] = as_eff_index(eff_idx);
            }
            ctx.scope_lane_last_eff[lane_offset] = as_eff_index(eff_idx);
            if matches!(ctx.scope_stack_kinds[stack_idx], ScopeKind::Route) {
                let arm = ctx.route_current_arm[stack_idx] as usize;
                if arm == 0 {
                    ctx.route_arm0_lane_last_eff_by_slot[lane_offset] = as_eff_index(eff_idx);
                } else if arm == 1 {
                    insert_offer_lane(
                        route_scope_lane_words_mut(
                            ctx.route_scope_arm1_lane_words,
                            ctx.route_scope_entries[entry_idx].lane_word_start(),
                            ctx.route_lane_word_len,
                        ),
                        atom.lane,
                    );
                }
            }
            stack_idx += 1;
        }
        if let Some(scope_id) = loop_scope
            && loop_control.is_none()
        {
            store_loop_entry_if_absent(
                ctx.loop_entry_ids,
                ctx.loop_entry_states,
                &mut loop_entry_len,
                scope_id,
                current_state,
            );
        }
        if let Some(scope_id) = loop_scope {
            let mut li = 0;
            while li < linger_arm_len {
                if ctx.linger_arm_scope_ids[li].local_ordinal() == scope_id.local_ordinal() {
                    if matches!(loop_control, Some(LoopControlMeaning::Break)) {
                        ctx.linger_arm_current[li] = 1;
                    }
                    break;
                }
                li += 1;
            }
        }
        if linger_arm_len > 0 {
            let mut stack_idx = 0usize;
            while stack_idx < scope_stack_len {
                let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
                if ctx.scope_entries[entry_idx].linger {
                    let scope_id = ctx.scope_stack[stack_idx];
                    let mut li = 0usize;
                    while li < linger_arm_len {
                        if ctx.linger_arm_scope_ids[li].local_ordinal() == scope_id.local_ordinal()
                        {
                            let arm = ctx.linger_arm_current[li] as usize;
                            if arm < 2 {
                                ctx.linger_arm_last_node[li][arm] = node_len as u16;
                            }
                            break;
                        }
                        li += 1;
                    }
                }
                stack_idx += 1;
            }
        }
        if scope_stack_len > 0
            && matches!(ctx.scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
        {
            let stack_idx = scope_stack_len - 1;
            let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
            if !ctx.scope_entries[entry_idx].linger {
                ctx.last_step_was_scope[stack_idx] = false;
                if let Some(arm) = route_arm
                    && (arm as usize) < 2
                {
                    ctx.route_arm_last_node[stack_idx][arm as usize] = as_state_index(node_len);
                }
            }
        }
        node_len += 1;
    } else if atom.from == role {
        let route_arm = if scope_stack_len > 0
            && matches!(ctx.scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
        {
            let stack_idx = scope_stack_len - 1;
            let arm = ctx.route_current_arm[stack_idx];
            let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
            let controller_role = ctx.scope_controller_roles[entry_idx];
            let is_passive = controller_role != CONTROLLER_ROLE_NONE && controller_role != role;
            if (arm as usize) < 2
                && is_passive
                && ctx.scope_entries[entry_idx].arm_entry[arm as usize].is_max()
            {
                ctx.scope_entries[entry_idx].arm_entry[arm as usize] = as_state_index(node_len);
            }
            Some(arm)
        } else {
            None
        };

        let current_state = as_state_index(node_len);
        let mut next = as_state_index(node_len + 1);
        if matches!(loop_control, Some(LoopControlMeaning::Continue))
            && let Some(scope_id) = loop_scope
            && let Some(entry) = find_loop_entry_state(
                ctx.loop_entry_ids,
                ctx.loop_entry_states,
                loop_entry_len,
                scope_id,
            )
        {
            next = entry;
        }

        ctx.nodes[node_len] = LocalNode::send(
            as_eff_index(eff_idx),
            atom.to,
            atom.label,
            atom.resource,
            atom.is_control,
            shot,
            policy,
            atom.lane,
            control_semantic,
            next,
            current_scope,
            loop_scope,
            route_arm,
            false,
        );
        let lane_idx = atom.lane as usize;
        if lane_idx >= ctx.lane_slot_count {
            panic!("scope lane facts missing lane slot capacity");
        }
        let mut stack_idx = 0usize;
        while stack_idx < scope_stack_len {
            let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
            let lane_offset = entry_idx * ctx.lane_slot_count + lane_idx;
            if ctx.scope_lane_first_eff[lane_offset] == EffIndex::MAX {
                ctx.scope_lane_first_eff[lane_offset] = as_eff_index(eff_idx);
            }
            ctx.scope_lane_last_eff[lane_offset] = as_eff_index(eff_idx);
            if matches!(ctx.scope_stack_kinds[stack_idx], ScopeKind::Route) {
                let arm = ctx.route_current_arm[stack_idx] as usize;
                if arm == 0 {
                    ctx.route_arm0_lane_last_eff_by_slot[lane_offset] = as_eff_index(eff_idx);
                } else if arm == 1 {
                    insert_offer_lane(
                        route_scope_lane_words_mut(
                            ctx.route_scope_arm1_lane_words,
                            ctx.route_scope_entries[entry_idx].lane_word_start(),
                            ctx.route_lane_word_len,
                        ),
                        atom.lane,
                    );
                }
            }
            stack_idx += 1;
        }
        if let Some(scope_id) = loop_scope
            && loop_control.is_none()
        {
            store_loop_entry_if_absent(
                ctx.loop_entry_ids,
                ctx.loop_entry_states,
                &mut loop_entry_len,
                scope_id,
                current_state,
            );
        }
        if linger_arm_len > 0 {
            let mut stack_idx = 0usize;
            while stack_idx < scope_stack_len {
                let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
                if ctx.scope_entries[entry_idx].linger {
                    let scope_id = ctx.scope_stack[stack_idx];
                    let mut li = 0usize;
                    while li < linger_arm_len {
                        if ctx.linger_arm_scope_ids[li].local_ordinal() == scope_id.local_ordinal()
                        {
                            let arm = ctx.linger_arm_current[li] as usize;
                            if arm < 2 {
                                ctx.linger_arm_last_node[li][arm] = node_len as u16;
                            }
                            break;
                        }
                        li += 1;
                    }
                }
                stack_idx += 1;
            }
        }
        if scope_stack_len > 0
            && matches!(ctx.scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
        {
            let stack_idx = scope_stack_len - 1;
            let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
            if !ctx.scope_entries[entry_idx].linger {
                ctx.last_step_was_scope[stack_idx] = false;
                if let Some(arm) = route_arm
                    && (arm as usize) < 2
                {
                    ctx.route_arm_last_node[stack_idx][arm as usize] = as_state_index(node_len);
                }
            }
        }
        node_len += 1;
    } else if atom.to == role {
        let (route_arm, is_choice_determinant) = if scope_stack_len > 0
            && matches!(ctx.scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
        {
            let stack_idx = scope_stack_len - 1;
            let arm = ctx.route_current_arm[stack_idx];
            let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
            let entry = &mut ctx.scope_entries[entry_idx];
            let route_entry = &mut ctx.route_scope_entries[entry_idx];
            let controller_role = ctx.scope_controller_roles[entry_idx];
            let is_passive = controller_role != CONTROLLER_ROLE_NONE && controller_role != role;

            if (arm as usize) < 2 && is_passive {
                let existing = entry.arm_entry[arm as usize];
                let should_set = if existing.is_max() {
                    true
                } else {
                    let existing_node = ctx.nodes[state_index_to_usize(existing)];
                    !matches!(existing_node.action(), LocalAction::Recv { .. })
                };
                if should_set {
                    entry.arm_entry[arm as usize] = as_state_index(node_len);
                }
            }

            let is_first_recv_of_arm = arm == route_entry.route_recv_count();
            if is_first_recv_of_arm && (arm as usize) < 2 {
                let current_state = as_state_index(node_len);
                route_entry.route_recv[arm as usize] = current_state;
                insert_offer_lane(
                    route_scope_lane_words_mut(
                        ctx.route_scope_offer_lane_words,
                        ctx.route_scope_entries[entry_idx].lane_word_start(),
                        ctx.route_lane_word_len,
                    ),
                    atom.lane,
                );
                (Some(arm), true)
            } else {
                (Some(arm), false)
            }
        } else {
            (None, false)
        };

        let current_state = as_state_index(node_len);
        let mut next = as_state_index(node_len + 1);
        if matches!(loop_control, Some(LoopControlMeaning::Continue))
            && let Some(scope_id) = loop_scope
            && let Some(entry) = find_loop_entry_state(
                ctx.loop_entry_ids,
                ctx.loop_entry_states,
                loop_entry_len,
                scope_id,
            )
        {
            next = entry;
        }

        ctx.nodes[node_len] = LocalNode::recv(
            as_eff_index(eff_idx),
            atom.from,
            atom.label,
            atom.resource,
            atom.is_control,
            shot,
            policy,
            atom.lane,
            control_semantic,
            next,
            current_scope,
            loop_scope,
            route_arm,
            is_choice_determinant,
        );
        let lane_idx = atom.lane as usize;
        if lane_idx >= ctx.lane_slot_count {
            panic!("scope lane facts missing lane slot capacity");
        }
        let mut stack_idx = 0usize;
        while stack_idx < scope_stack_len {
            let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
            let lane_offset = entry_idx * ctx.lane_slot_count + lane_idx;
            if ctx.scope_lane_first_eff[lane_offset] == EffIndex::MAX {
                ctx.scope_lane_first_eff[lane_offset] = as_eff_index(eff_idx);
            }
            ctx.scope_lane_last_eff[lane_offset] = as_eff_index(eff_idx);
            if matches!(ctx.scope_stack_kinds[stack_idx], ScopeKind::Route) {
                let arm = ctx.route_current_arm[stack_idx] as usize;
                if arm == 0 {
                    ctx.route_arm0_lane_last_eff_by_slot[lane_offset] = as_eff_index(eff_idx);
                } else if arm == 1 {
                    insert_offer_lane(
                        route_scope_lane_words_mut(
                            ctx.route_scope_arm1_lane_words,
                            ctx.route_scope_entries[entry_idx].lane_word_start(),
                            ctx.route_lane_word_len,
                        ),
                        atom.lane,
                    );
                }
            }
            stack_idx += 1;
        }
        if let Some(scope_id) = loop_scope
            && loop_control.is_none()
        {
            store_loop_entry_if_absent(
                ctx.loop_entry_ids,
                ctx.loop_entry_states,
                &mut loop_entry_len,
                scope_id,
                current_state,
            );
        }
        if linger_arm_len > 0 {
            let mut stack_idx = 0usize;
            while stack_idx < scope_stack_len {
                let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
                if ctx.scope_entries[entry_idx].linger {
                    let scope_id = ctx.scope_stack[stack_idx];
                    let mut li = 0usize;
                    while li < linger_arm_len {
                        if ctx.linger_arm_scope_ids[li].local_ordinal() == scope_id.local_ordinal()
                        {
                            let arm = ctx.linger_arm_current[li] as usize;
                            if arm < 2 {
                                ctx.linger_arm_last_node[li][arm] = node_len as u16;
                            }
                            break;
                        }
                        li += 1;
                    }
                }
                stack_idx += 1;
            }
        }
        if scope_stack_len > 0
            && matches!(ctx.scope_stack_kinds[scope_stack_len - 1], ScopeKind::Route)
        {
            let stack_idx = scope_stack_len - 1;
            let entry_idx = ctx.scope_stack_entries[stack_idx] as usize;
            if !ctx.scope_entries[entry_idx].linger {
                ctx.last_step_was_scope[stack_idx] = false;
                if let Some(arm) = route_arm
                    && (arm as usize) < 2
                {
                    ctx.route_arm_last_node[stack_idx][arm as usize] = as_state_index(node_len);
                }
            }
        }
        node_len += 1;
    }

    *node_len_out = node_len;
    *loop_entry_len_out = loop_entry_len;
}

#[inline(never)]
pub(super) unsafe fn init_role_typestate_value<P: TypestateProgramView>(
    role: u8,
    storage: &mut super::builder::RoleTypestateInitStorage<'_>,
    scratch: &mut RoleTypestateBuildScratch,
    len_dst: *mut u16,
    scope_registry_dst: *mut super::registry::ScopeRegistry,
    program: P,
) {
    let slice = program.as_slice();
    let scope_markers = program.scope_markers();
    let nodes = unsafe { core::slice::from_raw_parts_mut(storage.nodes_ptr, storage.nodes_cap) };
    let lane_slot_count = storage.lane_slot_count;
    let scope_slots_by_scope = storage.scope_slots_by_scope;
    let route_dense_by_slot = storage.route_dense_by_slot;
    let route_records = storage.route_records;
    let route_offer_lane_words = storage.route_offer_lane_words;
    let route_arm1_lane_words = storage.route_arm1_lane_words;
    let route_lane_word_len = storage.route_lane_word_len;
    let route_dispatch_shapes = storage.route_dispatch_shapes;
    let route_dispatch_shape_cap = storage.route_dispatch_shape_cap;
    let route_dispatch_entries = storage.route_dispatch_entries;
    let route_dispatch_entry_cap = storage.route_dispatch_entry_cap;
    let route_dispatch_targets = storage.route_dispatch_targets;
    let route_dispatch_target_cap = storage.route_dispatch_target_cap;
    let route_scope_cap = storage.route_scope_cap;
    let lane_matrix_len = storage
        .scope_records
        .len()
        .saturating_mul(storage.lane_slot_count);
    let scope_lane_first_eff = if lane_matrix_len == 0 {
        &mut []
    } else {
        unsafe { core::slice::from_raw_parts_mut(storage.scope_lane_first_eff, lane_matrix_len) }
    };
    let scope_lane_last_eff = if lane_matrix_len == 0 {
        &mut []
    } else {
        unsafe { core::slice::from_raw_parts_mut(storage.scope_lane_last_eff, lane_matrix_len) }
    };
    let route_arm0_lane_last_eff_by_slot = if lane_matrix_len == 0 {
        &mut []
    } else {
        unsafe {
            core::slice::from_raw_parts_mut(
                storage.route_arm0_lane_last_eff_by_slot,
                lane_matrix_len,
            )
        }
    };
    scope_lane_first_eff.fill(EffIndex::MAX);
    scope_lane_last_eff.fill(EffIndex::MAX);
    route_arm0_lane_last_eff_by_slot.fill(EffIndex::MAX);
    let route_lane_word_cap = storage
        .route_scope_cap
        .saturating_mul(storage.route_lane_word_len);
    let route_scope_offer_lane_words = if route_lane_word_cap == 0 {
        &mut []
    } else {
        unsafe {
            core::slice::from_raw_parts_mut(storage.route_offer_lane_words, route_lane_word_cap)
        }
    };
    let route_scope_arm1_lane_words = if route_lane_word_cap == 0 {
        &mut []
    } else {
        unsafe {
            core::slice::from_raw_parts_mut(storage.route_arm1_lane_words, route_lane_word_cap)
        }
    };
    route_scope_offer_lane_words.fill(0);
    route_scope_arm1_lane_words.fill(0);

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
    let scope_controller_roles = &mut scratch.scope_controller_roles;
    let scope_route_policy_tags = &mut scratch.scope_route_policy_tags;
    let scope_route_policy_ids = &mut scratch.scope_route_policy_ids;
    let scope_route_policy_effs = &mut scratch.scope_route_policy_effs;
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
    let scope_entries = &mut *storage.scope_records;
    let route_scope_entries = &mut scratch.route_scope_entries;
    let mut scope_entries_len = 0usize;
    let mut scope_range_counter: u16 = 0;
    let mut route_scope_lane_word_cursor = 0usize;

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
                        if matches!(marker.scope_kind, ScopeKind::Route) {
                            let lane_word_end = route_scope_lane_word_cursor
                                .checked_add(route_lane_word_len)
                                .expect("route scope lane-word cursor overflow");
                            if lane_word_end > route_scope_offer_lane_words.len()
                                || lane_word_end > route_scope_arm1_lane_words.len()
                            {
                                panic!("route scope lane-word scratch overflow");
                            }
                            let lane_word_start = route_scope_lane_word_cursor;
                            if lane_word_start > u16::MAX as usize {
                                panic!("route scope lane-word start overflow");
                            }
                            route_scope_entries[entry_idx].lane_word_start = lane_word_start as u16;
                            route_scope_lane_words_mut(
                                route_scope_offer_lane_words,
                                lane_word_start,
                                route_lane_word_len,
                            )
                            .fill(0);
                            route_scope_lane_words_mut(
                                route_scope_arm1_lane_words,
                                lane_word_start,
                                route_lane_word_len,
                            )
                            .fill(0);
                            route_scope_lane_word_cursor = lane_word_end;
                        }
                    }
                    scope_stack_len += 1;

                    // Update entry fields (short borrow scope)
                    {
                        let control_parent = if parent_entry == SCOPE_LINK_NONE {
                            SCOPE_LINK_NONE
                        } else {
                            let parent_stack_idx = scope_stack_len - 2;
                            let parent_entry_idx = parent_entry as usize;
                            let parent_record = scope_entries[parent_entry_idx];
                            if matches!(
                                scope_stack_kinds[parent_stack_idx],
                                ScopeKind::Route | ScopeKind::Loop
                            ) {
                                parent_entry
                            } else {
                                parent_record.control_parent
                            }
                        };
                        let (route_parent, route_parent_arm) = if parent_entry == SCOPE_LINK_NONE {
                            (SCOPE_LINK_NONE, super::registry::ROUTE_PARENT_ARM_NONE)
                        } else {
                            let parent_stack_idx = scope_stack_len - 2;
                            let parent_entry_idx = parent_entry as usize;
                            let parent_record = scope_entries[parent_entry_idx];
                            if matches!(scope_stack_kinds[parent_stack_idx], ScopeKind::Route) {
                                (parent_entry, route_current_arm[parent_stack_idx])
                            } else {
                                (parent_record.route_parent, parent_record.route_parent_arm)
                            }
                        };
                        let entry = &mut scope_entries[entry_idx];
                        if marker.linger {
                            entry.linger = true;
                        }
                        if entry.parent != SCOPE_LINK_NONE && entry.parent != parent_entry {
                            panic!("scope parent mismatch for ordinal");
                        }
                        if entry.control_parent != SCOPE_LINK_NONE
                            && entry.control_parent != control_parent
                        {
                            panic!("scope control parent mismatch for ordinal");
                        }
                        if entry.control_parent == SCOPE_LINK_NONE {
                            entry.control_parent = control_parent;
                        }
                        if entry.start.is_max() {
                            entry.start = as_state_index(node_len);
                        }
                        if entry.route_parent != SCOPE_LINK_NONE
                            && (entry.route_parent != route_parent
                                || entry.route_parent_arm != route_parent_arm)
                        {
                            panic!("scope route parent mismatch for ordinal");
                        }
                        if entry.route_parent == SCOPE_LINK_NONE {
                            entry.route_parent = route_parent;
                            entry.route_parent_arm = route_parent_arm;
                        }
                        // Propagate controller_role from ScopeMarker into the shared-atlas scratch.
                        // This keeps the builder's passive/controller decisions detached from the
                        // final role-local scope record payload.
                        if let Some(controller_role) = marker.controller_role
                            && scope_controller_roles[entry_idx] == CONTROLLER_ROLE_NONE
                        {
                            scope_controller_roles[entry_idx] = controller_role;
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
                            let parent_controller_role = scope_controller_roles[parent_entry_idx];
                            let parent_is_passive = parent_controller_role != CONTROLLER_ROLE_NONE
                                && parent_controller_role != role;
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

                        // At first Enter (enter_count == 1), read route policy from the lowering view.
                        // This keeps route policy metadata independent of role projection.
                        if route_enter_count[stack_idx] == 1
                            && scope_route_policy_effs[entry_idx] == EffIndex::MAX
                        {
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
                            if let Some((policy, eff_offset, tag, _op)) = program
                                .first_route_head_dynamic_policy_in_range(
                                    scope,
                                    scope_marker_idx,
                                    scope_end,
                                )
                            {
                                scope_route_policy_ids[entry_idx] = policy
                                    .dynamic_policy_id()
                                    .expect("route policy marker must be dynamic");
                                scope_route_policy_effs[entry_idx] = as_eff_index(eff_offset);
                                scope_route_policy_tags[entry_idx] = tag;
                            }
                        }
                    }
                }
                ScopeEvent::Exit => {
                    let mut walk_ctx = RoleWalkCtx {
                        nodes,
                        scope_entries,
                        route_scope_entries,
                        route_scope_offer_lane_words,
                        route_scope_arm1_lane_words,
                        route_lane_word_len,
                        lane_slot_count,
                        scope_lane_first_eff,
                        scope_lane_last_eff,
                        route_arm0_lane_last_eff_by_slot,
                        loop_entry_ids,
                        loop_entry_states,
                        linger_arm_scope_ids,
                        linger_arm_current,
                        linger_arm_last_node,
                        jump_backpatch_indices,
                        jump_backpatch_scopes,
                        jump_backpatch_kinds,
                        scope_stack,
                        scope_stack_kinds,
                        scope_stack_entries,
                        route_current_arm,
                        scope_controller_roles,
                        scope_route_policy_effs,
                        last_step_was_scope,
                        route_arm_last_node,
                        dispatch_table: &mut scratch.dispatch_table,
                        prefix_actions: &mut scratch.prefix_actions,
                        prefix_lens: &mut scratch.prefix_lens,
                        arm_seen_recv: &mut scratch.arm_seen_recv,
                        scan_stack: &mut scratch.scan_stack,
                        visited: &mut scratch.visited,
                    };
                    handle_scope_exit_for_role(
                        &mut walk_ctx,
                        &mut node_len,
                        scope_markers,
                        scope_marker_idx,
                        scope,
                        role,
                        &mut scope_stack_len,
                        scope_entries_len,
                        linger_arm_len,
                        &mut jump_backpatch_len,
                    );
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
            let mut walk_ctx = RoleWalkCtx {
                nodes,
                scope_entries,
                route_scope_entries,
                route_scope_offer_lane_words,
                route_scope_arm1_lane_words,
                route_lane_word_len,
                lane_slot_count,
                scope_lane_first_eff,
                scope_lane_last_eff,
                route_arm0_lane_last_eff_by_slot,
                loop_entry_ids,
                loop_entry_states,
                linger_arm_scope_ids,
                linger_arm_current,
                linger_arm_last_node,
                jump_backpatch_indices,
                jump_backpatch_scopes,
                jump_backpatch_kinds,
                scope_stack,
                scope_stack_kinds,
                scope_stack_entries,
                route_current_arm,
                scope_controller_roles,
                scope_route_policy_effs,
                last_step_was_scope,
                route_arm_last_node,
                dispatch_table: &mut scratch.dispatch_table,
                prefix_actions: &mut scratch.prefix_actions,
                prefix_lens: &mut scratch.prefix_lens,
                arm_seen_recv: &mut scratch.arm_seen_recv,
                scan_stack: &mut scratch.scan_stack,
                visited: &mut scratch.visited,
            };
            handle_atom_for_role(
                &mut walk_ctx,
                &program,
                eff_idx,
                eff,
                role,
                &mut node_len,
                current_scope,
                loop_scope,
                scope_stack_len,
                &mut loop_entry_len,
                linger_arm_len,
            );
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
            route_offer_lane_words,
            route_arm1_lane_words,
            route_lane_word_len,
            route_dispatch_shapes,
            route_dispatch_shape_cap,
            route_dispatch_entries,
            route_dispatch_entry_cap,
            route_dispatch_targets,
            route_dispatch_target_cap,
            route_scope_entries.as_mut_ptr(),
            lane_slot_count,
            scope_lane_first_eff.as_mut_ptr(),
            scope_lane_last_eff.as_mut_ptr(),
            route_arm0_lane_last_eff_by_slot.as_mut_ptr(),
            scope_entries_len,
        );
    }
}
