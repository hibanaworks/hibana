//! Typestate owner and validation facade.

#[cfg(test)]
use super::facts::MAX_STATES;
use super::{
    facts::{LocalNode, StateIndex},
    registry::{
        RouteDispatchEntry, RouteDispatchShape, RouteScopeRecord, ScopeRecord, ScopeRegistry,
    },
};
use crate::{eff::EffIndex, global::role_program::LaneWord};

pub(crate) use super::registry::{ARM_SHARED, MAX_FIRST_RECV_DISPATCH, ScopeRegion};

#[inline(always)]
pub(super) const fn encode_typestate_len(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("typestate length overflow");
    }
    value as u16
}

/// Role-specific typestate graph synthesized from a global effect list.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleTypestate<const ROLE: u8> {
    pub(super) nodes: [LocalNode; MAX_STATES],
    pub(super) len: u16,
    pub(super) scope_registry: ScopeRegistry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RoleTypestateValue {
    pub(super) nodes: *const LocalNode,
    pub(super) len: u16,
    pub(super) scope_registry: ScopeRegistry,
}

pub(crate) struct RoleTypestateInitStorage<'a> {
    pub(crate) nodes_ptr: *mut LocalNode,
    pub(crate) nodes_cap: usize,
    pub(crate) scope_records: &'a mut [ScopeRecord],
    pub(crate) scope_slots_by_scope: *mut u16,
    pub(crate) route_dense_by_slot: *mut u16,
    pub(crate) route_records: *mut RouteScopeRecord,
    pub(crate) route_offer_lane_words: *mut LaneWord,
    pub(crate) route_arm1_lane_words: *mut LaneWord,
    pub(crate) route_lane_word_len: usize,
    pub(crate) route_dispatch_shapes: *mut RouteDispatchShape,
    pub(crate) route_dispatch_shape_cap: usize,
    pub(crate) route_dispatch_entries: *mut RouteDispatchEntry,
    pub(crate) route_dispatch_entry_cap: usize,
    pub(crate) route_dispatch_targets: *mut StateIndex,
    pub(crate) route_dispatch_target_cap: usize,
    pub(crate) lane_slot_count: usize,
    pub(crate) scope_lane_first_eff: *mut EffIndex,
    pub(crate) scope_lane_last_eff: *mut EffIndex,
    pub(crate) route_arm0_lane_last_eff_by_slot: *mut EffIndex,
    pub(crate) route_scope_cap: usize,
}
