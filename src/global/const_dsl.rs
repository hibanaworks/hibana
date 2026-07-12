//! Const helpers for building segmented `EffStruct` images at compile time.
//!
//! These helpers lower global combinators (`send/seq/par/route`) into a
//! segment-addressed `EffList` read through crate-private accessors.
//!
//! # Unsafe Owner Contract
//!
//! `EffList` owns fixed arrays of compile-time metadata markers. The only raw
//! slice construction in this module exposes initialized prefixes whose lengths
//! are advanced by the same const builder methods that write the backing rows.
//! No returned slice outlives `self`, and no method exposes mutable aliases to
//! those rows while a shared prefix view exists.
mod eff_list;
mod endpoint_controller;
mod endpoint_selectors;
mod receive_lane_causality;
mod route;
mod scope_ranges;

use crate::eff::{self, EffStruct};
use crate::global::Message;

const MAX_SEGMENT_EFFS: usize = eff::meta::MAX_SEGMENT_EFFS;
const MAX_SEGMENTS: usize = eff::meta::MAX_SEGMENTS;
const MAX_CAPACITY: usize = eff::meta::MAX_EFF_NODES;
const MAX_ROUTE_RESOLVER_MARKERS: usize = MAX_CAPACITY / 2;

mod scope;

pub(crate) use self::eff_list::{ScopeRebase, const_send_typed};
pub(crate) use self::endpoint_controller::first_visible_controller_mask;
pub(crate) use self::endpoint_selectors::{
    first_visible_endpoint_selector_conflicts_from_markers, local_route_observer_paths_mergeable,
    validate_parallel_endpoint_selectors, validate_roll_reentry_endpoint_selectors,
};
pub(crate) use self::receive_lane_causality::validate_receive_lane_causality;
pub(crate) use self::route::{DynamicRouteResolver, ReentryMark, RouteResolverMarker};
pub(crate) use self::scope::{ScopeEvent, ScopeId, ScopeKind};
pub(crate) use self::scope_ranges::{
    parallel_arm_ranges_from_enter, route_arm_ranges_from_first_enter,
};

pub(crate) const INTRINSIC_ROUTE_RESOLVER_ID: u16 = u16::MAX;

#[derive(Clone, Copy)]
pub(crate) struct ScopeMarker {
    offset: u16,
    pub(crate) scope_id: ScopeId,
    pub(crate) event: ScopeEvent,
    pub(crate) reentry: ReentryMark,
}

impl ScopeMarker {
    pub(crate) const fn empty() -> Self {
        Self {
            offset: 0,
            scope_id: ScopeId::none(),
            event: ScopeEvent::Enter,
            reentry: ReentryMark::SinglePass,
        }
    }

    pub(crate) const fn new(
        offset: usize,
        scope_id: ScopeId,
        event: ScopeEvent,
        reentry: ReentryMark,
    ) -> Self {
        if offset > u16::MAX as usize {
            panic!("scope marker offset overflow");
        }
        Self {
            offset: offset as u16,
            scope_id,
            event,
            reentry,
        }
    }

    #[inline(always)]
    pub(crate) const fn offset(self) -> usize {
        self.offset as usize
    }
}

/// Accumulator used to build `EffStruct` sequences in const contexts.
pub(crate) struct EffList {
    segments: [[EffStruct; MAX_SEGMENT_EFFS]; MAX_SEGMENTS],
    len: usize,
    scope_budget: u16,
    scope_markers: [ScopeMarker; MAX_CAPACITY],
    scope_marker_len: usize,
    resolver_markers: [RouteResolverMarker; MAX_ROUTE_RESOLVER_MARKERS],
    resolver_marker_len: usize,
}
