use super::{
    facts::{
        MAX_STATES, RouteRecvIndex, SCOPE_ORDINAL_INDEX_CAPACITY, SCOPE_ORDINAL_INDEX_EMPTY,
        StateIndex,
    },
    registry::{ScopeEntry, ScopeRegistry},
    route_facts::RouteRecvNode,
};
use crate::{
    eff,
    global::const_dsl::{ScopeId, ScopeKind},
};

pub(super) const fn alloc_scope_entry(
    scope_entries: &mut [ScopeEntry; eff::meta::MAX_EFF_NODES],
    scope_entries_len: &mut usize,
    scope_entry_index_by_ordinal: &mut [u16; SCOPE_ORDINAL_INDEX_CAPACITY],
    scope_range_counter: &mut u16,
    scope: ScopeId,
    scope_kind: ScopeKind,
    linger: bool,
    parent_scope: ScopeId,
    nest: usize,
) -> (usize, bool) {
    let ordinal = scope.local_ordinal() as usize;
    if ordinal >= SCOPE_ORDINAL_INDEX_CAPACITY {
        panic!("scope ordinal exceeds typestate capacity");
    }
    match scope_entry_index_by_ordinal[ordinal] {
        SCOPE_ORDINAL_INDEX_EMPTY => {
            if *scope_entries_len >= eff::meta::MAX_EFF_NODES {
                panic!("structured scope metadata overflow");
            }
            if *scope_range_counter == u16::MAX {
                panic!("scope range ordinal overflow");
            }
            scope_entry_index_by_ordinal[ordinal] = *scope_entries_len as u16;
            let idx = *scope_entries_len;
            scope_entries[idx] = ScopeEntry::EMPTY;
            scope_entries[idx].scope_id = scope;
            scope_entries[idx].kind = scope_kind;
            scope_entries[idx].linger = linger;
            scope_entries[idx].parent = parent_scope;
            scope_entries[idx].range = *scope_range_counter;
            scope_entries[idx].nest = nest as u16;
            *scope_range_counter = scope_range_counter.wrapping_add(1);
            *scope_entries_len += 1;
            (idx, true)
        }
        existing => (existing as usize, false),
    }
}

pub(super) const fn finalize_scope_registry(
    scope_entries: &mut [ScopeEntry; eff::meta::MAX_EFF_NODES],
    scope_entries_len: usize,
    route_recv_nodes: &[RouteRecvNode; MAX_STATES],
    route_recv_nodes_len: usize,
) -> ScopeRegistry {
    let mut route_recv_flat = [StateIndex::MAX; MAX_STATES];
    let mut route_recv_flat_len = 0usize;
    let mut entry_idx = 0usize;
    while entry_idx < scope_entries_len {
        if scope_entries[entry_idx].route_recv_len > 0 {
            scope_entries[entry_idx].route_recv_offset =
                RouteRecvIndex::from_usize(route_recv_flat_len);
            let mut remaining = scope_entries[entry_idx].route_recv_len;
            let mut cursor = scope_entries[entry_idx].route_recv_head;
            while remaining > 0 {
                if cursor.is_max() {
                    panic!("route recv list truncated");
                }
                if route_recv_flat_len >= MAX_STATES {
                    panic!("route recv table overflow");
                }
                let node = route_recv_nodes[cursor.as_usize()];
                route_recv_flat[route_recv_flat_len] = node.state;
                route_recv_flat_len += 1;
                cursor = node.next;
                remaining -= 1;
            }
        } else {
            scope_entries[entry_idx].route_recv_offset =
                RouteRecvIndex::from_usize(route_recv_flat_len);
        }
        entry_idx += 1;
    }

    if route_recv_flat_len > route_recv_nodes_len {
        panic!("route recv registry length exceeded recorded route recv nodes");
    }

    ScopeRegistry::from_scope_entries(
        *scope_entries,
        scope_entries_len,
        route_recv_flat,
        route_recv_flat_len,
    )
}
